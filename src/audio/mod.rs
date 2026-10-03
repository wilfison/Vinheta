/* mod.rs
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mic: None,
            monitor: None,
            call_volume: 1.0,
            monitor_volume: 1.0,
            send_to_call: true,
        }
    }
}

/// Identifies one playback started by [`AudioEngine::play`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlaybackId(u64);

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The virtual microphone exists; the value is its PipeWire node id.
    NodeCreated(u32),
    /// `fallback` is set when the default source was the virtual microphone
    /// itself and a physical source was linked instead.
    MicLinked {
        name: String,
        fallback: bool,
    },
    PlaybackFinished {
        id: PlaybackId,
        path: PathBuf,
    },
    Error(Error),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
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

        let (commands, command_receiver) = pw::channel::channel();
        let (started, started_receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("vinheta-pipewire".into())
            .spawn(move || graph::run(config.mic, events, command_receiver, started))
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
    pub fn play(&self, path: impl AsRef<Path>) -> Result<PlaybackId, Error> {
        self.player.play(path.as_ref())
    }

    /// Stops one playback. It reports no event afterwards.
    pub fn stop(&self, id: PlaybackId) {
        self.player.stop(id);
    }

    pub fn stop_all(&self) {
        self.player.stop_all();
    }

    /// Mutes or unmutes the call branch of current and future playbacks.
    pub fn set_send_to_call(&self, enabled: bool) {
        self.player.set_send_to_call(enabled);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.player.stop_all();
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
