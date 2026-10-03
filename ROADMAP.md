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

### Phase 0: audio proof of concept

The biggest risk in the project. No interface here.

**Done.** The findings, the scripts, and how to re-run the checks are in [docs/audio-poc.md](docs/audio-poc.md). A test on a real call is still pending (the manual call checklist in that document).

First on the command line, without writing any Rust:

- Create the virtual node with `pw-cli create-node adapter` and check in Helvum or `qpwgraph` that it shows up.
- Play a file into it with `gst-launch-1.0`: `uridecodebin ! audioconvert ! audioresample ! volume ! pipewiresink target-object=<node>`.
- Link the real microphone to the node with `pw-link`, on both channels (FL and FR) if the microphone is mono.
- Answer the main open question: does WirePlumber accept a playback stream whose `target-object` points at an `Audio/Source/Virtual` node? If not, use `node.autoconnect=false` and create the links by hand.

Then in Rust:

- Add `pipewire` (pipewire-rs) and `gstreamer` to `Cargo.toml`. Check that the `gstreamer` version uses the same `glib` as `gtk4` (today `gtk4 0.9` uses `glib 0.20`).
- Create the node, the microphone link (`link-factory`), and the pipeline from code.
- Update `debian/control`: `libpipewire-0.3-dev`, `libgstreamer1.0-dev`, `libgstreamer-plugins-base1.0-dev`, and `libclang-dev` in `Build-Depends`; `gstreamer1.0-pipewire` in `Depends` (plugins are not picked up by `${shlibs:Depends}`).

Done when: someone on a call hears your voice and the sound together, and the node disappears when the app closes.

### Phase 1: interface MVP

- Window following `mockups/main-page.svg` and `mockups/empty-state.svg`.
- Add a directory with `GtkFileDialog` (`select_folder`); each directory becomes a tab.
- Pad grid using `GtkGridView` on top of a list model (makes search and sorting easier later).
- Clicking plays, clicking again stops.
- Bottom bar with "Stop all" and the "Send sounds to call" toggle.
- Empty state with the "Add Sounds…" call to action and the tip to pick the "Vinheta" microphone in the call app.
- Persist the directories in GSettings (the schema is currently empty).
- Removing a tab only removes the directory from the library, never the files.

### Phase 2: mixing controls

- Two volumes: monitor and call. This needs a `tee` in the pipeline with two branches, each with its own `volume`.
- Selectors for the real microphone and the monitor device.
- "Include my voice" option (creates and removes the real microphone link).
- The microphone selector must never offer the "Vinheta" node. The engine already falls back to a physical source when "Vinheta" is the system default.
- React to devices being plugged in and removed (PipeWire registry events).
- Preferences dialog (`mockups/preferences.svg`) and the `app.preferences` action, which the menu already references but does not exist yet.

### Phase 3: per-pad features

- Loop, fade in/out, individual volume.
- Editable color and name.
- Elapsed and remaining time, and a highlighted border while playing.
- Indicator of how many sounds are playing.
- Trigger mode: overlap, restart, or stop the others.
- JSON file for per-pad metadata.

### Phase 4: organization

- Watch the directories with `gio::FileMonitor` to reflect new, removed, and renamed files.
- Drag and drop of files and "Add Sounds…", copying into the default sounds folder.
- Search, favorites, and sorting (`GtkFilterListModel`, `GtkSortListModel`).
- Reorder and rename tabs.
- Adaptive layout with `AdwBreakpoint`.

### Phase 5: shortcuts

- Local shortcuts first, with the window focused (keys 1 to 9, etc.), as in the mockup.
- Global shortcuts next, through the `GlobalShortcuts` portal (`ashpd` crate).
- With the portal, the user picks the key in the system dialog; the app can only suggest one. The shortcuts page in the preferences has to respect that.
- Test early whether the portal identifies the app by its app ID outside a sandbox.

### Phase 6: polish and release

- First-run screen explaining how to pick "Vinheta" in the call app and how to turn off its noise suppression. This is where most users get stuck.
- Error handling: PipeWire missing, corrupted file, directory removed, microphone removed.
- Icon, metainfo, screenshots, and translations (gettext; `po/LINGUAS` is still empty).
- Publishing the `.deb` (GitHub releases or a PPA).
- Before the release, remove `vinheta-audio-poc`, the `audio-poc` cargo feature, and the `rust` mode of `scripts/verify-audio-poc.sh`.

## Risks to check early

- **Playback into an `Audio/Source/Virtual` node**: settled in Phase 0. `target-object` does not work (WirePlumber sends the stream to the default sink), so the engine always uses `node.autoconnect=false` and creates the links itself, whatever the WirePlumber version.
- **Noise suppression in call apps**: Discord (Krisp), Meet, and similar apps may cut the sounds out as if they were noise. Not tested yet: run the manual call checklist in `docs/audio-poc.md`; the result feeds the first-run screen of Phase 6.
- **Echo**: if the local monitor leaks into the real microphone, the call hears the sound twice. Recommend headphones.
- **Mono microphone into a stereo node**: link the microphone to both channels, otherwise the call hears the voice on one side only.
- **Latency and sync** between the two `tee` branches: use a `queue` on each branch.
- **Threads**: the PipeWire loop runs outside the GTK thread. Connect the two with channels (`async-channel` with `glib::spawn_future_local`).
- **Crate versions**: `gtk4`, `libadwaita`, and `gstreamer` must share the same `glib` version.
- **Large directories**: hundreds of files in one tab. `GtkGridView` handles it, but reading file durations must be asynchronous.
