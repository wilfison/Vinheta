# Roadmap: Vinheta, a soundboard for GNOME

**Stack:** Rust, GTK4 + libadwaita, PipeWire, GStreamer. Distributed as a `.deb` (Ubuntu 26.04), not sandboxed.

The hard part of this app is the audio, not the interface. That is why the audio proof of concept comes before any UI work.

## How "sound into the microphone" works

Everything goes through PipeWire:

1. The app creates a **virtual microphone** named "Vinheta" (a `support.null-audio-sink` node with `media.class=Audio/Source/Virtual`).
2. The app **links the real microphone** to that node, so voice and sounds are mixed. This link is controlled by the "Include my voice" option.
3. Each sound plays on **two outputs**: the virtual microphone (what the call hears) and the headphones (local monitor), with separate volumes. The virtual microphone branch is controlled by the "Send sounds to call" option.
4. In Discord, Meet, etc., the user picks "Vinheta" as the input device.

The node is created by the app's connection and disappears when the app closes, leaving nothing behind on the system.

## Library model

- The user adds **directories**; each directory becomes a **tab**.
- Every audio file in the directory is shown as a pad. Nothing is copied: the app reads the files where they are.
- The list of directories (and the tab order) is stored in GSettings.
- Per-pad metadata (name, color, volume, loop, shortcut) is stored as JSON in `~/.local/share/vinheta`, keyed by file path.
- Loose files (dropped, or picked through "Add Sounds…") are copied into the **default sounds folder** ("Sounds folder" in the preferences), which is itself a library directory and therefore a tab.

## Phases

### Phase 0: audio proof of concept ✅

The biggest risk in the project. No interface here.

**Done.** The findings, the scripts, and how to re-run the checks are in [docs/audio-poc.md](docs/audio-poc.md).

First on the command line, without writing any Rust:

- ✅ Create the virtual node with `pw-cli create-node adapter`. Checked with `pw-dump`, `pw-link`, and `wpctl status` instead of Helvum or `qpwgraph`, because they can be scripted.
- ✅ Play a file into it with `gst-launch-1.0`.
- ✅ Link the real microphone to the node with `pw-link`, on both channels (FL and FR) if the microphone is mono.
- ✅ Answer the main open question: WirePlumber does **not** accept a playback stream whose `target-object` points at an `Audio/Source/Virtual` node (it sends the stream to the default sink). The stream uses `node.autoconnect=false` and the links are created by hand.

Then in Rust:

- ✅ Add `pipewire` 0.10, `gstreamer` 0.23, and `async-channel` to `Cargo.toml`. `gstreamer` 0.23 is the version built on `glib 0.20`, the same as `gtk4 0.9`.
- ✅ Create the node, the microphone link (`link-factory`), and the pipeline from code (`src/audio/`).
- ✅ Update `debian/control` with the build dependencies and the runtime plugins.

Done when: someone on a call hears your voice and the sound together, and the node disappears when the app closes.

- ✅ Voice and sound arrive together on both channels of the virtual microphone. Checked by `scripts/verify-audio-poc.sh` with a fake microphone and a recording, not by a person on a call.
- ✅ The node and its links disappear after a normal exit, Ctrl+C, and `kill -9`.
- ⬜ Test on a real call (Discord, Google Meet), including noise suppression and echo. The steps are in the manual call checklist of `docs/audio-poc.md`.

What changed from the original plan:

- The local monitor was brought forward from Phase 2: the pipeline already has a `tee` with two branches, each with its own `queue` and `volume`.
- The engine always creates the call branch links itself. `target-object` is only used for the monitor branch.
- `Depends` also lists `gstreamer1.0-plugins-base` and `gstreamer1.0-plugins-good` (decoders and converters), not only `gstreamer1.0-pipewire`.
- The crate became a library plus binaries: `src/lib.rs` exposes `audio`, and `vinheta-audio-poc` (behind the `audio-poc` cargo feature) exercises the engine without the interface. It is kept as a diagnostic tool until Phase 6.
- The engine follows the system default source while it runs, and falls back to a physical source when the default is the "Vinheta" node itself.

Notes for the next phases:

- The offset between the call branch and the monitor branch is 0 ms or about 21 ms (one PipeWire cycle), which needs no fix.
- Both sinks set `state.restore-props=false`; without it WirePlumber applies a volume saved for an earlier stream.
- A live GStreamer source (for example `audiotestsrc is-live=true`) linked by hand into the node stalls the graph. Files do not.
- The capture ports of a virtual source carry `port.monitor=true`; do not filter monitor ports out when walking the registry.
- What the engine did not do at the end of Phase 0: stop a sound, change a volume while playing, turn the call branch or the microphone link on and off, choose devices at run time. Phase 1 added the stop and the call branch switch; Phase 2 added the rest.
- After changing audio code, run `scripts/verify-audio-poc.sh rust`.

