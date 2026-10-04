# The audio engine

This document describes the audio engine of Vinheta (`src/audio/`): a virtual microphone that carries the user's voice and the soundboard sounds, plus a local monitor on the headphones. It also records what was measured with command line tools before the engine was written; those command lines are kept as the record of the measurements.

## Source layout

- `src/audio/`: the audio engine. `mod.rs` is the public API (`AudioEngine`, `Config`, `PlayOptions`, `Position`, `Event`, `Error`, `PlaybackId`, `Device`, `slider_gain`); `graph.rs` owns the PipeWire thread (virtual microphone node, registry, links); `player.rs` builds one GStreamer pipeline per sound. PipeWire objects never leave the engine thread: commands go in through a `pipewire::channel`, events come out through `async-channel`.
- `src/bin/vinheta-audio-test.rs`: diagnostic binary behind the `audio-test` cargo feature (`cargo run --features audio-test --bin vinheta-audio-test -- --help`). Meson does not build or install it.

## Rules that are easy to get wrong

- WirePlumber does not route playback into the virtual microphone. The call branch sink uses `node.autoconnect=false` and `graph.rs` links any stream whose node name starts with `vinheta-call-`.
- The node and links are created without `object.linger`, so they vanish when the process ends. Do not add it.
- A playback that was stopped on request never reports an event afterwards; the interface relies on that, and on events carrying the `PlaybackId`.
- "Send sounds to call" mutes the `call-volume` element of each pipeline. The call stream and its links stay in place. The call volume is the `volume` property of the same element, independent of the mute.
- The gain of a branch element is a product: branch × playback × fade (`effective_gain`). Never set a gain before the `tee`: it is heard a queue late. A branch volume change keeps each playback's own gain.
- The queues of a pipeline hold 200 ms, so the end of a pass is known about 350 ms before it is heard.
- Every playback runs in segment mode, looping or not: preroll, a flushing `SEGMENT` seek, then `PLAYING`; on each `SEGMENT_DONE` either a non-flushing seek (loop) or an EOS pushed into the sink pad of the `tee` (the only way it ends). These actions are queued with `call_async` from the bus handler, and skipped once the playback is retired.
- `restart` is a flushing seek: the `PlaybackId` and the streams stay, and nothing is reported.
- With a fade set, a stopped playback is forgotten at once (no events, unknown to every call that takes its id, untouched by volume changes) while a thread of the engine ramps it down in 10 ms steps and stops it 150 ms after the ramp. Dropping the engine stops fading playbacks at once.
- The volumes and the mute are set by `player.rs` on the caller's thread; the microphone, the voice switch, and the monitor output are commands for the PipeWire thread.
- The settings store slider positions (0 to 1); the engine takes gains. `slider_gain` (cubic) converts.
- The monitor branch is routed by WirePlumber. A new playback gets the chosen output as `target-object`; a running one is moved by writing `target.object` for its stream (node name `vinheta-monitor-*`) in the default metadata. The system default is asked for with the value `-1`: removing the key would leave the target the stream was created with.
- `Error` tells the fatal failures apart: `Unreachable` (PipeWire cannot be reached at the start), `ConnectionLost` (while running: the engine is of no use afterwards), `NodeExists`. `Error::kind` is a stable name for logs and scripts. An engine can be started again in the same process after the previous one was dropped, also after a lost connection.
- A chosen device that does not exist is not an error to recover from in the interface. For the monitor, WirePlumber uses the default sink and moves the stream when the device shows up. For the microphone, `graph.rs` reports `MicNotFound` once, links the default source, and links the chosen one when it appears.
- `Event::DevicesChanged` is only sent when a list really changed, and never lists the "Vinheta" node.
- After changing audio code, run `scripts/verify-audio.sh`. Its sections on a second engine, a taken node name, and a lost connection (on a private PipeWire instance it starts and destroys) need no sound. While working on one behavior, `--only REGEX` runs only the sections whose title matches (`--list` prints the titles); run all of it before committing. It uses fake devices only (no real microphone or headphones) and needs `ffmpeg` and `python3`. In a new check, place the measurements with `after FILE SECONDS [MARGIN]` (an action scheduled `SECONDS` into the playback) instead of hand-computed offsets. A sound with no silence at its start needs `--start-after 1`, so the recorders are ready, and `window` instead of `after`. `window` fails the run when it finds no onset: the tone must reach -40 dBFS in the recording, which a gain of 0.1 does not. A real device unplugged during the run is reported as a `NOTE`, not as a failure.
- The call branch is muted, not dropped, so turning "Send sounds to call" back on is instant and applies to sounds already playing.
- "Fade Out on Stop" is one global setting (300 ms, on by default). There is no fade in and no fade per pad, and a restart does not fade.
- A volume change is heard within 200 ms and does not touch the other branch (measured by the harness).
- The engine follows the system default source while it runs, and falls back to a physical source when the default is the "Vinheta" node itself.

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

