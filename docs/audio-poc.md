# Audio proof of concept (Phase 0)

This document records what was learned while proving the audio path of Vinheta: a virtual microphone that carries the user's voice and the soundboard sounds, plus a local monitor on the headphones. It is the reference for the later phases.

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

## Linking the microphone

The default source name comes from the `default` metadata:

```sh
pw-metadata 0 default.audio.source
# value:'{"name":"alsa_input.usb-...mono-fallback"}'
```

Rule, used by both the script and the engine:

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
- Both sinks set `state.restore-props=false`. Without it WirePlumber applies whatever volume it saved for an earlier stream of the same application, which made levels unpredictable during the tests.
- The microphone is linked to the node, not to the pipeline, so it never reaches the monitor branch.
- One pipeline per sound. Starting a second one while the first is playing works and both are mixed by PipeWire, but overlapping playback has not been verified beyond that.

## Branch offset

Measured by the harness: one channel of each branch is recorded into the same stereo file and the moment the 1000 Hz tone appears on each side is compared.

Over 8 runs (3 with the shell script, 5 with the Rust engine) the offset "call minus monitor" was either about 0 ms (0.0, 0.0, 0.0, 0.5, 0.5) or about -21 ms (-21.0, -21.0, -21.5). 21.3 ms is one PipeWire quantum (1024 samples at 48 kHz): depending on the order in which the graph processes the nodes, the monitor path picks the sound up one cycle later. There is no target for this value and it is far below what a call would notice.

## Cleanup

The Rust engine creates the node and the links on its own connection, without `object.linger`. The harness checks that, within 2 seconds, no `vinheta` node and no link to it remain after:

- a normal exit (`--once`, end of the file),
- SIGINT (Ctrl+C),
- SIGKILL (`kill -9`).

All three pass. The shell script uses lingering nodes and removes them in a `trap`, so `kill -9` on the script does leave the node behind; remove it with `pw-cli destroy`.

## Known issues

- **A live GStreamer source stalls the graph.** `audiotestsrc is-live=true ! pipewiresink` with `node.autoconnect=false`, linked by hand into a virtual node, left every node suspended and every later link stuck in the `init` state. File sources (`uridecodebin`, `filesrc`) do not have the problem. The soundboard only plays files, but keep this in mind before feeding a live source into the node.
- **An unlinked call branch blocks the whole pipeline**, including the monitor branch, because the sink does not consume data until it is linked. The engine links the stream as soon as its ports show up in the registry, so in practice the delay is not noticeable.
- **`pw-link` by port name failed once** with "No such file or directory" right after the node was created, and worked on retry and by port id. It was not reproduced.
- **Noise suppression and echo have not been tested**; see the manual call checklist below.

## Running it

The shell proof of concept (command line tools only):

```sh
scripts/audio-poc.sh [--mic NODE_NAME] [--monitor NODE_NAME] [--call-volume N] [--monitor-volume N] FILE
```

The Rust engine, through the diagnostic binary (behind the `audio-poc` cargo feature, never installed or packaged):

```sh
cargo run --features audio-poc --bin vinheta-audio-poc -- [OPTIONS] [FILE]
```

- Without `FILE`: creates the node, links the microphone, and waits for Ctrl+C.
- With `FILE`: plays it once, and again each time Enter is pressed.
- `--once`: plays `FILE` once and exits when it ends.
- `--mic`, `--monitor`, `--call-volume`, `--monitor-volume`: same meaning as in the script.
- `--version`: prints the PipeWire and GStreamer library versions.

The automated check:

```sh
scripts/verify-audio-poc.sh shell   # checks scripts/audio-poc.sh
scripts/verify-audio-poc.sh rust    # builds and checks vinheta-audio-poc
```

It never uses the real microphone or headphones. A fake microphone (a mono virtual source fed with a 440 Hz tone) and a temporary sink stand in for them, and the sound is a 1000 Hz tone. It checks that:

- the call recording has both tones on both channels (-40 dBFS or louder),
- the monitor recording has the sound and not the voice (below -60 dBFS),
- a branch volume of 0 silences only that branch,
- nothing is left behind after each kind of exit,
- the default source and sink are unchanged.

It prints one `PASS` or `FAIL` line per check and exits with status 0 only when all pass. Recordings and logs go to `tmp/audio-poc/`. It needs `ffmpeg` and `python3`, which are development tools only, not package dependencies.

## Manual call checklist

Optional. It does not block Phase 0, but its results feed the first-run screen planned for Phase 6. Use headphones unless a step says otherwise.

1. Start the engine: `cargo run --features audio-poc --bin vinheta-audio-poc -- some-sound.ogg`.
2. In the call app, pick "Vinheta" as the microphone (input device).
3. Join a call with someone else, or use the app's microphone test.
4. Speak, then press Enter in the terminal to play the sound while speaking.
5. Ask the other side: do they hear the voice and the sound together? On both sides (left and right)?
6. Repeat step 4 with the app's noise suppression turned on, then off.
7. Repeat step 4 with speakers instead of headphones and ask whether the sound is heard twice (echo).

| Check | Discord | Google Meet |
| --- | --- | --- |
| "Vinheta" is listed as an input device | | |
| Voice and sound heard together | | |
| Voice on both sides | | |
| Sound survives with noise suppression on | | |
| Sound survives with noise suppression off | | |
| Echo with speakers | | |
