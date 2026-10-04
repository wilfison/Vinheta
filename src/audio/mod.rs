/* mod.rs
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

//! Audio engine: the "Vinheta" virtual microphone, the microphone link, and
//! playback to the call and to the local monitor.
//!
//! PipeWire runs on its own thread; the caller only sends commands and reads
//! [`Event`]s from the receiver returned by [`AudioEngine::start`].

mod graph;
mod player;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use pipewire as pw;

use self::graph::Command;
use self::player::Player;

#[derive(Debug, Clone)]
pub struct Config {
    /// Node name of the microphone. `None` follows the system default source.
    pub mic: Option<String>,
    /// Node name of the monitor sink. `None` uses the system default sink.
    pub monitor: Option<String>,
    pub call_volume: f64,
    pub monitor_volume: f64,
    /// When false, the call branch of every sound is muted.
    pub send_to_call: bool,
    /// When false, the microphone is not linked to the virtual microphone.
    pub include_voice: bool,
    /// How long a stopped playback takes to fade out. Zero stops at once.
    pub fade_out: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mic: None,
            monitor: None,
            call_volume: 1.0,
            monitor_volume: 1.0,
            send_to_call: true,
            include_voice: true,
            fade_out: Duration::ZERO,
        }
    }
}

/// How one playback starts. Both can be changed while it plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayOptions {
    /// The gain of this playback (0.0 to 1.0), on both branches.
    pub volume: f64,
    pub looping: bool,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            volume: 1.0,
            looping: false,
        }
    }
}

/// How far along a playback is. In a loop, `elapsed` starts again on each
/// pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub elapsed: Duration,
    pub duration: Option<Duration>,
}

/// Identifies one playback started by [`AudioEngine::play`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlaybackId(u64);

/// A microphone or an output. `name` is the PipeWire node name, which is what
/// [`AudioEngine::set_microphone`] and [`AudioEngine::set_monitor`] take.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The virtual microphone exists; the value is its PipeWire node id.
    NodeCreated(u32),
    /// `fallback` is set when the linked microphone is not the one asked
    /// for: the chosen one does not exist, or the default source is the
    /// virtual microphone itself.
    MicLinked {
        name: String,
        fallback: bool,
    },
    /// The voice was turned off, so no microphone is linked.
    MicUnlinked,
    /// Sent once after the start and whenever either list changes.
    DevicesChanged {
        microphones: Vec<Device>,
        outputs: Vec<Device>,
    },
    PlaybackFinished {
        id: PlaybackId,
        path: PathBuf,
    },
    Error(Error),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// PipeWire could not be reached when the engine started.
    Unreachable(String),
    /// The connection was lost while the engine ran. The engine is of no
    /// use afterwards: drop it and start another one.
    ConnectionLost(String),
    PipeWire(String),
    GStreamer(String),
    NodeExists,
    MicNotFound(String),
    NoMicrophone,
    FileNotFound(PathBuf),
    Playback {
        id: PlaybackId,
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(message) => write!(f, "PipeWire cannot be reached: {message}"),
            Self::ConnectionLost(message) => {
                write!(f, "the connection to PipeWire was lost: {message}")
            }
            Self::PipeWire(message) => write!(f, "PipeWire error: {message}"),
            Self::GStreamer(message) => write!(f, "GStreamer error: {message}"),
            Self::NodeExists => write!(f, "a node named \"{}\" already exists", graph::NODE_NAME),
            Self::MicNotFound(name) => write!(f, "microphone \"{name}\" not found"),
            Self::NoMicrophone => write!(f, "no microphone available to link"),
            Self::FileNotFound(path) => write!(f, "file not found: {}", path.display()),
            Self::Playback { path, message, .. } => {
                write!(f, "could not play {}: {message}", path.display())
            }
        }
    }
}

impl Error {
    /// A stable name of the variant, for logs and scripts.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable(_) => "unreachable",
            Self::ConnectionLost(_) => "connection-lost",
            Self::PipeWire(_) => "pipewire",
            Self::GStreamer(_) => "gstreamer",
            Self::NodeExists => "node-exists",
            Self::MicNotFound(_) => "mic-not-found",
            Self::NoMicrophone => "no-microphone",
            Self::FileNotFound(_) => "file-not-found",
            Self::Playback { .. } => "playback",
        }
    }
}

impl std::error::Error for Error {}

pub struct AudioEngine {
    commands: pw::channel::Sender<Command>,
    thread: Option<JoinHandle<()>>,
    player: Player,
}

impl AudioEngine {
    /// Creates the virtual microphone and returns once it has been requested,
    /// or fails if PipeWire is unreachable or the node already exists.
    pub fn start(config: Config) -> Result<(Self, async_channel::Receiver<Event>), Error> {
        let (events, receiver) = async_channel::unbounded();
        let player = Player::new(&config, events.clone())?;

        let options = graph::Options {
            mic: config.mic,
            include_voice: config.include_voice,
        };
        let (commands, command_receiver) = pw::channel::channel();
        let (started, started_receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("vinheta-pipewire".into())
            .spawn(move || graph::run(options, events, command_receiver, started))
            .map_err(|error| Error::PipeWire(error.to_string()))?;

        started_receiver
            .recv()
            .unwrap_or_else(|_| Err(Error::PipeWire("the PipeWire thread died".into())))?;

        let engine = Self {
            commands,
            thread: Some(thread),
            player,
        };
        Ok((engine, receiver))
    }

    /// Plays a file on both branches. Failures after the start arrive as
    /// [`Event::Error`].
    pub fn play(&self, path: impl AsRef<Path>, options: PlayOptions) -> Result<PlaybackId, Error> {
        self.player.play(path.as_ref(), options)
    }

    /// Stops one playback, fading it out when a fade is set. Either way it
    /// is forgotten at once and reports no event afterwards.
    pub fn stop(&self, id: PlaybackId) {
        self.player.stop(id);
    }

    pub fn stop_all(&self) {
        self.player.stop_all();
    }

    /// Sets how long the playbacks stopped from now on take to fade out.
    pub fn set_fade_out(&self, duration: Duration) {
        self.player.set_fade_out(duration);
    }

    /// Sets the gain of one playback (0.0 to 1.0), which multiplies the gain
    /// of each branch.
    pub fn set_playback_volume(&self, id: PlaybackId, gain: f64) {
        self.player.set_playback_volume(id, gain);
    }

    /// Turned off, the playback ends with the pass that is being heard,
    /// unless that pass is in its last 350 ms or so: then one more plays.
    pub fn set_playback_loop(&self, id: PlaybackId, looping: bool) {
        self.player.set_playback_loop(id, looping);
    }

    /// Starts a playback again from the beginning. It keeps its id and
    /// reports nothing.
    pub fn restart(&self, id: PlaybackId) {
        self.player.restart(id);
    }

    /// `None` for a playback that is over or has not started yet.
    pub fn position(&self, id: PlaybackId) -> Option<Position> {
        self.player.position(id)
    }

    /// Mutes or unmutes the call branch of current and future playbacks.
    pub fn set_send_to_call(&self, enabled: bool) {
        self.player.set_send_to_call(enabled);
    }

    /// Sets the gain of the call branch (0.0 to 1.0) of current and future
    /// playbacks. It does not undo the mute of [`Self::set_send_to_call`].
    pub fn set_call_volume(&self, gain: f64) {
        self.player.set_call_volume(gain);
    }

    /// Sets the gain of the monitor branch (0.0 to 1.0) of current and future
    /// playbacks.
    pub fn set_monitor_volume(&self, gain: f64) {
        self.player.set_monitor_volume(gain);
    }

    /// Links another microphone, by node name. `None` follows the system
    /// default source, which is also used while the chosen one does not exist.
    pub fn set_microphone(&self, name: Option<String>) {
        let _ = self.commands.send(Command::SetMic(name));
    }

    /// Removes or restores the link from the microphone to the virtual one.
    pub fn set_include_voice(&self, enabled: bool) {
        let _ = self.commands.send(Command::SetIncludeVoice(enabled));
    }

    /// Moves the monitor branch of current and future playbacks to another
    /// output, by node name. `None` is the system default sink, which is also
    /// used while the chosen one does not exist.
    pub fn set_monitor(&self, name: Option<String>) {
        self.player.set_monitor(name.clone());
        let _ = self.commands.send(Command::SetMonitor(name));
    }
}

/// The gain of a volume slider at `position` (0.0 to 1.0). The curve is
/// cubic, so the slider feels even to the ear.
pub fn slider_gain(position: f64) -> f64 {
    position.clamp(0.0, 1.0).powi(3)
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.player.shutdown();
        let _ = self.commands.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// PipeWire and GStreamer library versions, for diagnostics.
pub fn versions() -> Result<(String, String), Error> {
    pw::init();
    gst::init().map_err(|error| Error::GStreamer(error.to_string()))?;
    let pipewire = unsafe { std::ffi::CStr::from_ptr(pw::sys::pw_get_library_version()) };
    Ok((
        pipewire.to_string_lossy().into_owned(),
        gst::version_string().to_string(),
    ))
}

#[cfg(test)]
mod tests;
