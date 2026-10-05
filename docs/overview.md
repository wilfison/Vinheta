# Overview

Vinheta is a soundboard for GNOME: Rust, GTK4 + libadwaita, PipeWire, and GStreamer. It is distributed as a `.deb` package for Ubuntu 26.04 and is not sandboxed. The current version is 1.0.0.

The hard part of the app is the audio, not the interface: see [audio.md](audio.md).

## What the app does

- The window shows one tab per folder, with a grid of pads. A click plays a sound and a second click stops it, through the audio engine (`src/audio/`).
- A sound plays on the headphones and into the apps that are recording a microphone, so a call hears it with the voice of the user. "Send sounds to call" turns the second part off, and "Send Sounds To" restricts it to one app.
- A pad has a name, a color, a volume, a loop option, and a key, edited from its context menu and its dialog and stored in `pads.json`. While it plays it shows its times and a border that gets shorter as the sound goes on, and the tab row shows how many sounds are playing.
- The library follows its folders while the app runs (files added, removed, renamed). Loose files are added with "Add Sounds…" or dropped on the window: they are copied into the sounds folder, which is a tab.
- Pads can be favorites (a star, and a "Favorites" tab), a search finds sounds in every folder and locates them, the pads are sorted by name or by most recent file, and tabs can be renamed and moved.
- The bottom bar has "Stop all", the "Headphones" and "Call" volume sliders, the selector of the app the sounds are sent to, and the "Send sounds to call" switch. Below 780 pixels of width it is stacked in rows and the pads get narrower; the window works down to 360 pixels.
- A pad key (a letter or a digit) triggers its pad from any tab while the window has the focus. "Stop All" and "Send Sounds to Call" have accelerators (Ctrl+Shift+S, L). There are no global shortcuts: see [shortcuts.md](shortcuts.md).
- The preferences dialog has the "Audio" group (monitor output, send sounds to call, the app the sounds are sent to), the "Playback" group (trigger mode, fade out on stop), and the "Library" group (sounds folder).
- The call setup guide opens on the first run and from the primary menu.
- Failures are told to the user: audio that is unavailable has a banner with the reason and "Try Again", a chosen device that is not connected and a pad file that could not be read or saved have a toast, and a missing folder has "Locate Folder…" and "Remove Folder" on its page.
- The interface is translated to Brazilian Portuguese.
- The package is built and published by GitHub Actions from a `v*` tag.

## How a sound reaches the call

Everything goes through PipeWire:

1. A call app (Discord, Meet, etc.) records the microphone the user chose there, usually the system default. That recording is a stream with input ports, and PipeWire mixes whatever is linked to them.
2. Each sound plays on **two branches**: the call (mono) and the headphones (local monitor), with separate volumes.
3. The app **links the call branch into the recording stream of every app that records a microphone**, next to the microphone itself, so the call hears voice and sounds together. A level meter and a recording of an output are left alone. "Send Sounds To" restricts the links to one app, and "Send sounds to call" mutes the branch.
4. Nothing is chosen in the call app, and no device is added to the system. A physical microphone has no input port, so the sound cannot be put into the device itself.

The node is created by the app's connection and disappears when the app closes, leaving nothing behind on the system.

## The library

The user adds folders, and each folder becomes a tab with one pad per audio file. Nothing is copied: the app reads the files where they are. Only loose files (dropped, or picked through "Add Sounds…") are copied, into the sounds folder. The details are in [library.md](library.md).

## Mockups

The SVGs in `mockups/` are the visual reference:

- `main-page.svg`: main window, with tabs per category, a grid of sound pads with a hotkey per item, elapsed/remaining time, and an indicator of how many sounds are playing.
- `empty-state.svg`: window with no sounds yet, with an "Add Sounds…" call to action (files can also be dropped), a tip to join a call with the usual microphone, and the bottom bar with "Stop all", monitor and call volumes, the selector of the app the sounds are sent to, and the "Send sounds to call" toggle.
- `preferences.svg`: preferences dialog, covering audio (monitor output, send sounds to call, the app the sounds are sent to), playback (behavior when a pad is triggered, fade out on stop), global shortcuts, and library (copy imported sounds, sounds folder).

The mockups show more than the app has. These parts were dropped:

- The "All" tab and the "Add sound" tile.
- The "Copy Imported Sounds" switch: loose files are always copied into the sounds folder.
- Durations on idle pads: a duration is only known from a running pipeline, so times are shown only while a pad plays.
- The "Shortcuts" group of the preferences (global shortcuts).

## Open items

- The manual call test ([audio.md](audio.md), "Manual call checklist") was never done: it needs a person on a real call. Until then two things are unknown:
  - **Noise suppression in call apps.** Discord (Krisp), Meet, and similar apps may cut the sounds out as if they were noise. The call setup guide tells the user to turn noise suppression off, as generic advice; the test may change the guide (the place of that setting in each call app).
  - **Echo.** If the local monitor leaks into the real microphone, the call hears the sound twice. The call setup guide recommends headphones.
- Showing the real device behind "System Default" or behind an unavailable choice. The engine does not report the default output, which needs a new engine event.
- A chosen app that was never seen recording in this run is named by its binary ("chrome (not recording)"), since only the name of the binary is stored.
- The tabs of a browser are one app: the sounds cannot be sent to one tab only.
- The label of an unavailable device is ellipsized in the preferences rows, which can hide the "(unavailable)" part.
- An automatic reconnection to PipeWire (today the user presses "Try Again").
- A PPA or another channel with updates, package signing, and other distributions.
- More languages.
- Global shortcuts ([shortcuts.md](shortcuts.md) has what was measured).
- Durations on idle pads would need a scan of each folder (for example with `GstDiscoverer`), done asynchronously so that a folder with hundreds of files stays fast.
