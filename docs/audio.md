# The audio engine

This document describes the audio engine of Vinheta (`src/audio/`): each sound plays on the headphones (the monitor branch) and into the apps that are recording a microphone (the call branch), so a call hears it next to the voice of the user. It also records what was measured with command line tools before the engine was written; those command lines are kept as the record of the measurements.

Until version 1.0.0 the engine created a virtual microphone named "Vinheta", mixed the real microphone into it, and the user had to choose it in the call app. That is gone: the user keeps the usual microphone in the call app, and nothing is added to the device lists of the system.

## Source layout

- `src/audio/`: the audio engine. `mod.rs` is the public API (`AudioEngine`, `Config`, `PlayOptions`, `Position`, `Event`, `Error`, `PlaybackId`, `Device`, `slider_gain`); `graph.rs` owns the PipeWire thread (drain node, registry, recording streams, links); `player.rs` builds one GStreamer pipeline per sound. PipeWire objects never leave the engine thread: commands go in through a `pipewire::channel`, events come out through `async-channel`.
- `src/bin/vinheta-audio-test.rs`: diagnostic binary behind the `audio-test` cargo feature (`cargo run --features audio-test --bin vinheta-audio-test -- --help`). Meson does not build or install it.

## Rules that are easy to get wrong

- The call branch sink uses `node.autoconnect=false`, so WirePlumber leaves it alone, and `graph.rs` links every stream of its own process, whose node name starts with `vinheta-call-PID-`. Another engine may be running (a second instance, a check): linking by the bare prefix made one engine feed the sounds of another into its own targets.
- A sound goes into a recording stream (`media.class` exactly `Stream/Input/Audio`) that was seen fed by a microphone (`Audio/Source` or `Audio/Source/Virtual`) and is not a level meter (`stream.monitor=true`). Once seen, the stream stays a target until it is removed: WirePlumber unlinks it for a moment when it moves it.
- The registry does not carry `stream.monitor` or `application.process.binary`. The engine binds each recording stream and reads them from its `info` event, and only when the change mask has `PROPS`: an update of anything else comes with an empty dictionary, which once erased the app of a stream.
- Every call stream is also linked to the drain node (`vinheta-drain-PID`, a `support.null-audio-sink` of class `Audio/Sink/Internal`). Without it a playback would not run while no app records: an unlinked sink consumes nothing and blocks the whole pipeline, the monitor branch included. Without a media class the node would be an `Audio/Sink` and show up as an output.
- The call branch is mono, and its one port is linked to every input port of a target. The ports of a recording stream follow the microphone it records (one for a mono microphone, two for a stereo one) and are replaced when it is moved to another; the links are made again when the new ports show up.
- The node and links are created without `object.linger`, so they vanish when the process ends. Do not add it.
- A playback that was stopped on request never reports an event afterwards; the interface relies on that, and on events carrying the `PlaybackId`.
- "Send sounds to call" mutes the `call-volume` element of each pipeline. The call stream and its links stay in place. The call volume is the `volume` property of the same element, independent of the mute.
- The gain of a branch element is a product: branch × playback × fade (`effective_gain`). Never set a gain before the `tee`: it is heard a queue late. A branch volume change keeps each playback's own gain.
- The queues of a pipeline hold 200 ms, so the end of a pass is known about 350 ms before it is heard.
- Every playback runs in segment mode, looping or not: preroll, a flushing `SEGMENT` seek, then `PLAYING`; on each `SEGMENT_DONE` either a non-flushing seek (loop) or an EOS pushed into the sink pad of the `tee` (the only way it ends). These actions are queued with `call_async` from the bus handler, and skipped once the playback is retired.
- `restart` is a flushing seek: the `PlaybackId` and the streams stay, and nothing is reported.
- With a fade set, a stopped playback is forgotten at once (no events, unknown to every call that takes its id, untouched by volume changes) while a thread of the engine ramps it down in 10 ms steps and stops it 150 ms after the ramp. Dropping the engine stops fading playbacks at once.
- The volumes and the mute are set by `player.rs` on the caller's thread; the target app and the monitor output are commands for the PipeWire thread.
- The settings store slider positions (0 to 1); the engine takes gains. `slider_gain` (cubic) converts.
- The monitor branch is routed by WirePlumber. A new playback gets the chosen output as `target-object`; a running one is moved by writing `target.object` for its stream (node name `vinheta-monitor-*`) in the default metadata. The system default is asked for with the value `-1`: removing the key would leave the target the stream was created with.
- `Error` tells the fatal failures apart: `Unreachable` (PipeWire cannot be reached at the start) and `ConnectionLost` (while running: the engine is of no use afterwards). `Error::kind` is a stable name for logs and scripts. An engine can be started again in the same process after the previous one was dropped, also after a lost connection.
- A chosen output that does not exist is not an error to recover from in the interface: WirePlumber uses the default sink and moves the stream when the device shows up. A chosen app that is not recording is not an error either: the sounds reach no app until it records.
- The target is the name of the binary of an app (`application.process.binary`, else `application.name`, else the node name), so every recording stream of a browser is one target. `None` is every app.
- `Event::DevicesChanged` (the outputs) and `Event::TargetsChanged` (the apps that are recording) are only sent when the list really changed. Neither lists the drain node, a level meter, or a recorder of an output.
- After changing audio code, run `scripts/verify-audio.sh`. Its sections on a second engine, a taken node name, and a lost connection (on a private PipeWire instance it starts and destroys) need no sound. While working on one behavior, `--only REGEX` runs only the sections whose title matches (`--list` prints the titles); run all of it before committing. It uses fake devices and fake call apps only (no real microphone, headphones, or call app) and needs `ffmpeg` and `python3`. Every playback of the harness has a fake app as its target: with every app as the target, the tones would reach the real apps that are recording. In a new check, place the measurements with `after FILE SECONDS [MARGIN]` (an action scheduled `SECONDS` into the playback) instead of hand-computed offsets. A sound with no silence at its start needs `--start-after 1`, so the recorders are ready, and `window` instead of `after`. `window` fails the run when it finds no onset: the tone must reach -40 dBFS in the recording, which a gain of 0.1 does not. A real device unplugged during the run is reported as a `NOTE`, not as a failure.
- The call branch is muted, not dropped, so turning "Send sounds to call" back on is instant and applies to sounds already playing. A target change applies to them too: the links are moved.
- "Fade Out on Stop" is one global setting (300 ms, on by default). There is no fade in and no fade per pad, and a restart does not fade.
- A volume change is heard within 200 ms and does not touch the other branch (measured by the harness).
- The engine does not look at the default source: an app is a target whatever microphone it records.