### Phase 1: interface MVP ✅

- ✅ Window following `mockups/main-page.svg` and `mockups/empty-state.svg`.
- ✅ Add a directory with `GtkFileDialog` (`select_folder`); each directory becomes a tab.
- ✅ Pad grid using `GtkGridView` on top of a list model (makes search and sorting easier later).
- ✅ Wire `AudioEngine` (`src/audio/`) into the application: start it with the app and consume its events with `glib::spawn_future_local`.
- ✅ Clicking plays, clicking again stops. The engine has a per-sound stop, keyed by a `PlaybackId` returned by `play`.
- ✅ Bottom bar with "Stop all" and the "Send sounds to call" toggle, with a public stop-all and a call branch mute in the engine.
- ✅ Empty state with the call to action and the tip to pick the "Vinheta" microphone in the call app.
- ✅ Persist the directories in GSettings (`directories`, plus `send-sounds-to-call`).
- ✅ Removing a tab only removes the directory from the library, never the files.

What changed from the original plan:

- The add button and the empty state only add folders ("Add Folder…"). Loose files and "Add Sounds…" stay in Phase 4.
- The controls of later phases that the mockup shows (volume sliders, microphone selector, search, hotkey badges, favorites, loop, times, playing counter, "Add sound" tile, "All" tab) are left out instead of shown disabled.
- Tabs are an `AdwViewStack` with an `AdwInlineViewSwitcher`, which needs the `v1_7` feature of the `libadwaita` crate. The switcher scrolls horizontally when the tabs do not fit.
- A folder is removed from the primary menu ("Remove Folder"), with an "Undo" toast. Only files directly inside a folder are listed; subfolders are not entered.
- The call branch is muted, not dropped: the stream stays linked, so turning the switch back on is instant and applies to sounds already playing.
- Sounds are started and stopped through the `app.toggle-sound` and `app.stop-all` actions, which Phase 5 can reuse for shortcuts.
- When the engine cannot start, the window opens with a banner and disabled pads; playback failures are toasts. The rest of the error handling is still Phase 6.
- Stopping a sound exposed a bug in the engine: a harmless PipeWire error ("unknown resource", after the server removes a link with its stream) was treated as a lost connection. Only `EPIPE` is now.
- New tooling: `scripts/screenshot.sh` captures the window on a virtual display, the project has its first Rust tests (`src/library.rs`, run by `meson test`), and `scripts/verify-audio-poc.sh rust` checks stop, stop all, and the call mute.

Notes for the next phases:

- Starting a sound takes about 1 to 3 ms on the GTK thread, so it stays there.
- A folder is scanned once, when its tab is created. Phase 4 adds the file monitor.
- The undo of a removal and the folder chooser were not exercised by the scripted checks (no simulated input); test them by hand.
- The window was not checked at its minimum width (360 pixels) for the same reason.

### Phase 2: mixing controls ✅

- ✅ Two volumes: monitor and call, adjustable while sounds play, as two sliders in the bottom bar.
- ✅ Selectors for the real microphone (bottom bar and preferences) and the monitor device (preferences).
- ✅ "Include my voice" option (creates and removes the real microphone link).
- ✅ The microphone selector never offers the "Vinheta" node.
- ✅ React to devices being plugged in and removed (PipeWire registry events): the lists follow, and a chosen device that is removed is used again when it comes back.
- ✅ Preferences dialog (`mockups/preferences.svg`) and the `app.preferences` action.

What changed from the original plan:

- The preferences dialog only has the "Audio" group. "Playback", "Shortcuts", and "Library" come with the phases that own them.
- The monitor selector lives only in the preferences; the microphone selector is in both places, as in the mockups.
- The sliders go from 0% to 100% (no amplification) with a cubic curve from position to gain. The settings store the position.
- Changing the monitor output moves the sounds that are playing, through the `target.object` key of the default metadata. The system default is asked for with `-1`, because removing the key does not undo the target a stream was created with.
- A chosen device that is not connected is shown as "(unavailable)" and replaced by the system default until it returns. For the monitor WirePlumber does that by itself; for the microphone the engine does.
- The minimum window width went from 360 to 820 pixels, which is what the bottom bar needs.
- Virtual sources other than "Vinheta" are offered as microphones.
- `Ctrl+,` opens the preferences.
- Tooling: `scripts/screenshot.sh` gained `--setting` and `--exec`, `scripts/check.sh` no longer fails on a clean tree, and `scripts/verify-app.sh` measures levels with the voice off.