## The virtual microphone

```sh
pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=vinheta node.description=Vinheta media.class=Audio/Source/Virtual audio.position=[FL FR] object.linger=true }'
```

- `pw-cli` exits right away, so on the command line the node needs `object.linger=true` and must be removed with `pw-cli destroy <id>`. The Rust engine does not set it: the node belongs to the engine's connection and dies with it.
- `wpctl status` lists "Vinheta" under Sources, and the default source and sink do not change.
- Ports: `vinheta:input_FL` and `vinheta:input_FR` (what gets mixed in), `vinheta:capture_FL` and `vinheta:capture_FR` (what a call app records).
- The capture ports carry `port.monitor=true`. Code that walks the registry must not skip monitor ports, or it will not find the output of a virtual source.
- `pw-record --target vinheta` works: a recording stream is routed to the node by WirePlumber like to any microphone.
- `wpctl set-default` refuses the node ("not a device node"), but the default metadata accepts it: `pw-metadata 0 default.audio.source '{"name":"vinheta"}'` makes it the default source. The engine then falls back to a physical source (see "Linking the microphone").

## Does `target-object` work for playback? No

```sh
gst-launch-1.0 uridecodebin uri=file:///path/sound.wav ! audioconvert ! audioresample ! volume \
    ! pipewiresink target-object=vinheta
```

WirePlumber ignores the target and links the stream to the default sink. `pw-link -l` during playback:

```
vinheta-play:output_FL
  |-> alsa_output.usb-...analog-stereo:playback_FL
vinheta-play:output_FR
  |-> alsa_output.usb-...analog-stereo:playback_FR
```

The recording taken from the node was silent. A playback stream is not routed to an `Audio/Source/Virtual` node.

What works, and what the engine always does regardless of this answer: the stream does not autoconnect and is linked by hand.

```sh
gst-launch-1.0 uridecodebin uri=file:///path/sound.wav ! audioconvert ! audioresample ! volume \
    ! pipewiresink stream-properties="p,node.autoconnect=false,node.name=vinheta-play" &
pw-link vinheta-play:output_FL vinheta:input_FL
pw-link vinheta-play:output_FR vinheta:input_FR
```

- Before the links exist, `pw-link -l` shows the stream linked to nothing: WirePlumber leaves it alone.
- The pipeline waits until it is linked, then plays from the start.
- The links disappear with the stream when playback ends. Nothing has to be removed by hand.
- `target-object` does work for the monitor branch, where the target is a regular `Audio/Sink`.

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

## Linking the microphone

The default source name comes from the `default` metadata:

```sh
pw-metadata 0 default.audio.source
# value:'{"name":"alsa_input.usb-...mono-fallback"}'
```

The rule:

- A source with one output port (mono) is linked to both `input_FL` and `input_FR`. Otherwise the call hears the voice on one side only.
- A source with several ports is linked channel by channel (`FL` to `FL`, `FR` to `FR`), matching on the `audio.channel` port property.

```sh
pw-link "$MIC:capture_MONO" vinheta:input_FL
pw-link "$MIC:capture_MONO" vinheta:input_FR
```

With the mono microphone linked, a recording from the node had the same level on both channels (-51.1 dBFS of room noise on each). After `pw-link -d` the recording was silent again.

The same rule applies to sounds: a mono file produces a stream with a single `output_MONO` port, which is linked to both inputs.

If the source to link is the "Vinheta" node itself, the engine links the first physical source instead (lowest id among nodes with `media.class=Audio/Source`, which excludes virtual sources) and reports which one it picked. The engine follows the default source while it runs: when the default changes, the old links are removed and new ones are created. With no physical source at all it reports an error and links nothing (checked against a private PipeWire instance: `PIPEWIRE_CORE=vinheta-test pipewire`, then the binary with `PIPEWIRE_REMOTE=vinheta-test`).

## The final pipeline

```
uridecodebin ! audioconvert ! audioresample ! tee name=t
  t. ! queue ! volume ! pipewiresink   (call: node.autoconnect=false, linked to vinheta by the engine)
  t. ! queue ! volume ! pipewiresink   (monitor: default sink, or target-object=<sink>)
```

- Each branch has its own `queue` and `volume`. Setting one volume to 0 silences only that branch.
- The queues hold 200 ms (`max-size-time`, with the buffer and byte limits off) instead of the default second, and the gain of each `volume` element is a product: branch × playback × fade.
- Both sinks set `state.restore-props=false`. Without it WirePlumber applies whatever volume it saved for an earlier stream of the same application, which made levels unpredictable during the tests.
- The microphone is linked to the node, not to the pipeline, so it never reaches the monitor branch.
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