## Environment and versions

| Component | Version |
| --- | --- |
| Ubuntu | 26.04.1 |
| PipeWire | 1.6.2 |
| WirePlumber | 0.5.13 |
| GStreamer | 1.28.2 |
| `pipewire` crate | 0.10 |
| `gstreamer` crate | 0.23 (built on `glib 0.20`, same as `gtk4 0.9`) |

The default source on the development machine is mono (`...mono-fallback`, one port named `capture_MONO`). The internal sound card provides a stereo source (`capture_FL`, `capture_FR`).

## Why not the microphone itself

The first wish was to put the sounds into the default microphone, so that nothing had to be chosen in the call app. A physical source only has output ports (`capture_*`): there is nothing to link a sound to. Two ways give the same result for the user:

- Make a virtual source the default (`pw-metadata 0 default.audio.source '{"name":"..."}'` works, although `wpctl set-default` refuses a node that is not a device). It changes a setting of the user's system, has to be undone on exit and after a crash, and shows a new microphone in the system settings. Not used.
- Link the sound into the recording stream of each app. This is what the engine does.

Measured on 2026-10-05 with a `pw-record` of the default microphone and a `pw-play` with `node.autoconnect=false` linked by hand to its input ports: the recording had the microphone at about -37 dBFS before and after, and the sound at -12 dBFS (peak -1.7) in between. The default source did not change, and the links went away with the stream that ended.

