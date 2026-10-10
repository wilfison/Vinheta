/* player.rs
 *
 * Copyright 2026 wilfison
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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gst::glib;
use gst::prelude::*;

use super::graph::{stream_prefix, CALL_STREAM_PREFIX, MONITOR_STREAM_PREFIX};
use super::limiter::Limiter;
use super::{Config, Error, Event, PlayOptions, PlaybackId, Position};

// One queue per branch so a slow sink cannot stall the other one. The queues
// are short so the end of a pass is known shortly before it is heard. The
// gains go after them: a volume before the tee would be heard a queue late.
// The call branch is mono, so it fits a recording app of any channel layout.
// Its limiter is a probe on `call-format`: after the volume, so the ceiling
// does not follow the slider, and after the tee, so the monitor is untouched.
const PIPELINE: &str = "uridecodebin name=source ! audioconvert ! audioresample ! tee name=tee \
    tee. ! queue max-size-buffers=0 max-size-bytes=0 max-size-time=200000000 \
        ! audioconvert ! audio/x-raw,channels=1 \
        ! volume name=call-volume \
        ! capsfilter name=call-format caps=audio/x-raw,format=F32LE \
        ! pipewiresink name=call \
    tee. ! queue max-size-buffers=0 max-size-bytes=0 max-size-time=200000000 \
        ! volume name=monitor-volume ! pipewiresink name=monitor";

const FADE_STEP: Duration = Duration::from_millis(10);
// The sinks still hold some audio when the ramp reaches zero.
const FADE_TAIL: Duration = Duration::from_millis(150);

// -6 dBFS: a loud sound leaves half of the scale to the voice.
const CALL_CEILING: f32 = 0.5;
const CALL_RELEASE: Duration = Duration::from_millis(200);
const DEFAULT_RATE: u32 = 48000;

/// The gain of a branch element: the branch, the playback, and the fade
/// multiplied, each from 0.0 to 1.0.
pub(super) fn effective_gain(branch: f64, playback: f64, fade: f64) -> f64 {
    branch.clamp(0.0, 1.0) * playback.clamp(0.0, 1.0) * fade.clamp(0.0, 1.0)
}

/// What a playback shares with its bus handler and with queued actions.
struct Control {
    looping: AtomicBool,
    /// Set once the pipeline was asked to play, after the first seek.
    started: AtomicBool,
    /// Set before the pipeline goes to `Null`, so that an action queued by
    /// the bus handler never revives it.
    retired: Mutex<bool>,
}

impl Control {
    fn retire(&self, pipeline: &gst::Pipeline) {
        *self.retired.lock().unwrap() = true;
        let _ = pipeline.set_state(gst::State::Null);
    }
}

struct Playback {
    pipeline: gst::Pipeline,
    control: Arc<Control>,
    volume: f64,
    limit_call: Arc<AtomicBool>,
}

impl Playback {
    fn apply_gains(&self, mix: &Mix, fade: f64) {
        set_gains(
            &self.pipeline,
            effective_gain(mix.call_volume, self.volume, fade),
            effective_gain(mix.monitor_volume, self.volume, fade),
        );
    }
}

fn set_gains(pipeline: &gst::Pipeline, call: f64, monitor: f64) {
    for (name, gain) in [("call-volume", call), ("monitor-volume", monitor)] {
        if let Some(element) = pipeline.by_name(name) {
            element.set_property("volume", gain);
        }
    }
}

type Playbacks = Arc<Mutex<HashMap<PlaybackId, Playback>>>;

/// A stopped playback on its way to silence. It is no longer in the map of
/// playbacks, so it reports nothing and no volume change reaches it.
struct Fade {
    playback: Playback,
    call_volume: f64,
    monitor_volume: f64,
    started: Instant,
    duration: Duration,
}

impl Fade {
    /// Returns false when the fade is over and the pipeline was stopped.
    fn step(&self) -> bool {
        let elapsed = self.started.elapsed();
        if elapsed >= self.duration + FADE_TAIL {
            self.playback.control.retire(&self.playback.pipeline);
            return false;
        }
        let fade = 1.0 - elapsed.as_secs_f64() / self.duration.as_secs_f64();
        set_gains(
            &self.playback.pipeline,
            effective_gain(self.call_volume, self.playback.volume, fade),
            effective_gain(self.monitor_volume, self.playback.volume, fade),
        );
        true
    }
}

fn run_fades(receiver: mpsc::Receiver<Fade>) {
    let mut fades: Vec<Fade> = Vec::new();
    loop {
        let received = if fades.is_empty() {
            receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            receiver.recv_timeout(FADE_STEP)
        };
        match received {
            Ok(fade) => fades.push(fade),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        fades.retain(Fade::step);
    }
    for fade in fades {
        fade.playback.control.retire(&fade.playback.pipeline);
    }
}

/// What every new pipeline starts with.
struct Mix {
    monitor: Option<String>,
    call_volume: f64,
    monitor_volume: f64,
    send_to_call: bool,
    limit_call: bool,
    fade_out: Duration,
}

pub(super) struct Player {
    events: async_channel::Sender<Event>,
    playbacks: Playbacks,
    next_id: AtomicU64,
    mix: Mutex<Mix>,
    fades: Mutex<Option<mpsc::Sender<Fade>>>,
    fade_thread: Mutex<Option<JoinHandle<()>>>,
}

impl Player {
    pub(super) fn new(
        config: &Config,
        events: async_channel::Sender<Event>,
    ) -> Result<Self, Error> {
        gst::init().map_err(|error| Error::GStreamer(error.to_string()))?;
        let (fades, fade_receiver) = mpsc::channel();
        let fade_thread = std::thread::Builder::new()
            .name("vinheta-fade".into())
            .spawn(move || run_fades(fade_receiver))
            .map_err(|error| Error::GStreamer(error.to_string()))?;
        Ok(Self {
            events,
            playbacks: Playbacks::default(),
            next_id: AtomicU64::new(0),
            mix: Mutex::new(Mix {
                monitor: config.monitor.clone(),
                call_volume: config.call_volume.clamp(0.0, 1.0),
                monitor_volume: config.monitor_volume.clamp(0.0, 1.0),
                send_to_call: config.send_to_call,
                limit_call: config.limit_call,
                fade_out: config.fade_out,
            }),
            fades: Mutex::new(Some(fades)),
            fade_thread: Mutex::new(Some(fade_thread)),
        })
    }

    pub(super) fn play(&self, path: &Path, options: PlayOptions) -> Result<PlaybackId, Error> {
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

        let control = Arc::new(Control {
            looping: AtomicBool::new(options.looping),
            started: AtomicBool::new(false),
            retired: Mutex::new(false),
        });
        let mix = self.mix.lock().unwrap();
        let playback = Playback {
            pipeline: pipeline.clone(),
            control: control.clone(),
            volume: options.volume.clamp(0.0, 1.0),
            limit_call: Arc::new(AtomicBool::new(mix.limit_call)),
        };

        element("source").set_property("uri", uri.as_str());
        limit(
            &element("call-format")
                .static_pad("src")
                .expect("a capsfilter has a src pad"),
            playback.limit_call.clone(),
        );
        element("call-volume").set_property("mute", !mix.send_to_call);
        playback.apply_gains(&mix, 1.0);

        // WirePlumber must not route the call branch to an output: the graph
        // thread links it to the drain node and to the recording apps.
        let call = element("call");
        call.set_property("client-name", "Vinheta");
        call.set_property(
            "stream-properties",
            gst::Structure::builder("props")
                .field(
                    "node.name",
                    format!("{}{}", stream_prefix(CALL_STREAM_PREFIX), id.0),
                )
                .field("node.autoconnect", "false")
                .field("state.restore-props", "false")
                .build(),
        );

        let monitor = element("monitor");
        monitor.set_property("client-name", "Vinheta");
        monitor.set_property(
            "stream-properties",
            gst::Structure::builder("props")
                .field(
                    "node.name",
                    format!("{}{}", stream_prefix(MONITOR_STREAM_PREFIX), id.0),
                )
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
            playbacks: self.playbacks.clone(),
            events: self.events.clone(),
        };
        let weak = pipeline.downgrade();
        let prerolls = AtomicU8::new(0);
        // A sync handler instead of a bus watch: it needs no main loop on the
        // caller's side and no guard object to keep alive. It runs on a
        // streaming thread, which cannot seek or change the state of its own
        // pipeline, so every action is queued.
        bus.set_sync_handler(move |_, message| {
            let queue = |action: fn(&gst::Pipeline, &Control)| {
                let Some(pipeline) = weak.upgrade() else {
                    return;
                };
                let control = control.clone();
                pipeline.call_async(move |pipeline| {
                    let retired = control.retired.lock().unwrap();
                    if !*retired {
                        action(pipeline, &control);
                    }
                });
            };
            match message.view() {
                gst::MessageView::Eos(_) => finisher.finish(None),
                gst::MessageView::Error(error) => finisher.finish(Some(error.error().to_string())),
                // Every playback runs in segment mode, so that the loop can
                // be switched while it plays: the first preroll is followed
                // by a segment seek, and the preroll after that by the start.
                gst::MessageView::AsyncDone(_) => match prerolls.load(Ordering::Relaxed) {
                    0 => {
                        prerolls.store(1, Ordering::Relaxed);
                        queue(|pipeline, _| {
                            let _ = seek_to_start(pipeline, gst::SeekFlags::FLUSH);
                        });
                    }
                    1 => {
                        prerolls.store(2, Ordering::Relaxed);
                        queue(|pipeline, control| {
                            control.started.store(true, Ordering::Relaxed);
                            let _ = pipeline.set_state(gst::State::Playing);
                        });
                    }
                    _ => {}
                },
                // The end of a pass: the next one follows without a gap, or
                // the playback ends through an EOS pushed after the last data.
                gst::MessageView::SegmentDone(_) => queue(|pipeline, control| {
                    if control.looping.load(Ordering::Relaxed) {
                        let _ = seek_to_start(pipeline, gst::SeekFlags::empty());
                    } else if let Some(pad) = pipeline
                        .by_name("tee")
                        .and_then(|tee| tee.static_pad("sink"))
                    {
                        pad.send_event(gst::event::Eos::new());
                    }
                }),
                _ => {}
            }
            gst::BusSyncReply::Drop
        });

        self.playbacks.lock().unwrap().insert(id, playback);
        if pipeline.set_state(gst::State::Paused).is_err() {
            // The bus error may have removed the pipeline already and reported it.
            if let Some(playback) = self.playbacks.lock().unwrap().remove(&id) {
                playback.control.retire(&pipeline);
                return Err(playback_error("the pipeline refused to start".into()));
            }
        }
        Ok(id)
    }

    // Removing the playback from the map first is what keeps a stopped
    // playback from reporting an event afterwards.
    pub(super) fn stop(&self, id: PlaybackId) {
        let playback = self.playbacks.lock().unwrap().remove(&id);
        if let Some(playback) = playback {
            self.release(playback);
        }
    }

    pub(super) fn stop_all(&self) {
        let playbacks: Vec<_> = self.playbacks.lock().unwrap().drain().collect();
        for (_, playback) in playbacks {
            self.release(playback);
        }
    }

    /// Stops a playback that was already removed from the map, at once or
    /// through the fade thread.
    fn release(&self, playback: Playback) {
        let (duration, call_volume, monitor_volume) = {
            let mix = self.mix.lock().unwrap();
            (mix.fade_out, mix.call_volume, mix.monitor_volume)
        };
        if duration.is_zero() {
            playback.control.retire(&playback.pipeline);
            return;
        }
        let fade = Fade {
            playback,
            call_volume,
            monitor_volume,
            started: Instant::now(),
            duration,
        };
        let fades = self.fades.lock().unwrap();
        if let Some(Err(mpsc::SendError(fade))) = fades.as_ref().map(|fades| fades.send(fade)) {
            fade.playback.control.retire(&fade.playback.pipeline);
        }
    }

    /// Stops everything at once, fading playbacks included.
    pub(super) fn shutdown(&self) {
        let playbacks: Vec<_> = self.playbacks.lock().unwrap().drain().collect();
        for (_, playback) in playbacks {
            playback.control.retire(&playback.pipeline);
        }
        // The fade thread stops its pipelines when the channel closes.
        self.fades.lock().unwrap().take();
        if let Some(thread) = self.fade_thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }

    pub(super) fn set_fade_out(&self, duration: Duration) {
        self.mix.lock().unwrap().fade_out = duration;
    }

    pub(super) fn set_send_to_call(&self, enabled: bool) {
        self.mix.lock().unwrap().send_to_call = enabled;
        for playback in self.playbacks.lock().unwrap().values() {
            if let Some(element) = playback.pipeline.by_name("call-volume") {
                element.set_property("mute", !enabled);
            }
        }
    }

    pub(super) fn set_limit_call(&self, enabled: bool) {
        self.mix.lock().unwrap().limit_call = enabled;
        for playback in self.playbacks.lock().unwrap().values() {
            playback.limit_call.store(enabled, Ordering::Relaxed);
        }
    }

    pub(super) fn set_call_volume(&self, gain: f64) {
        self.mix.lock().unwrap().call_volume = gain.clamp(0.0, 1.0);
        self.apply_gains();
    }

    pub(super) fn set_monitor_volume(&self, gain: f64) {
        self.mix.lock().unwrap().monitor_volume = gain.clamp(0.0, 1.0);
        self.apply_gains();
    }

    pub(super) fn set_playback_volume(&self, id: PlaybackId, gain: f64) {
        let mix = self.mix.lock().unwrap();
        if let Some(playback) = self.playbacks.lock().unwrap().get_mut(&id) {
            playback.volume = gain.clamp(0.0, 1.0);
            playback.apply_gains(&mix, 1.0);
        }
    }

    fn apply_gains(&self) {
        let mix = self.mix.lock().unwrap();
        for playback in self.playbacks.lock().unwrap().values() {
            playback.apply_gains(&mix, 1.0);
        }
    }

    /// Takes effect at the end of the pass that is being read.
    pub(super) fn set_playback_loop(&self, id: PlaybackId, looping: bool) {
        if let Some(playback) = self.playbacks.lock().unwrap().get(&id) {
            playback.control.looping.store(looping, Ordering::Relaxed);
        }
    }

    pub(super) fn restart(&self, id: PlaybackId) {
        let pipeline = match self.playbacks.lock().unwrap().get(&id) {
            // Before the start there is nothing to restart, and a seek would
            // confuse the count of prerolls.
            Some(playback) if playback.control.started.load(Ordering::Relaxed) => {
                playback.pipeline.clone()
            }
            _ => return,
        };
        let _ = seek_to_start(&pipeline, gst::SeekFlags::FLUSH);
    }

    pub(super) fn position(&self, id: PlaybackId) -> Option<Position> {
        let pipeline = match self.playbacks.lock().unwrap().get(&id) {
            Some(playback) if playback.control.started.load(Ordering::Relaxed) => {
                playback.pipeline.clone()
            }
            _ => return None,
        };
        let elapsed = pipeline.query_position::<gst::ClockTime>()?;
        let duration = pipeline.query_duration::<gst::ClockTime>();
        Some(Position {
            elapsed: elapsed.into(),
            duration: duration.map(Into::into),
        })
    }

    /// Only affects the next playbacks; the graph thread moves the running ones.
    pub(super) fn set_monitor(&self, name: Option<String>) {
        self.mix.lock().unwrap().monitor = name;
    }
}

/// Runs a [`Limiter`] on the F32LE buffers that leave `pad` while `enabled`
/// is set. The probe runs on the streaming thread of the branch.
fn limit(pad: &gst::Pad, enabled: Arc<AtomicBool>) {
    let limiter: Mutex<Option<Limiter>> = Mutex::new(None);
    pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
        let mut limiter = limiter.lock().unwrap();
        let limiter = limiter.get_or_insert_with(|| {
            let rate = pad
                .current_caps()
                .and_then(|caps| caps.structure(0)?.get::<i32>("rate").ok())
                .and_then(|rate| u32::try_from(rate).ok())
                .unwrap_or(DEFAULT_RATE);
            Limiter::new(CALL_CEILING, CALL_RELEASE, rate)
        });
        if !enabled.load(Ordering::Relaxed) {
            limiter.reset();
            return gst::PadProbeReturn::Ok;
        }
        let Some(buffer) = info.buffer_mut() else {
            return gst::PadProbeReturn::Ok;
        };
        let Ok(mut map) = buffer.make_mut().map_writable() else {
            return gst::PadProbeReturn::Ok;
        };
        let bytes = map.as_mut_slice();
        if bytes.len() % 4 != 0 {
            return gst::PadProbeReturn::Ok;
        }
        let mut samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        limiter.process(&mut samples);
        for (chunk, sample) in bytes.chunks_exact_mut(4).zip(samples) {
            chunk.copy_from_slice(&sample.to_le_bytes());
        }
        gst::PadProbeReturn::Ok
    });
}

fn seek_to_start(pipeline: &gst::Pipeline, flags: gst::SeekFlags) -> Result<(), glib::BoolError> {
    pipeline.seek(
        1.0,
        flags | gst::SeekFlags::SEGMENT,
        gst::SeekType::Set,
        gst::ClockTime::ZERO,
        gst::SeekType::None,
        gst::ClockTime::NONE,
    )
}

struct Finisher {
    id: PlaybackId,
    path: PathBuf,
    playbacks: Playbacks,
    events: async_channel::Sender<Event>,
}

impl Finisher {
    fn finish(&self, error: Option<String>) {
        let Some(playback) = self.playbacks.lock().unwrap().remove(&self.id) else {
            return;
        };
        // This runs on a streaming thread, which cannot stop its own pipeline.
        let control = playback.control;
        playback
            .pipeline
            .call_async(move |pipeline| control.retire(pipeline));
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

#[cfg(test)]
mod tests;
