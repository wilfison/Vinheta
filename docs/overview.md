# Overview

Vinheta is a soundboard for GNOME: Rust, GTK4 + libadwaita, PipeWire, and GStreamer. It is distributed as a `.deb` package for Ubuntu 26.04 and is not sandboxed. The current version is 1.0.0.

The hard part of the app is the audio, not the interface: see [audio.md](audio.md).

## What the app does

- The window shows one tab per folder, with a grid of pads. A click plays a sound and a second click stops it, through the audio engine (`src/audio/`).
- A sound plays on the headphones and into the "Vinheta" virtual microphone, which also carries the voice of the user. "Send sounds to call" and "Include my voice" turn each part off.
- A pad has a name, a color, a volume, a loop option, and a key, edited from its context menu and its dialog and stored in `pads.json`. While it plays it shows its times, a progress bar, and a border, and the tab row shows how many sounds are playing.
- The library follows its folders while the app runs (files added, removed, renamed). Loose files are added with "Add Sounds…" or dropped on the window: they are copied into the sounds folder, which is a tab.
- Pads can be favorites (a star, and a "Favorites" tab), a search finds sounds in every folder and locates them, the pads are sorted by name or by most recent file, and tabs can be renamed and moved.
- The bottom bar has "Stop all", the "Headphones" and "Call" volume sliders, the microphone selector, and the "Send sounds to call" switch. Below 780 pixels of width it is stacked in rows and the pads get narrower; the window works down to 360 pixels.
- A pad key (a letter or a digit) triggers its pad from any tab while the window has the focus. "Stop All", "Send Sounds to Call", and "Include My Voice" have accelerators (Ctrl+Shift+S, L, M). There are no global shortcuts: see [shortcuts.md](shortcuts.md).
- The preferences dialog has the "Audio" group (microphone, monitor output, send sounds to call, include my voice), the "Playback" group (trigger mode, fade out on stop), and the "Library" group (sounds folder).
- The call setup guide opens on the first run and from the primary menu.
- Failures are told to the user: audio that is unavailable has a banner with the reason and "Try Again", a chosen device that is not connected and a pad file that could not be read or saved have a toast, and a missing folder has "Locate Folder…" and "Remove Folder" on its page.
- The interface is translated to Brazilian Portuguese.
- The package is built and published by GitHub Actions from a `v*` tag.

## How a sound reaches the call

Everything goes through PipeWire:

1. The app creates a **virtual microphone** named "Vinheta" (a `support.null-audio-sink` node with `media.class=Audio/Source/Virtual`).
2. The app **links the real microphone** to that node, so voice and sounds are mixed. This link is controlled by the "Include my voice" option.
3. Each sound plays on **two outputs**: the virtual microphone (what the call hears) and the headphones (local monitor), with separate volumes. The virtual microphone branch is controlled by the "Send sounds to call" option.
4. In Discord, Meet, etc., the user picks "Vinheta" as the input device.

The node is created by the app's connection and disappears when the app closes, leaving nothing behind on the system.

## The library

The user adds folders, and each folder becomes a tab with one pad per audio file. Nothing is copied: the app reads the files where they are. Only loose files (dropped, or picked through "Add Sounds…") are copied, into the sounds folder. The details are in [library.md](library.md).

## Mockups

The SVGs in `mockups/` are the visual reference:

- `main-page.svg`: main window, with tabs per category, a grid of sound pads with a hotkey per item, elapsed/remaining time, and an indicator of how many sounds are playing.
- `empty-state.svg`: window with no sounds yet, with an "Add Sounds…" call to action (files can also be dropped), a tip to pick the "Vinheta" virtual microphone in the call app, and the bottom bar with "Stop all", monitor and call volumes, the real microphone, and the "Send sounds to call" toggle.
- `preferences.svg`: preferences dialog, covering audio (real microphone, monitor output, send sounds to call, include my voice), playback (behavior when a pad is triggered, fade out on stop), global shortcuts, and library (copy imported sounds, sounds folder).

The mockups show more than the app has. These parts were dropped:

- The "All" tab and the "Add sound" tile.
- The "Copy Imported Sounds" switch: loose files are always copied into the sounds folder.
- Durations on idle pads: a duration is only known from a running pipeline, so times are shown only while a pad plays.
- The "Shortcuts" group of the preferences (global shortcuts).

## Open items

- The manual call test ([audio.md](audio.md), "Manual call checklist") was never done: it needs a person on a real call. Until then two things are unknown:
  - **Noise suppression in call apps.** Discord (Krisp), Meet, and similar apps may cut the sounds out as if they were noise. The call setup guide tells the user to turn noise suppression off, as generic advice; the test may change the guide (the place of that setting in each call app).
  - **Echo.** If the local monitor leaks into the real microphone, the call hears the sound twice. The call setup guide recommends headphones.
- Showing the real device behind "System Default" or behind an unavailable choice. The engine reports the linked microphone (`MicLinked`) but not the default output, which needs a new engine event.
- The label of an unavailable device is ellipsized in the preferences rows, which can hide the "(unavailable)" part.
- An automatic reconnection to PipeWire (today the user presses "Try Again").
- A PPA or another channel with updates, package signing, and other distributions.
- More languages.
- Global shortcuts ([shortcuts.md](shortcuts.md) has what was measured).
- Durations on idle pads would need a scan of each folder (for example with `GstDiscoverer`), done asynchronously so that a folder with hundreds of files stays fast.