## Recording streams

```sh
pw-record --target MIC -P '{ node.name=fake-call application.process.binary=fake-call }' call.wav
```

- An app that records is a node of class `Stream/Input/Audio`. Its input ports are named after the microphone it is linked to: `input_MONO` for a mono source, `input_FL` and `input_FR` for a stereo one. PipeWire mixes everything linked to a port.
- The registry gives `node.name`, `media.class`, and `application.name` for such a node. `application.process.binary` and `stream.monitor` only come with the properties of the bound node.
- The "Peak detect" streams of GNOME Settings and pavucontrol record a microphone too, with `stream.monitor=true`. They are not targets, or every volume control would be listed as an app.
- A recorder of the monitor ports of a sink (a desktop recording) is fed by an `Audio/Sink` node, so it is not a target: the sound already reaches it through the monitor branch.
- The streams of the Bluetooth loopback have the class `Stream/Input/Audio/Internal`, which the exact class match leaves out.
- Google Chrome (the Discord and Meet of the browser) records through `pipewire-pulse` as `application.name=Google Chrome input` and `application.process.binary=chrome`, one stream per tab that records. A page recording the "Default" device with echo cancellation, noise suppression, and automatic gain on got the sound of the engine at -8 to -14 dBFS RMS, and its device list had no entry of the engine.
- When WirePlumber moves a recording stream to another microphone (`pw-metadata STREAM_ID target.object MIC`, or a change of the default source), links made by hand stay while the ports stay. A move from a mono microphone to a stereo one replaces the ports, and the links are gone with them. The engine makes them again: the harness measures a silence of 20 ms in the sound.
- A stream that starts recording while a sound plays gets it 40 ms later (measured by the harness).

## The drain node

```sh
pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=drain media.class=Audio/Sink/Internal audio.position=[MONO] object.linger=true }'
```

- Its ports are `playback_MONO` (what the call streams are linked to) and `monitor_MONO`.
- With no `media.class` at all the node gets `Audio/Sink`, and the engine listed it as an output. `Audio/Sink/Internal` is not in `wpctl status`, and `pipewire-pulse` only offers the exact classes `Audio/Sink`, `Audio/Source`, `Audio/Source/Virtual`, and `Audio/Duplex` to its clients.
- A stream linked only to it plays to its end at the normal speed: the node drives itself.
- `pw-cli` exits right away, so on the command line the node needs `object.linger=true` and must be removed with `pw-cli destroy <id>`. The Rust engine does not set it: the node belongs to the engine's connection and dies with it.

## The call stream does not autoconnect

The call branch must not be routed by WirePlumber, which would send it to the default sink. The stream does not autoconnect and is linked by hand (the node `TARGET` below stands for the drain node or a recording stream):

```sh
gst-launch-1.0 uridecodebin uri=file:///path/sound.wav ! audioconvert ! audioresample ! volume \
    ! pipewiresink stream-properties="p,node.autoconnect=false,node.name=vinheta-play" &
pw-link vinheta-play:output_MONO TARGET:input_MONO
```

- Before the links exist, `pw-link -l` shows the stream linked to nothing: WirePlumber leaves it alone.
- The pipeline waits until it is linked, then plays from the start.
- The links disappear with the stream when playback ends. Nothing has to be removed by hand.
- `target-object` is ignored for a target that is not a sink (measured with a virtual source: the stream went to the default sink). It does work for the monitor branch, where the target is a regular `Audio/Sink`.

## Moving the monitor branch

The monitor stream is routed by WirePlumber, which follows the `target.object` key of the default metadata, per stream:

