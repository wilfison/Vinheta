# Changelog

All notable changes to Vinheta are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Loud sounds no longer clip the call: each sound reaches the call at -6 dBFS at most, with no distortion, while the headphones hear it at its full level. "Limit Call Level" in the preferences turns it off.

## [1.2.0] - 2026-10-06

### Added

- A pad can have a background image: drop an image on the pad, or choose one in its "Edit Sound" dialog. Vinheta keeps a small copy of it, so the original can be moved or deleted.
- Audio comes back by itself when PipeWire stops or restarts: the app tries again on its own, at growing intervals, and "Try Again" still tries at once.

### Changed

- The settings of sounds deleted from their folder are forgotten when the app starts. Those of a folder that is missing, such as one on a drive that is not connected, are kept.

## [1.1.0] - 2026-10-05

### Changed

- The sounds are now played straight into the apps that are using your microphone. Keep your usual microphone in the call app: there is no "Vinheta" microphone to choose anymore, and the call setup guide says so.
- The bottom bar and the preferences have a "Send Sounds To" selector: every app that is using the microphone (the default), or one of them.
- The sounds reach the call in mono.
- A new app icon, with a waveform on the playing pad.

### Removed

- The "Vinheta" virtual microphone, the microphone selector, and the "Include my voice" switch with its Ctrl+Shift+M shortcut. Your voice reaches the call through the call app itself.

## [1.0.0] - 2026-10-04

The first public release.

### Added

- A virtual microphone named "Vinheta" that carries your voice together with the sounds you play. Choose it as the microphone in your call app.
- One tab per folder of sounds, kept in sync with the folder while the app runs. Loose files can be added with "Add Sounds…" or dropped on the window.
- Pads with a name, a color, a volume, a loop, and a key (a letter or a digit) that plays them from any tab.
- A readable pad name made from the file name, and a border on a playing pad that gets shorter until the sound ends.
- "Open in Audio Editor" in the menu of a pad, with the editor chosen in the preferences.
- A context menu on each folder tab to rename, move, or remove it.
- Favorites in a tab of their own, a search across every folder, and sorting by name or by most recent file.
- Separate volumes for your headphones and for the call, a microphone selector, and the "Send sounds to call" and "Include my voice" switches.
- Trigger modes (overlap, restart, stop the others) and a fade out when a sound is stopped.
- A call setup guide, shown on the first run and from the menu.
- Clear messages when audio is unavailable, with a "Try Again" button, and notices for a device that is not connected, a damaged pad settings file, and a folder that went away (with "Locate Folder…").
- A narrow layout: the window works down to 360 pixels of width.
- A Brazilian Portuguese translation.

[Unreleased]: https://github.com/wilfison/Vinheta/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/wilfison/Vinheta/releases/tag/v1.2.0
[1.1.0]: https://github.com/wilfison/Vinheta/releases/tag/v1.1.0
[1.0.0]: https://github.com/wilfison/Vinheta/releases/tag/v1.0.0