Measured by the harness: one channel of each branch is recorded into the same stereo file and the moment the 1000 Hz tone appears on each side is compared.

Over 8 runs (3 with command line tools, 5 with the Rust engine) the offset "call minus monitor" was either about 0 ms (0.0, 0.0, 0.0, 0.5, 0.5) or about -21 ms (-21.0, -21.0, -21.5). 21.3 ms is one PipeWire quantum (1024 samples at 48 kHz): depending on the order in which the graph processes the nodes, the monitor path picks the sound up one cycle later. There is no target for this value and it is far below what a call would notice.

## Cleanup

The Rust engine creates the node and the links on its own connection, without `object.linger`. The harness checks that, within 2 seconds, no `vinheta` node and no link to it remain after:

- a normal exit (`--once`, end of the file),
- SIGINT (Ctrl+C),
- SIGKILL (`kill -9`).

All three pass. A node made with `pw-cli` and `object.linger` (as the fake devices of the checks are) stays until `pw-cli destroy`.

## Known issues

- **A live GStreamer source stalls the graph.** `audiotestsrc is-live=true ! pipewiresink` with `node.autoconnect=false`, linked by hand into a virtual node, left every node suspended and every later link stuck in the `init` state. File sources (`uridecodebin`, `filesrc`) do not have the problem. The soundboard only plays files, but keep this in mind before feeding a live source into the node.
- **An unlinked call branch blocks the whole pipeline**, including the monitor branch, because the sink does not consume data until it is linked. The engine links the stream as soon as its ports show up in the registry, so in practice the delay is not noticeable.
- **`pw-link` by port name failed once** with "No such file or directory" right after the node was created, and worked on retry and by port id. It was not reproduced.
- **Noise suppression and echo have not been tested**; see the manual call checklist below.
- **The server removes the engine's links together with a stream that ends.** When the engine then drops its own proxy for such a link, PipeWire reports "unknown resource" on the core. That error is harmless and must not be treated as a lost connection; only `EPIPE` on the core is. It shows up whenever a sound is stopped while the process keeps running.

## Running it

The engine is exercised without the interface through the test binary (behind the `audio-test` cargo feature, never installed or packaged):

```sh
cargo run --features audio-test --bin vinheta-audio-test -- [OPTIONS] [FILE...]
```

- Without `FILE`: creates the node, links the microphone, and waits for Ctrl+C.
- With one or more `FILE`s: plays them together once, and again each time Enter is pressed.
- `--once`: plays the files once and exits when they end.
- `--no-call`: starts with the call branch muted (the "Send sounds to call" switch turned off).
- `--stop-after SECONDS`: stops the first file that many seconds after the playback starts.
- `--stop-all-after SECONDS`: stops every file.
- `--mute-call-after SECONDS`, `--unmute-call-after SECONDS`: turn the call branch off and on while the files play.
- `--call-volume-after SECONDS GAIN`, `--monitor-volume-after SECONDS GAIN`: change the gain of a branch (0 to 1) while the files play.
- `--no-voice`: starts without the microphone link. `--voice-off-after SECONDS`, `--voice-on-after SECONDS`: remove and restore it.
- `--mic-after SECONDS NODE_NAME`, `--monitor-after SECONDS NODE_NAME`: switch the microphone and the monitor output; the name `default` follows the system default.
- `--replay-after SECONDS`: plays the files again.
- `--volume GAIN`: the gain of every file (0 to 1). `--playback-volume-after SECONDS GAIN`: changes the gain of the first file.
- `--loop`: every file loops. `--loop-off-after SECONDS`: ends the loop of the first file. `--restart-after SECONDS`: starts the first file again.
- `--position-after SECONDS`: prints `position ELAPSED_MS DURATION_MS` for the first file (`unknown` for a missing duration, `position none` when it is over).
- `--fade-out SECONDS`: how long a stopped file takes to fade out (0 by default).
- `--start-after SECONDS`: waits before the first playback, so the recorders of the harness are ready for a sound with no silence at its start.
- The device lists are printed after the start and whenever they change (`devices changed`, then `microphone: NAME (DESCRIPTION)` and `output: NAME (DESCRIPTION)` lines).
- `--restart-engine-after SECONDS`: drops the engine that many seconds after the playback starts, starts a new one in the same process, and plays the files again. It does so twice, and prints `threads N` (the threads of the process) after each drop.
- `--mic NODE_NAME`, `--monitor NODE_NAME`: the microphone and the monitor output. `--call-volume N`, `--monitor-volume N`: the gain of each branch.
- An error is printed as `error: KIND: TEXT`. `KIND` is the stable name of the variant (`Error::kind`): `unreachable` (PipeWire cannot be reached at the start), `connection-lost`, `node-exists`, `pipewire`, `gstreamer`, `mic-not-found`, `no-microphone`, `file-not-found`, `playback`.
- `--version`: prints the PipeWire and GStreamer library versions.