```sh
pw-metadata STREAM_ID target.object SINK_NAME   # moves a playing stream within a second
pw-metadata STREAM_ID target.object -- -1       # back to the default sink
```

- The value is the plain node name (a JSON-quoted name does not work). The sink's `object.serial` with the type `Spa:Id` works too.
- Removing the key (`pw-metadata -d`) only goes back to the default when the stream was created without a target. A stream created with `target-object=X` returns to X. The value `-1` always means the default, so that is what the engine writes.
- A target that does not exist (set at creation or through the metadata) leaves the stream on the default sink; it moves when a sink with that name appears, and moves back when that sink is removed, without stopping the pipeline.
- From Rust: `Metadata::set_property(stream_id, "target.object", None, Some(name))` on the bound default metadata.
- The engine names the monitor stream `vinheta-monitor-PID-ID` to find it in the registry.

## The final pipeline

```
uridecodebin ! audioconvert ! audioresample ! tee name=t
  t. ! queue ! audioconvert ! audio/x-raw,channels=1 ! volume ! pipewiresink   (call: node.autoconnect=false, linked by the engine)
  t. ! queue ! volume ! pipewiresink                                            (monitor: default sink, or target-object=<sink>)
```

- Each branch has its own `queue` and `volume`. Setting one volume to 0 silences only that branch.
- The queues hold 200 ms (`max-size-time`, with the buffer and byte limits off) instead of the default second, and the gain of each `volume` element is a product: branch × playback × fade.
- The call branch is mixed down to mono after its queue. A stereo stream linked to the single port of an app that records a mono microphone would have both channels added up there, up to 6 dB louder.
- Both sinks set `state.restore-props=false`. Without it WirePlumber applies whatever volume it saved for an earlier stream of the same application, which made levels unpredictable during the tests.
- The voice never goes through the engine: the call app records it by itself, so it never reaches the monitor branch.
- One pipeline per sound. Starting a second one while the first is playing works and both are mixed by PipeWire, but overlapping playback has not been verified beyond that.

## Gain per playback, loop, restart, and fade

Measured on 2026-10-03 with throwaway programs of the same pipeline shape, then checked on the real engine by the harness.

- **Where a gain goes.** A `volume` element before the `tee` is heard about 1 second after it is changed, because the queue of each branch is always full (the decoder is faster than the sink). The same change on the elements after the queues is heard within 100 ms. So the gain of a playback and the fade multiply into the two branch elements, and `player.rs` keeps the factors.
- **Gapless loop.** After the preroll the pipeline gets a flushing seek to 0 with the `SEGMENT` flag; at the end of the file it then posts `SEGMENT_DONE` instead of `EOS`, and a non-flushing `SEGMENT` seek to 0 starts the next pass. A 1 second WAV tone loops with no 10 ms window below -50 dBFS at the seams. Ogg Vorbis showed one 20 ms gap at the first seam in about half of the runs, and MP3 a gap of 20 to 30 ms at every seam (decoder padding); both are accepted. A flushing seek on `EOS` instead leaves gaps of 10 to 40 ms and is not used.
- **Every playback is in segment mode**, so the loop can be switched while it plays. The cost at the start is about 5 ms. `play` only sets `PAUSED`; the bus handler issues the seek on the first `ASYNC_DONE` and sets `PLAYING` on the second.
- **Ending.** A pipeline in segment mode never posts `EOS` by itself, and an EOS event sent to the pipeline is not enough. An EOS event pushed into the sink pad of the `tee` on `SEGMENT_DONE` ends it after the last sample.
- **`SEGMENT_DONE` comes early**: when the source has read the file, which is one queue before the end is heard. With 200 ms queues that is 280 to 360 ms, so a loop turned off ends with the pass being heard unless it is in its last 350 ms or so.
- **Seeks from the bus handler** go through `pipeline.call_async`: the sync handler runs on a streaming thread, which must not seek or change the state of its own pipeline. A queued action is skipped once the playback was retired, so it cannot revive a stopped pipeline.
- **Restart** is a flushing seek to 0 on the playing pipeline: a gap of about 50 ms, the same streams and links.
- **Position.** `query_position` and `query_duration` on the pipeline; the position starts again on each pass.
- **Fade out.** A thread of the engine steps the gain of both branch elements every 10 ms. The artifacts of the steps are 58 dB below the tone (64 dB for a sample-accurate ramp), so no controller is used. The sinks still hold 60 to 80 ms when the ramp ends: setting `Null` right away cuts the ramp about 13 dB down, so the pipeline stays silent for 150 ms first. A fading playback is already out of the map of playbacks, which is what keeps it from reporting events.

