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

use super::graph::CALL_STREAM_PREFIX;
use super::{Config, Error, Event};

// One queue per branch so a slow sink cannot stall the other one.
const PIPELINE: &str = "uridecodebin name=source ! audioconvert ! audioresample ! tee name=tee \
    tee. ! queue ! volume name=call-volume ! pipewiresink name=call \
    tee. ! queue ! volume name=monitor-volume ! pipewiresink name=monitor";

type Pipelines = Arc<Mutex<HashMap<u64, gst::Pipeline>>>;

pub(super) struct Player {
    events: async_channel::Sender<Event>,
    pipelines: Pipelines,
    next_id: AtomicU64,
    monitor: Option<String>,
    call_volume: f64,
    monitor_volume: f64,
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
            monitor: config.monitor.clone(),
            call_volume: config.call_volume,
            monitor_volume: config.monitor_volume,
        })
    }

    pub(super) fn play(&self, path: &Path) {
        if let Err(error) = self.try_play(path) {
            let _ = self.events.try_send(Event::Error(error));
        }
    }

    fn try_play(&self, path: &Path) -> Result<(), Error> {
        let playback_error = |message: String| Error::Playback {
            path: path.to_owned(),
            message,
        };

        let absolute = std::path::absolute(path).map_err(|e| playback_error(e.to_string()))?;
        if !absolute.is_file() {
            return Err(Error::FileNotFound(path.to_owned()));
        }
        let uri =
            glib::filename_to_uri(&absolute, None).map_err(|e| playback_error(e.to_string()))?;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let pipeline = gst::parse::launch(PIPELINE)
            .map_err(|e| playback_error(e.to_string()))?
            .downcast::<gst::Pipeline>()
            .expect("a parsed pipeline with several elements is a gst::Pipeline");
        let element = |name: &str| pipeline.by_name(name).expect("element named in PIPELINE");

        element("source").set_property("uri", uri.as_str());
        element("call-volume").set_property("volume", self.call_volume);
        element("monitor-volume").set_property("volume", self.monitor_volume);

        // WirePlumber does not route a playback stream to an Audio/Source/Virtual
        // node, so the call branch stays unconnected and the graph thread links it.
        let stream_name = format!("{CALL_STREAM_PREFIX}{}-{id}", std::process::id());
        let call = element("call");
        call.set_property("client-name", "Vinheta");
        call.set_property(
            "stream-properties",
            gst::Structure::builder("props")
                .field("node.name", stream_name)
                .field("node.autoconnect", "false")
                .field("state.restore-props", "false")
                .build(),
        );

        let monitor = element("monitor");
        monitor.set_property("client-name", "Vinheta");
        monitor.set_property(
            "stream-properties",
            gst::Structure::builder("props")
                .field("state.restore-props", "false")
                .build(),
        );
        if let Some(target) = &self.monitor {
            monitor.set_property("target-object", target);
        }

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
        Ok(())
    }

    pub(super) fn stop_all(&self) {
        let pipelines: Vec<_> = self.pipelines.lock().unwrap().drain().collect();
        for (_, pipeline) in pipelines {
            let _ = pipeline.set_state(gst::State::Null);
        }
    }
}

struct Finisher {
    id: u64,
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
            None => Event::PlaybackFinished(self.path.clone()),
            Some(message) => Event::Error(Error::Playback {
                path: self.path.clone(),
                message,
            }),
        };
        let _ = self.events.try_send(event);
    }
}