Notes for the next phases:

- A volume change is heard within 200 ms and does not touch the other branch (measured by the harness).
- Phase 3 (per-pad volume) can multiply into the same `volume` elements, or add one before the `tee`.
- Phase 4: the bottom bar needs an adaptive layout before the window can be narrow again.
- Phase 5: consider a shortcut for "Include My Voice".
- Phase 6: show which device is really in use behind "System Default" or an unavailable choice. The engine reports the linked microphone (`MicLinked`) but not the default output. The label of an unavailable device is ellipsized in the preferences rows, which can hide the "(unavailable)" part.
- A noisy real microphone hides quiet test tones on the virtual microphone; turn the voice off before measuring.
- Dragging a slider with the mouse and switching between two real outputs were not exercised by the scripted checks; test them by hand.

### Phase 3: per-pad features

- ⬜ Loop, fade in/out, individual volume.
- ⬜ Editable color and name.
- ⬜ Elapsed and remaining time, and a highlighted border while playing.
- ⬜ Indicator of how many sounds are playing.
- ⬜ Trigger mode: overlap, restart, or stop the others. Overlap is what the engine does today (one pipeline per sound).
- ⬜ JSON file for per-pad metadata.

### Phase 4: organization

- ⬜ Watch the directories with `gio::FileMonitor` to reflect new, removed, and renamed files.
- ⬜ Drag and drop of files and "Add Sounds…", copying into the default sounds folder.
- ⬜ Search, favorites, and sorting (`GtkFilterListModel`, `GtkSortListModel`).
- ⬜ Reorder and rename tabs.
- ⬜ Adaptive layout with `AdwBreakpoint`.

### Phase 5: shortcuts

- ⬜ Local shortcuts first, with the window focused (keys 1 to 9, etc.), as in the mockup.
- ⬜ Global shortcuts next, through the `GlobalShortcuts` portal (`ashpd` crate).
- ⬜ With the portal, the user picks the key in the system dialog; the app can only suggest one. The shortcuts page in the preferences has to respect that.
- ⬜ Test early whether the portal identifies the app by its app ID outside a sandbox.

### Phase 6: polish and release

- ⬜ First-run screen explaining how to pick "Vinheta" in the call app and how to turn off its noise suppression. This is where most users get stuck.
- ⬜ Error handling: PipeWire missing, corrupted file, directory removed, microphone removed. The engine already reports the first two and a missing microphone as `Error` values; the interface has to show them.
- ⬜ Icon, metainfo, screenshots, and translations (gettext; `po/LINGUAS` is still empty).
- ⬜ Publishing the `.deb` (GitHub releases or a PPA).
- ⬜ Before the release, remove `vinheta-audio-poc`, the `audio-poc` cargo feature, and the `rust` mode of `scripts/verify-audio-poc.sh`.

## Risks to check early

- **Playback into an `Audio/Source/Virtual` node**: settled in Phase 0. `target-object` does not work (WirePlumber sends the stream to the default sink), so the engine always uses `node.autoconnect=false` and creates the links itself, whatever the WirePlumber version.
- **Noise suppression in call apps**: Discord (Krisp), Meet, and similar apps may cut the sounds out as if they were noise. Not tested yet: run the manual call checklist in `docs/audio-poc.md`; the result feeds the first-run screen of Phase 6.
- **Echo**: if the local monitor leaks into the real microphone, the call hears the sound twice. Recommend headphones. Not tested yet (same checklist).
- **Mono microphone into a stereo node**: settled in Phase 0. A source with a single port is linked to both channels; the same rule covers mono sound files.
- **Latency and sync** between the two `tee` branches: settled in Phase 0. Each branch has a `queue`, and the measured offset is at most one PipeWire cycle (about 21 ms).
- **Threads**: settled in Phase 0 for the engine. The PipeWire loop runs on its own thread, with a `pipewire::channel` for commands and `async-channel` for events. Still to do in Phase 1: consume the events from the GTK thread with `glib::spawn_future_local`.
- **Crate versions**: `gtk4`, `libadwaita`, and `gstreamer` must share the same `glib` version. Today that is `glib 0.20` with `gstreamer` 0.23; bumping `gtk4` means bumping `gstreamer` with it (`cargo tree -i glib` must list one version).
- **Large directories**: hundreds of files in one tab. `GtkGridView` handles it, but reading file durations must be asynchronous.