## Branch offset

Measured by the harness: the call branch as the drain node gets it and one channel of the monitor branch are recorded into the same stereo file and the moment the 1000 Hz tone appears on each side is compared.

Over 8 runs (3 with command line tools, 5 with the Rust engine) the offset "call minus monitor" was either about 0 ms (0.0, 0.0, 0.0, 0.5, 0.5) or about -21 ms (-21.0, -21.0, -21.5). 21.3 ms is one PipeWire quantum (1024 samples at 48 kHz): depending on the order in which the graph processes the nodes, the monitor path picks the sound up one cycle later. There is no target for this value and it is far below what a call would notice.

## Cleanup

The Rust engine creates the drain node and the links on its own connection, without `object.linger`. The harness checks that, within 2 seconds, no `vinheta-drain-*`, `vinheta-call-*`, or `vinheta-monitor-*` node remains after:

- a normal exit (`--once`, end of the file),
- SIGINT (Ctrl+C),
- SIGKILL (`kill -9`).

All three pass, and the app that was recording is still fed by its microphone afterwards. A node made with `pw-cli` and `object.linger` (as the fake devices of the checks are) stays until `pw-cli destroy`.

## Known issues

- **A live GStreamer source stalls the graph.** `audiotestsrc is-live=true ! pipewiresink` with `node.autoconnect=false`, linked by hand into a virtual node, left every node suspended and every later link stuck in the `init` state. File sources (`uridecodebin`, `filesrc`) do not have the problem. The soundboard only plays files, but keep this in mind before feeding a live source into the node.
- **An unlinked call branch blocks the whole pipeline**, including the monitor branch, because the sink does not consume data until it is linked. The engine links the stream to the drain node as soon as its ports show up in the registry, so in practice the delay is not noticeable.
- **The sounds reach every app that records a microphone**, a sound recorder included, unless one app is chosen. The tabs of a browser cannot be told apart.
- **The call can clip.** The sound is added to the voice inside the app's recording stream, at the level of the call volume. A loud sound at 100% reached 0 dBFS in a browser with automatic gain on.
- **Some headsets mute their microphone while they play.** The "AB13X Headset Adapter" (USB `001f:0b21`) delivers exact zeros on its capture, for 1.2 to 1.8 s, while a loud sound plays on its own output, with or without Vinheta (`pw-play` does the same). The sound still reaches the call, since it does not go through the microphone, but the voice is cut meanwhile. Sending the monitor branch to another output avoids it.
- **`pw-link` by port name failed once** with "No such file or directory" right after the node was created, and worked on retry and by port id. It was not reproduced.
- **Noise suppression and echo have not been tested on a real call**; see the manual call checklist below.
- **The server removes the engine's links together with a stream that ends.** When the engine then drops its own proxy for such a link, PipeWire reports "unknown resource" on the core. That error is harmless and must not be treated as a lost connection; only `EPIPE` on the core is. It shows up whenever a sound is stopped while the process keeps running.

## Running it

The engine is exercised without the interface through the test binary (behind the `audio-test` cargo feature, never installed or packaged):

