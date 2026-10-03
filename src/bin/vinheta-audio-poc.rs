/* vinheta-audio-poc.rs
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

//! Diagnostic tool that exercises the audio engine without the interface.
//! See docs/audio-poc.md.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gst::glib;
use vinheta::audio::{self, AudioEngine, Config, Event, PlaybackId};

const USAGE: &str = "usage: vinheta-audio-poc [--help] [--version] [--once] [--mic NODE_NAME] \
[--monitor NODE_NAME] [--call-volume N] [--monitor-volume N] [--no-call] \
[--stop-after SECONDS] [--stop-all-after SECONDS] [--mute-call-after SECONDS] \
[--unmute-call-after SECONDS] [--call-volume-after SECONDS GAIN] \
[--monitor-volume-after SECONDS GAIN] [--no-voice] [--voice-off-after SECONDS] \
[--voice-on-after SECONDS] [--mic-after SECONDS NODE_NAME] \
[--monitor-after SECONDS NODE_NAME] [--replay-after SECONDS] [FILE...]

Without FILE, only the virtual microphone and the microphone link are created.
With FILEs, they are played together once, and again each time Enter is pressed.
--once plays the FILEs a single time and exits when they end.
--no-call starts with the call branch muted.
The --*-after options act that many seconds after the first playback starts:
--stop-after stops the first FILE, --stop-all-after stops every FILE, and
--mute-call-after and --unmute-call-after turn the call branch off and on,
--call-volume-after and --monitor-volume-after set the gain of a branch (0 to 1),
--voice-off-after and --voice-on-after remove and restore the microphone link,
--mic-after and --monitor-after switch the microphone and the monitor output
(the name \"default\" follows the system default), and --replay-after plays
the FILEs again.
--no-voice starts without the microphone link.
The device lists are printed after the start and whenever they change.";

#[derive(Default)]
struct Args {
    config: Config,
    files: Vec<PathBuf>,
    once: bool,
    version: bool,
    stop_after: Option<Duration>,
    stop_all_after: Option<Duration>,
    mute_call_after: Option<Duration>,
    unmute_call_after: Option<Duration>,
    call_volume_after: Option<(Duration, f64)>,
    monitor_volume_after: Option<(Duration, f64)>,
    voice_off_after: Option<Duration>,
    voice_on_after: Option<Duration>,
    mic_after: Option<(Duration, Option<String>)>,
    monitor_after: Option<(Duration, Option<String>)>,
    replay_after: Option<Duration>,
}

fn parse_args() -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let volume = |text: String| {
            text.parse::<f64>()
                .map_err(|_| format!("bad volume: {text}"))
        };
        let seconds = |text: String| {
            Duration::try_from_secs_f64(text.parse().unwrap_or(-1.0))
                .map(Some)
                .map_err(|_| format!("bad number of seconds: {text}"))
        };
        let device = |name: String| (name != "default").then_some(name);
        match arg.as_str() {
            "--version" => parsed.version = true,
            "--once" => parsed.once = true,
            "--mic" => parsed.config.mic = Some(value()?),
            "--monitor" => parsed.config.monitor = Some(value()?),
            "--call-volume" => parsed.config.call_volume = volume(value()?)?,
            "--monitor-volume" => parsed.config.monitor_volume = volume(value()?)?,
            "--no-call" => parsed.config.send_to_call = false,
            "--stop-after" => parsed.stop_after = seconds(value()?)?,
            "--stop-all-after" => parsed.stop_all_after = seconds(value()?)?,
            "--mute-call-after" => parsed.mute_call_after = seconds(value()?)?,
            "--unmute-call-after" => parsed.unmute_call_after = seconds(value()?)?,
            "--call-volume-after" => {
                parsed.call_volume_after = seconds(value()?)?.zip(Some(volume(value()?)?));
            }
            "--monitor-volume-after" => {
                parsed.monitor_volume_after = seconds(value()?)?.zip(Some(volume(value()?)?));
            }
            "--no-voice" => parsed.config.include_voice = false,
            "--voice-off-after" => parsed.voice_off_after = seconds(value()?)?,
            "--voice-on-after" => parsed.voice_on_after = seconds(value()?)?,
            "--mic-after" => parsed.mic_after = seconds(value()?)?.zip(Some(device(value()?))),
            "--monitor-after" => {
                parsed.monitor_after = seconds(value()?)?.zip(Some(device(value()?)));
            }
            "--replay-after" => parsed.replay_after = seconds(value()?)?,
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => parsed.files.push(PathBuf::from(&arg)),
        }
    }
    if parsed.once && parsed.files.is_empty() {
        return Err("--once needs a FILE".into());
    }
    Ok(parsed)
}

struct Poc {
    engine: AudioEngine,
    args: Args,
    main_loop: glib::MainLoop,
    playing: RefCell<Vec<PlaybackId>>,
    failed: Cell<bool>,
}

impl Poc {
    fn play_all(&self) {
        for file in &self.args.files {
            match self.engine.play(file) {
                Ok(id) => self.playing.borrow_mut().push(id),
                Err(error) => self.playback_failed(&error),
            }
        }
    }

    fn playback_failed(&self, error: &audio::Error) {
        eprintln!("error: {error}");
        if self.args.once {
            self.failed.set(true);
            self.main_loop.quit();
        }
    }

    fn ended(&self, id: PlaybackId) {
        self.playing.borrow_mut().retain(|playing| *playing != id);
        if self.args.once && self.playing.borrow().is_empty() {
            self.main_loop.quit();
        }
    }

    fn schedule(self: &Rc<Self>, delay: Option<Duration>, action: impl FnOnce(&Self) + 'static) {
        if let Some(delay) = delay {
            let poc = self.clone();
            glib::timeout_add_local_once(delay, move || action(&poc));
        }
    }

    fn start(self: &Rc<Self>) {
        self.play_all();
        let first = self.playing.borrow().first().copied();
        self.schedule(self.args.stop_after, move |poc| {
            if let Some(id) = first {
                poc.engine.stop(id);
                println!("stopped the first file");
                poc.ended(id);
            }
        });
        self.schedule(self.args.stop_all_after, |poc| {
            poc.engine.stop_all();
            println!("stopped all files");
            poc.playing.borrow_mut().clear();
            if poc.args.once {
                poc.main_loop.quit();
            }
        });
        self.schedule(self.args.mute_call_after, |poc| {
            poc.engine.set_send_to_call(false);
            println!("call branch muted");
        });
        self.schedule(self.args.unmute_call_after, |poc| {
            poc.engine.set_send_to_call(true);
            println!("call branch unmuted");
        });
        if let Some((delay, gain)) = self.args.call_volume_after {
            self.schedule(Some(delay), move |poc| {
                poc.engine.set_call_volume(gain);
                println!("call volume set to {gain}");
            });
        }
        if let Some((delay, gain)) = self.args.monitor_volume_after {
            self.schedule(Some(delay), move |poc| {
                poc.engine.set_monitor_volume(gain);
                println!("monitor volume set to {gain}");
            });
        }
        self.schedule(self.args.voice_off_after, |poc| {
            poc.engine.set_include_voice(false);
            println!("voice turned off");
        });
        self.schedule(self.args.voice_on_after, |poc| {
            poc.engine.set_include_voice(true);
            println!("voice turned on");
        });
        if let Some((delay, name)) = self.args.mic_after.clone() {
            self.schedule(Some(delay), move |poc| {
                println!("microphone set to {name:?}");
                poc.engine.set_microphone(name);
            });
        }
        if let Some((delay, name)) = self.args.monitor_after.clone() {
            self.schedule(Some(delay), move |poc| {
                println!("monitor output set to {name:?}");
                poc.engine.set_monitor(name);
            });
        }
        self.schedule(self.args.replay_after, |poc| {
            println!("playing the files again");
            poc.play_all();
        });
    }
}

fn main() -> ExitCode {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let mut args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    if args.version {
        return match audio::versions() {
            Ok((pipewire, gstreamer)) => {
                println!("PipeWire {pipewire}\n{gstreamer}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }

    let (engine, events) = match AudioEngine::start(std::mem::take(&mut args.config)) {
        Ok(started) => started,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    // The same setup the GTK app has: a GLib main loop on the main thread,
    // with engine events consumed by a local future.
    let main_loop = glib::MainLoop::new(None, false);

    for signal in [2, 15] {
        let main_loop = main_loop.clone();
        glib::unix_signal_add_local(signal, move || {
            main_loop.quit();
            glib::ControlFlow::Break
        });
    }

    let poc = Rc::new(Poc {
        engine,
        args,
        main_loop: main_loop.clone(),
        playing: RefCell::default(),
        failed: Cell::new(false),
    });

    // Enter replays the files. Lines are read on a thread and forwarded here.
    if !poc.args.files.is_empty() && !poc.args.once {
        let (replays, replay_receiver) = async_channel::unbounded();
        std::thread::spawn(move || {
            for _ in std::io::stdin().lines().map_while(Result::ok) {
                if replays.send_blocking(()).is_err() {
                    break;
                }
            }
        });
        let poc = poc.clone();
        glib::spawn_future_local(async move {
            while replay_receiver.recv().await.is_ok() {
                poc.play_all();
            }
        });
    }

    glib::spawn_future_local({
        let poc = poc.clone();
        async move {
            while let Ok(event) = events.recv().await {
                match event {
                    Event::NodeCreated(id) => {
                        println!("virtual microphone created, node id {id}");
                        poc.start();
                    }
                    Event::MicLinked {
                        name,
                        fallback: false,
                    } => {
                        println!("microphone linked: {name}");
                    }
                    Event::MicLinked {
                        name,
                        fallback: true,
                    } => {
                        println!("microphone linked as a fallback: {name}");
                    }
                    Event::MicUnlinked => println!("microphone unlinked"),
                    Event::DevicesChanged {
                        microphones,
                        outputs,
                    } => {
                        println!("devices changed");
                        for device in microphones {
                            println!("microphone: {} ({})", device.name, device.description);
                        }
                        for device in outputs {
                            println!("output: {} ({})", device.name, device.description);
                        }
                    }
                    Event::PlaybackFinished { id, path } => {
                        println!("finished playing {}", path.display());
                        poc.ended(id);
                    }
                    Event::Error(error @ audio::Error::Playback { .. }) => {
                        poc.playback_failed(&error);
                    }
                    Event::Error(error) => eprintln!("error: {error}"),
                }
            }
        }
    });

    main_loop.run();
    if poc.failed.get() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
