/* player.rs
 *
 * Copyright 2026 Will
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gst::glib;
use gst::prelude::*;

use super::graph::{CALL_STREAM_PREFIX, MONITOR_STREAM_PREFIX};
use super::{Config, Error, Event, PlaybackId};

// One queue per branch so a slow sink cannot stall the other one.
const PIPELINE: &str = "uridecodebin name=source ! audioconvert ! audioresample ! tee name=tee \
    tee. ! queue ! volume name=call-volume ! pipewiresink name=call \
    tee. ! queue ! volume name=monitor-volume ! pipewiresink name=monitor";

type Pipelines = Arc<Mutex<HashMap<PlaybackId, gst::Pipeline>>>;

/// What every new pipeline starts with.
struct Mix {
    monitor: Option<String>,
    call_volume: f64,
    monitor_volume: f64,
    send_to_call: bool,
}

pub(super) struct Player {
    events: async_channel::Sender<Event>,
    pipelines: Pipelines,
    next_id: AtomicU64,
    mix: Mutex<Mix>,
}

impl Player {
    pub(super) fn new(
        config: &Config,
        events: async_channel::Sender<Event>,
    ) -> Result<Self, Error> {
        gst::init().map_err(|error| Error::GStreamer(error.to_string()))?;
        Ok(Self {
            events,
            pipelines: Pipelines::default(),
            next_id: AtomicU64::new(0),
            mix: Mutex::new(Mix {
                monitor: config.monitor.clone(),
                call_volume: config.call_volume.clamp(0.0, 1.0),
                monitor_volume: config.monitor_volume.clamp(0.0, 1.0),
                send_to_call: config.send_to_call,
            }),
        })
    }

    pub(super) fn play(&self, path: &Path) -> Result<PlaybackId, Error> {
        let id = PlaybackId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let playback_error = |message: String| Error::Playback {
            id,
            path: path.to_owned(),
            message,
        };

        let absolute = std::path::absolute(path).map_err(|e| playback_error(e.to_string()))?;
        if !absolute.is_file() {
            return Err(Error::FileNotFound(path.to_owned()));
        }
        let uri =
            glib::filename_to_uri(&absolute, None).map_err(|e| playback_error(e.to_string()))?;

        let pipeline = gst::parse::launch(PIPELINE)
            .map_err(|e| playback_error(e.to_string()))?
            .downcast::<gst::Pipeline>()
            .expect("a parsed pipeline with several elements is a gst::Pipeline");
        let element = |name: &str| pipeline.by_name(name).expect("element named in PIPELINE");

        let mix = self.mix.lock().unwrap();
        element("source").set_property("uri", uri.as_str());
        element("call-volume").set_property("volume", mix.call_volume);
        element("call-volume").set_property("mute", !mix.send_to_call);
        element("monitor-volume").set_property("volume", mix.monitor_volume);

        // WirePlumber does not route a playback stream to an Audio/Source/Virtual
        // node, so the call branch stays unconnected and the graph thread links it.
        let suffix = format!("{}-{}", std::process::id(), id.0);
        let call = element("call");
        call.set_property("client-name", "Vinheta");
        call.set_property(
            "stream-properties",
            gst::Structure::builder("props")
                .field("node.name", format!("{CALL_STREAM_PREFIX}{suffix}"))
                .field("node.autoconnect", "false")
                .field("state.restore-props", "false")
                .build(),
        );

        let monitor = element("monitor");
        monitor.set_property("client-name", "Vinheta");
        monitor.set_property(
            "stream-properties",
            gst::Structure::builder("props")
                .field("node.name", format!("{MONITOR_STREAM_PREFIX}{suffix}"))
                .field("state.restore-props", "false")
                .build(),
        );
        if let Some(target) = &mix.monitor {
            monitor.set_property("target-object", target);
        }
        drop(mix);

        let bus = pipeline.bus().expect("a pipeline always has a bus");
        let finisher = Finisher {
            id,
            path: path.to_owned(),
            pipelines: self.pipelines.clone(),
            events: self.events.clone(),
        };
        // A sync handler instead of a bus watch: it needs no main loop on the
        // caller's side and no guard object to keep alive.
        bus.set_sync_handler(move |_, message| {
            match message.view() {
                gst::MessageView::Eos(_) => finisher.finish(None),
                gst::MessageView::Error(error) => finisher.finish(Some(error.error().to_string())),
                _ => {}
            }
            gst::BusSyncReply::Drop
        });

        self.pipelines.lock().unwrap().insert(id, pipeline.clone());
        if pipeline.set_state(gst::State::Playing).is_err() {
            // The bus error may have removed the pipeline already and reported it.
            if self.pipelines.lock().unwrap().remove(&id).is_some() {
                let _ = pipeline.set_state(gst::State::Null);
                return Err(playback_error("the pipeline refused to start".into()));
            }
        }
        Ok(id)
    }

    // Removing the pipeline from the map first is what keeps a stopped
    // playback from reporting an event afterwards.
    pub(super) fn stop(&self, id: PlaybackId) {
        let pipeline = self.pipelines.lock().unwrap().remove(&id);
        if let Some(pipeline) = pipeline {
            let _ = pipeline.set_state(gst::State::Null);
        }
    }

    pub(super) fn set_send_to_call(&self, enabled: bool) {
        self.mix.lock().unwrap().send_to_call = enabled;
        self.set_on_pipelines("call-volume", "mute", !enabled);
    }

    pub(super) fn set_call_volume(&self, gain: f64) {
        let gain = gain.clamp(0.0, 1.0);
        self.mix.lock().unwrap().call_volume = gain;
        self.set_on_pipelines("call-volume", "volume", gain);
    }

    pub(super) fn set_monitor_volume(&self, gain: f64) {
        let gain = gain.clamp(0.0, 1.0);
        self.mix.lock().unwrap().monitor_volume = gain;
        self.set_on_pipelines("monitor-volume", "volume", gain);
    }

    /// Only affects the next playbacks; the graph thread moves the running ones.
    pub(super) fn set_monitor(&self, name: Option<String>) {
        self.mix.lock().unwrap().monitor = name;
    }

    fn set_on_pipelines(&self, element: &str, property: &str, value: impl Into<glib::Value>) {
        let value = value.into();
        for pipeline in self.pipelines.lock().unwrap().values() {
            if let Some(element) = pipeline.by_name(element) {
                element.set_property_from_value(property, &value);
            }
        }
    }

    pub(super) fn stop_all(&self) {
        let pipelines: Vec<_> = self.pipelines.lock().unwrap().drain().collect();
        for (_, pipeline) in pipelines {
            let _ = pipeline.set_state(gst::State::Null);
        }
    }
}

struct Finisher {
    id: PlaybackId,
    path: PathBuf,
    pipelines: Pipelines,
    events: async_channel::Sender<Event>,
}

impl Finisher {
    fn finish(&self, error: Option<String>) {
        let Some(pipeline) = self.pipelines.lock().unwrap().remove(&self.id) else {
            return;
        };
        // This runs on a streaming thread, which cannot stop its own pipeline.
        pipeline.call_async(|pipeline| {
            let _ = pipeline.set_state(gst::State::Null);
        });
        let event = match error {
            None => Event::PlaybackFinished {
                id: self.id,
                path: self.path.clone(),
            },
            Some(message) => Event::Error(Error::Playback {
                id: self.id,
                path: self.path.clone(),
                message,
            }),
        };
        let _ = self.events.try_send(event);
    }
}