```sh
cargo run --features audio-test --bin vinheta-audio-test -- [OPTIONS] [FILE...]
```

- Without `FILE`: starts the engine, prints the lists, and waits for Ctrl+C.
- With one or more `FILE`s: plays them together once, and again each time Enter is pressed.
- `--once`: plays the files once and exits when they end.
- `--no-call`: starts with the call branch muted (the "Send sounds to call" switch turned off).
- `--stop-after SECONDS`: stops the first file that many seconds after the playback starts.
- `--stop-all-after SECONDS`: stops every file.
- `--mute-call-after SECONDS`, `--unmute-call-after SECONDS`: turn the call branch off and on while the files play.
- `--call-volume-after SECONDS GAIN`, `--monitor-volume-after SECONDS GAIN`: change the gain of a branch (0 to 1) while the files play.
- `--target APP`: the app the sounds are sent to, as the `target:` lines name it (`all`, the default, is every app that is recording a microphone). `--target-after SECONDS APP`: changes it.
- `--monitor-after SECONDS NODE_NAME`: switches the monitor output; the name `default` follows the system default.
- `--replay-after SECONDS`: plays the files again.
- `--volume GAIN`: the gain of every file (0 to 1). `--playback-volume-after SECONDS GAIN`: changes the gain of the first file.
- `--loop`: every file loops. `--loop-off-after SECONDS`: ends the loop of the first file. `--restart-after SECONDS`: starts the first file again.
- `--position-after SECONDS`: prints `position ELAPSED_MS DURATION_MS` for the first file (`unknown` for a missing duration, `position none` when it is over).
- `--fade-out SECONDS`: how long a stopped file takes to fade out (0 by default).
- `--start-after SECONDS`: waits before the first playback, so the recorders of the harness are ready for a sound with no silence at its start.
- `engine ready` is printed once the engine can play. The outputs are printed after the start and whenever they change (`devices changed`, then `output: NAME (DESCRIPTION)` lines), and so are the apps that are recording (`targets changed`, then `target: APP (DESCRIPTION)` lines).
- `--restart-engine-after SECONDS`: drops the engine that many seconds after the playback starts, starts a new one in the same process, and plays the files again. It does so twice, and prints `threads N` (the threads of the process) after each drop.
- `--monitor NODE_NAME`: the monitor output. `--call-volume N`, `--monitor-volume N`: the gain of each branch.
- An error is printed as `error: KIND: TEXT`. `KIND` is the stable name of the variant (`Error::kind`): `unreachable` (PipeWire cannot be reached at the start), `connection-lost`, `pipewire`, `gstreamer`, `file-not-found`, `playback`.
- `--version`: prints the PipeWire and GStreamer library versions.

The automated check:

```sh
scripts/verify-audio.sh                  # builds and checks vinheta-audio-test
scripts/verify-audio.sh --only 'loop'    # only the sections whose title matches
scripts/verify-audio.sh --list           # the section titles
```

It never uses the real microphone, headphones, or call apps. A fake microphone (a mono virtual source fed with a 440 Hz tone) and a temporary sink stand in for the devices, the sound is a 1000 Hz tone, and a call app is a `pw-record` of the fake microphone with a name of its own (`call_app`): what it records is "the call recording" below. The engine is always started with that app as its target. It checks that:

