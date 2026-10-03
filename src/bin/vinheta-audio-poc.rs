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

use std::path::PathBuf;
use std::process::ExitCode;

use gst::glib;
use vinheta::audio::{self, AudioEngine, Config, Event};

const USAGE: &str = "usage: vinheta-audio-poc [--help] [--version] [--once] [--mic NODE_NAME] \
[--monitor NODE_NAME] [--call-volume N] [--monitor-volume N] [FILE]

Without FILE, only the virtual microphone and the microphone link are created.
With FILE, it is played once, and again each time Enter is pressed.
--once plays FILE a single time and exits when it ends.";

struct Args {
    config: Config,
    file: Option<PathBuf>,
    once: bool,
    version: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut parsed = Args {
        config: Config::default(),
        file: None,
        once: false,
        version: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let volume = |text: String| {
            text.parse::<f64>()
                .map_err(|_| format!("bad volume: {text}"))
        };
        match arg.as_str() {
            "--version" => parsed.version = true,
            "--once" => parsed.once = true,
            "--mic" => parsed.config.mic = Some(value()?),
            "--monitor" => parsed.config.monitor = Some(value()?),
            "--call-volume" => parsed.config.call_volume = volume(value()?)?,
            "--monitor-volume" => parsed.config.monitor_volume = volume(value()?)?,
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => parsed.file = Some(PathBuf::from(&arg)),
        }
    }
    if parsed.once && parsed.file.is_none() {
        return Err("--once needs a FILE".into());
    }
    Ok(parsed)
}

fn main() -> ExitCode {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let args = match parse_args() {
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

    let (engine, events) = match AudioEngine::start(args.config) {
        Ok(started) => started,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    // The same setup the GTK app will have: a GLib main loop on the main
    // thread, with engine events consumed by a local future.
    let main_loop = glib::MainLoop::new(None, false);
    let failed = std::rc::Rc::new(std::cell::Cell::new(false));

    for signal in [2, 15] {
        let main_loop = main_loop.clone();
        glib::unix_signal_add_local(signal, move || {
            main_loop.quit();
            glib::ControlFlow::Break
        });
    }

    // Enter replays the file. Lines are read on a thread and forwarded here.
    let (replays, replay_receiver) = async_channel::unbounded();
    if args.file.is_some() && !args.once {
        std::thread::spawn(move || {
            for _ in std::io::stdin().lines().map_while(Result::ok) {
                if replays.send_blocking(()).is_err() {
                    break;
                }
            }
        });
    }

    let engine = std::rc::Rc::new(engine);
    if let Some(file) = args.file.clone() {
        let engine = engine.clone();
        glib::spawn_future_local(async move {
            while replay_receiver.recv().await.is_ok() {
                engine.play(&file);
            }
        });
    }

    glib::spawn_future_local({
        let engine = engine.clone();
        let main_loop = main_loop.clone();
        let failed = failed.clone();
        async move {
            while let Ok(event) = events.recv().await {
                match event {
                    Event::NodeCreated(id) => {
                        println!("virtual microphone created, node id {id}");
                        if let Some(file) = &args.file {
                            engine.play(file);
                        }
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
                        println!("the default source is Vinheta itself, linked {name} instead");
                    }
                    Event::PlaybackFinished(path) => {
                        println!("finished playing {}", path.display());
                        if args.once {
                            main_loop.quit();
                        }
                    }
                    Event::Error(error) => {
                        eprintln!("error: {error}");
                        if args.once
                            && matches!(
                                error,
                                audio::Error::Playback { .. } | audio::Error::FileNotFound(_)
                            )
                        {
                            failed.set(true);
                            main_loop.quit();
                        }
                    }
                }
            }
        }
    });

    main_loop.run();
    if failed.get() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