The automated check:

```sh
scripts/verify-audio.sh                  # builds and checks vinheta-audio-test
scripts/verify-audio.sh --only 'loop'    # only the sections whose title matches
scripts/verify-audio.sh --list           # the section titles
```

It never uses the real microphone or headphones. A fake microphone (a mono virtual source fed with a 440 Hz tone) and a temporary sink stand in for them, and the sound is a 1000 Hz tone. It checks that:

- the call recording has both tones on both channels (-40 dBFS or louder),
- the monitor recording has the sound and not the voice (below -60 dBFS),
- a branch volume of 0 silences only that branch,
- stopping one sound, or all of two sounds, silences both branches while the process, the node, and the voice stay,
- muting the call branch silences the sound in the call recording only, keeps the voice there, and unmuting brings the sound back,
- with `--no-call` the sound never reaches the call recording,
- a gain of 0.1 on one branch lowers it by 20 dB within 200 ms and leaves the other branch and the voice alone,
- a stop without a fade is silent 150 ms later,
- a playback gain raised from 0.1 to 1 raises both branches by 20 dB within 200 ms and leaves the voice alone, and a branch volume change keeps the playback gain,
- a looping 1 second tone is still there in its third pass with no silence longer than 15 ms at the seams (voice off, so silence means a gap), and reports no end,
- a loop turned off in the second pass ends after two passes, with one "finished" line and exit status 0,
- a restart goes back to the silence at the start of the file and plays again, with the voice untouched,
- the position 2 seconds in and the duration of the file are reported,
- a stop with a fade of 0.3 s is 1 to 15 dB down 100 to 200 ms later and silent after 600 ms, and reports no end; an exit in the middle of a 2 second fade leaves nothing behind,
- the device lists name the fake devices, never the virtual microphone, and follow a sink that is created and destroyed,
- turning the voice off removes it from the call recording and keeps the sound, and `--no-voice` starts that way,
- switching to a second fake microphone (880 Hz) replaces the voice in the call recording,
- switching to a second fake sink moves the playing sound within a second, without a gap in the call recording, and the next sound starts there,
- when the chosen sink or microphone is destroyed, the engine falls back to the system default and keeps running, and the chosen microphone is linked again when it returns,
- a second and a third engine started in the same process (`--restart-engine-after`) create the node again, once, play on both branches, leave nothing behind, and the process has no more threads after the second drop than after the first,
- while another node named `vinheta` exists the start fails with the kind `node-exists`, and it succeeds once that node is gone,
- against a private PipeWire instance (`PIPEWIRE_CORE=NAME pipewire`, reached with `PIPEWIRE_REMOTE=NAME`) that is not running the start fails with the kind `unreachable`, and killing a running one is reported as `connection-lost` within 2 seconds. Nothing is played there: a private instance has no session manager, so the monitor branch never links and a playback stays at position 0. The harness removes the instance and its socket whatever happens,
- nothing is left behind after each kind of exit,
- the default source and sink are unchanged.

If a real device is unplugged during the run, the system defaults change by themselves; that is printed as a `NOTE` instead of failing the "default source and sink unchanged" check.

The two removal checks fall back to the real default devices, so the sink one plays with the monitor volume at 0 and the microphone one plays and records nothing.

It prints one `PASS` or `FAIL` line per check and exits with status 0 only when all pass. Recordings and logs go to `tmp/audio/`. It needs `ffmpeg` and `python3`, which are development tools only, not package dependencies.

## Manual call checklist

Optional, and not done yet: it needs a person on a real call. Its results decide the wording of the call setup guide of the app, which ships with generic advice until then. Use headphones unless a step says otherwise.

1. Start the app (`scripts/run-dev.sh`, or the installed package) and add a folder with one audible sound.
2. In the call app, pick "Vinheta" as the microphone (input device).
3. Join a call with someone else, or use the microphone test of the app.
4. Speak, then click the pad while speaking.
5. Ask the other side: do they hear the voice and the sound together? On both sides (left and right)?
6. Repeat step 4 with the noise suppression of the call app turned on, then off. Write down where that setting is, as the app names it.
7. Repeat step 4 with a long or looping sound and the noise suppression on: is it cut after some seconds?
8. Repeat step 4 with speakers instead of headphones and ask whether the sound is heard twice (echo).

| Check | Discord | Google Meet |
| --- | --- | --- |
| "Vinheta" is listed as an input device | | |
| Voice and sound heard together | | |
| Voice on both sides | | |
| Sound survives with noise suppression on | | |
| Sound survives with noise suppression off | | |
| Echo with speakers | | |
| A long sound survives with noise suppression on | | |
| Where the noise suppression setting is | | |