- the call recording has both tones on both channels (-40 dBFS or louder),
- the monitor recording has the sound and not the voice (below -60 dBFS),
- a branch volume of 0 silences only that branch,
- stopping one sound, or all of two sounds, silences both branches while the process, the drain node, and the voice stay,
- muting the call branch silences the sound in the call recording only, keeps the voice there, and unmuting brings the sound back,
- with `--no-call` the sound never reaches the call recording,
- a gain of 0.1 on one branch lowers it by 20 dB within 200 ms and leaves the other branch and the voice alone,
- a stop without a fade is silent 150 ms later,
- a playback gain raised from 0.1 to 1 raises both branches by 20 dB within 200 ms and leaves the voice alone, and a branch volume change keeps the playback gain,
- a looping 1 second tone is still there in its third pass with no silence longer than 15 ms at the seams (the call app records a silent microphone there, so silence means a gap), and reports no end,
- a loop turned off in the second pass ends after two passes, with one "finished" line and exit status 0,
- a restart goes back to the silence at the start of the file and plays again, with the voice untouched,
- the position 2 seconds in and the duration of the file are reported,
- a stop with a fade of 0.3 s is 1 to 15 dB down 100 to 200 ms later and silent after 600 ms, and reports no end; an exit in the middle of a 2 second fade leaves nothing behind,
- the output list names the fake sink, never the drain node, and follows a sink that is created and destroyed,
- with no app recording, the sound plays on the monitor to its end,
- with a target that is not recording, another app gets no sound and the monitor does,
- a target change in the middle of a sound takes it from the first app within 200 ms and gives it to the second,
- an app that starts recording in the middle of a sound gets it within 500 ms,
- an app moved to a stereo microphone (so its ports are replaced) has the sound back within 200 ms,
- with every app as the target (played with the call volume at 0, and checked on the links), two apps are linked and listed with their names, and a level meter (`stream.monitor=true`) and a recorder of the monitor of a sink are neither,
- switching to a second fake sink moves the playing sound within a second, without a gap in the call recording, and the next sound starts there,
- when the chosen sink is destroyed, the monitor branch falls back to the system default and the engine keeps running,
- a second and a third engine started in the same process (`--restart-engine-after`) create the drain node again, once, play on both branches, leave nothing behind, and the process has no more threads after the second drop than after the first,
- against a private PipeWire instance (`PIPEWIRE_CORE=NAME pipewire`, reached with `PIPEWIRE_REMOTE=NAME`) that is not running the start fails with the kind `unreachable`, and killing a running one is reported as `connection-lost` within 2 seconds. Nothing is played there: a private instance has no session manager, so the monitor branch never links and a playback stays at position 0. The harness removes the instance and its socket whatever happens,
- nothing is left behind after each kind of exit, and the app that was recording is still fed by its microphone,
- the default source and sink are unchanged.

If a real device is unplugged during the run, the system defaults change by themselves; that is printed as a `NOTE` instead of failing the "default source and sink unchanged" check.

The removal check falls back to the real default sink, so it plays with the monitor volume at 0.

It prints one `PASS` or `FAIL` line per check and exits with status 0 only when all pass. Recordings and logs go to `tmp/audio/`. It needs `ffmpeg` and `python3`, which are development tools only, not package dependencies.

## Manual call checklist

Optional, and not done yet: it needs a person on a real call. Its results decide the wording of the call setup guide of the app, which ships with generic advice until then. Use headphones unless a step says otherwise.

1. Start the app (`scripts/run-dev.sh`, or the installed package) and add a folder with one audible sound.
2. In the call app, keep the usual microphone (the default one is fine).
3. Join a call with someone else, or use the microphone test of the app.
4. Speak, then click the pad while speaking.
5. Ask the other side: do they hear the voice and the sound together? On both sides (left and right)?
6. Repeat step 4 with the noise suppression of the call app turned on, then off. Write down where that setting is, as the app names it.
7. Repeat step 4 with a long or looping sound and the noise suppression on: is it cut after some seconds?
8. Repeat step 4 with speakers instead of headphones and ask whether the sound is heard twice (echo).

| Check | Discord | Google Meet |
| --- | --- | --- |
| The app is listed in "Send Sounds To" while in the call | | |
| Voice and sound heard together | | |
| Voice on both sides | | |
| Sound survives with noise suppression on | | |
| Sound survives with noise suppression off | | |
| Echo with speakers | | |
| A long sound survives with noise suppression on | | |
| Where the noise suppression setting is | | |
