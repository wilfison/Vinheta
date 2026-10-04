# AGENTS.md

This file provides guidance to AI coding agents working with code in this repository.

## What the project is

Vinheta is a soundboard for GNOME (GTK4 + libadwaita, written in Rust, distributed as a `.deb` package).

Intended product behavior:

- The user adds directories; each directory becomes a tab in the app.
- Every sound in the directory is shown in the interface; clicking a sound plays it.
- The sound plays on the audio output **and** is injected into the user's microphone (for use in calls).
- Microphone injection can be turned off from the interface, leaving only the audio output.

The SVGs in `mockups/` are the visual reference:

- `main-page.svg`: main window, with tabs per category, a grid of sound pads with a hotkey per item, elapsed/remaining time, and an indicator of how many sounds are playing.
- `empty-state.svg`: window with no sounds yet, with an "Add Sounds…" call to action (files can also be dropped), a tip to pick the "Vinheta" virtual microphone in the call app, and the bottom bar with "Stop all", monitor and call volumes, the real microphone, and the "Send sounds to call" toggle.
- `preferences.svg`: preferences dialog, covering audio (real microphone, monitor output, send sounds to call, include my voice), playback (behavior when a pad is triggered, fade out on stop), global shortcuts, and library (copy imported sounds, sounds folder).

**Current state:** The six phases of `ROADMAP.md` are done and the version is 1.0.0. The window shows one tab per folder with a grid of pads and plays and stops sounds through the audio engine (`src/audio/`). A pad has a name, a color, a volume, and a loop option, edited from its context menu and stored in `pads.json`; while it plays it shows its times, a progress bar, and a border, and the tab row shows how many sounds are playing. The library follows its folders while the app runs (files added, removed, renamed), loose files are added with "Add Sounds…" or dropped on the window (copied into the sounds folder, which is a tab), pads can be favorites (a star, and a "Favorites" tab), a search finds sounds in every folder and locates them, the pads are sorted by name or by most recent file, and tabs can be renamed and moved. The bottom bar has "Stop all", the "Headphones" and "Call" volume sliders, the microphone selector, and the "Send sounds to call" switch; below 780 pixels of width it is stacked in rows and the pads get narrower (the window works down to 360 pixels). A pad can have a key (a letter or a digit, chosen in its dialog and shown as a badge) that triggers it from any tab while the window has the focus; "Stop All", "Send Sounds to Call", and "Include My Voice" have accelerators (Ctrl+Shift+S, L, M). Shortcuts only work while the window has the focus: global shortcuts (through the `GlobalShortcuts` portal) were tried in Phase 5 and dropped. The preferences dialog has the "Audio" group (microphone, monitor output, send sounds to call, include my voice), the "Playback" group (trigger mode, fade out on stop), and the "Library" group (sounds folder). The call setup guide opens on the first run and from the primary menu. Failures are told to the user: audio that is unavailable has a banner with the reason and "Try Again", a chosen device that is not connected and a pad file that could not be read or saved have a toast, and a missing folder has "Locate Folder…" and "Remove Folder" on its page. The interface is translated to Brazilian Portuguese. The package is built and published by GitHub Actions from a `v*` tag. The "All" tab, the "Add sound" tile, "Copy Imported Sounds", durations on idle pads, and the "Shortcuts" group of the preferences (global shortcuts) of the mockups were dropped. Left open: the manual call test of `docs/audio.md`.

## Build and run

Meson is the real build system; it generates the configuration, compiles the resources, and invokes `cargo` underneath.

```sh
meson setup build            # configure (add --prefix=... to choose where it installs)
meson compile -C build       # compiles the gresource + cargo build
meson test -C build          # validates the .desktop file, metainfo (appstreamcli), GSettings schema, and runs the Rust tests
meson test -C build "Validate schema file"   # run a single test
```

The Rust tests are unit tests of code that needs no GTK, PipeWire, or display (today `src/library.rs` with the scan, the folder diff, and the import, `src/devices.rs`, `src/pads.rs` with the pad file, the pad keys and their uniqueness, the trigger and play rules, the search match, and the sort order, and the pure functions of `src/audio/`: the slider curve and the gain product); run them directly with `cargo test --lib`. The validations are defined in `data/meson.build` and the Rust test in `src/meson.build`. debhelper runs `meson test` while building the package, so tests must not depend on a session. The audio engine is checked by `scripts/verify-audio.sh` (see Architecture).

### Running in development

`scripts/run-dev.sh` does everything below in one step: it builds, installs into the local prefix, and runs the app (`--debug` prints the app's debug messages). By hand:

Install into a local prefix inside `build/`, so nothing touches the system and no `sudo` is needed:

```sh
meson setup build --prefix="$PWD/build/install"   # once; add --reconfigure if build/ already exists
meson install -C build                            # rebuilds and reinstalls after each change
GSETTINGS_SCHEMA_DIR="$PWD/build/install/share/glib-2.0/schemas" \
XDG_DATA_DIRS="$PWD/build/install/share:$XDG_DATA_DIRS" \
build/install/bin/vinheta
```

- The prefix is baked into `PKGDATADIR` at configure time, which is how the binary finds `vinheta.gresource`.
- `GSETTINGS_SCHEMA_DIR` points at the schema compiled into the local prefix; `XDG_DATA_DIRS` lets the app find its icon there.
- Building requires `libgtk-4-dev` and `libadwaita-1-dev` (they pull in the GLib dev tools, including `glib-compile-resources`), plus `libpipewire-0.3-dev`, `libgstreamer1.0-dev`, `libgstreamer-plugins-base1.0-dev`, and `libclang-dev` for the audio engine (`libclang` is used by bindgen in the `pipewire` crate). Playing audio needs `gstreamer1.0-pipewire`, `gstreamer1.0-plugins-base`, and `gstreamer1.0-plugins-good` at run time. Installing `libxml2-utils` silences the `xmllint` warning when compiling the gresource.

### Checks

Run `scripts/check.sh` before committing. It prints one `PASS` or `FAIL` line per check and keeps the logs in `tmp/check/`.

```sh
scripts/check.sh           # rustfmt, clippy without warnings, a single glib version, no em dash, one version, complete translations, meson test
scripts/check.sh --audio   # plus the audio engine harness (fake devices, about 4 minutes)
scripts/check.sh --app     # plus scripts/verify-app.sh on the installed app
scripts/check.sh --deb     # plus the package build
scripts/check.sh --all
```

- `scripts/verify-app.sh` checks the installed app end to end on a virtual display: the virtual microphone, playing and stopping through actions and through real clicks, the "Send sounds to call" switch and the call volume measured on a recording, the voice switch, the monitor output (with a fake sink), the pad volume and loop, the trigger modes, a file removed while it plays, the settings of a renamed file, the saved pad settings, a pad key pressed for real, audio coming back after "Try Again", and the cleanup on exit. It uses the real PipeWire and plays a quiet tone on the default output for a few seconds. The levels are measured with the voice off, because a noisy real microphone hides the tone.
- While working on one behavior, `scripts/verify-app.sh --only REGEX` runs only the sections whose title matches (`--list` prints the titles); the start and the exit of the app are always checked. Every section starts and stops what it needs, so a new section must not depend on the one before it. Run all of it before committing.
- `scripts/check.sh --all` takes more than 10 minutes. An agent whose commands time out before that runs it in the background and reads `tmp/check/` or its output when it ends.
- The code is formatted with `cargo fmt` (default settings); the check fails when it would change something.
- `scripts/build-deb.sh` builds the package from a copy under `tmp/deb`, so nothing is written to the working tree or to its parent directory.
- `scripts/version.sh` prints the version when its four places agree (see "Debian package"), and `scripts/check-translations.sh` fails when a file with strings is missing from `po/POTFILES.in`, when a `gettext` call of a `.rs` file did not reach the template, or when a language of `po/LINGUAS` is incomplete. Both are checks of `scripts/check.sh`.
- `scripts/ci.sh` is what GitHub Actions runs (`.github/workflows/ci.yml` on pushes to `main` and pull requests, on the `ubuntu-26.04` runner): it installs the build dependencies with `apt-get build-dep ./`, uses the Rust of the distribution (`/usr/bin` first in `PATH`), and runs `scripts/check.sh --deb`. It installs packages, so do not run it on a developer machine: `scripts/ci-container.sh` rehearses it in a clean `ubuntu:26.04` container (Docker) on a copy of the tree, with `lintian`, and installs the package in a second clean container. The audio and app checks need a PipeWire session and a display, so they never run on the CI.
- `scripts/dev-common.sh` holds what these scripts share (the local prefix environment, the virtual session, test sounds). New development scripts should source it from bash (it refuses any other shell, where it would compute the wrong root).
- Scripted checks never touch the user's settings or pad settings: `virtual_session` points `XDG_CONFIG_HOME` at its own directory and `XDG_DATA_HOME` at the `data` directory inside it.

### Checking the interface

`scripts/screenshot.sh` runs the installed app on a virtual display, with its own D-Bus session and its own settings, and captures the window:

```sh
meson install -C build
scripts/screenshot.sh grid --folder DIR --action "toggle-sound '/abs/path/sound.wav'"
```

- It writes `tmp/screenshots/NAME.png`. Screenshots are temporary: read them, then delete them (`scripts/screenshot.sh --clean`).
- `scripts/fixtures.sh` creates the folders these checks use in `tmp/fixtures` and prints them: `Fixture` (three silent pads), `Palette` with `palette.json` (one pad per color), `Loop` with `loop.json` (a 3 second pad that loops), `dialog.json` (one pad with every setting), `Many` (60 pads, enough to scroll), `Effects` (three pads, so that a search for "app" has results in two folders), `favorites.json` (two favorites in two folders and a pad named "Zebra"), and `shortcuts.json` (the keys 1 on "Air Horn", Q on "Applause", and W on "Bell" of `Effects`), `Broken` (one file that holds text, so playing it fails), and `corrupt-pads.json` (not valid JSON, for `--pads`).
- A check that creates, renames, or deletes files works on a copy of a fixture (for example under `tmp/monitor-test`), never in `tmp/fixtures`.
- The virtual display has no window manager: the content of the window is 10 pixels narrower than the size given to `--size`. Use `--size 370,700` for the narrow layout at 360 pixels (dialogs need exactly 360).
- A scripted import lands in `tmp/screenshot-config/data/vinheta/sounds`, and a trashed file in `tmp/screenshot-config/data/Trash`.
- `--folder` fills the library, `--setting 'KEY VALUE'` sets any other key of the app's settings before it starts (`VALUE` is a GVariant: `0.5`, `false`, `"'name'"`), `--no-audio` makes the engine fail to start, `--light` uses the light style, `--lang pt_BR` runs the app in that language (every scripted session is in English otherwise, whatever the language of the user).
- Every run starts with `call-guide-shown` set to true, so the call setup guide does not open; `--first-run` leaves the key at its default.
- `--private-pipewire` starts a PipeWire instance of the script's own (no session manager, no devices) and points the app and the fake devices at it; the steps `--stop-pipewire` and `--start-pipewire` kill and start it while the app runs. It is how a lost connection and a system with no microphone are reached. A pad shows as playing at 00:00 there, and `--expect-playing` does not work with it.
- `--pads FILE` gives the app a pad settings file to start with: `{"version": 1, "pads": {"/abs/sound.wav": {"name": "Intro", "color": "purple", "volume": 0.8, "loop": true}}}`. The one the app wrote during the run stays in `tmp/screenshot-config/data/vinheta/pads.json`.
- Steps run in the given order before the capture: `--action` activates an `app.*` or `win.*` action, `--click X,Y`, `--right-click X,Y`, and `--key KEYS` simulate the user (`xdotool`), `--size W,H` resizes the window, `--wait SECONDS` waits, `--restart` quits the app and starts it again with the same settings (to check what is restored), `--exec COMMAND` runs a shell command (a status other than 0 is reported as a failed step).
- A step can check instead of showing: `--expect-setting 'KEY VALUE'` (the value as `gsettings get` prints it: `false`, `0.5`, `'text'`) and `--expect-playing N` (how many sounds play) wait up to 2 seconds and make the script exit with 1 when they fail. Prefer them to reading a picture or the debug output for anything that is a value.
- `--key` reaches the app with no click before it; to hold a key use `--exec "xdotool keydown q; sleep 1; xdotool keyup q"`. The display is 1100 by 800: scroll a tall dialog with `--exec 'xdotool mousemove 500 400 click --repeat 12 --delay 30 5'`.
- The scripts are bash. From zsh an unquoted `$VAR` holding several options is passed as one word: put a sequence of runs in a bash script (under `tmp/`) instead of typing it.
- One run can take several pictures: the step `--capture NAME` writes `tmp/screenshots/NAME.png` at that point, and `--crop WxH+X+Y` crops the captures that follow (`--crop full` undoes it). Prefer that to starting the app once per state. `--sheet` also writes `NAME-sheet.png`, every capture of the run stacked, to review them in one look.
- Warnings and criticals logged by the app are printed at the end of the run; treat them as findings. `--debug` also prints the app's debug messages (for example how long starting a sound took).
- Dialogs opened by an action (`--action preferences`) and open popovers are captured.
- The device lists come from the real PipeWire, so they differ between machines. For a known entry use a fake device: `--fake-mic 'NODE DESCRIPTION'` and `--fake-sink 'NODE DESCRIPTION'` create one before the app starts, the steps `--plug-mic`, `--plug-sink`, and `--unplug NODE` do it while the app runs. Name the nodes `vinheta-shot-*`. The script destroys them when it ends.
- Portal dialogs (the folder chooser) do not work on the virtual display and cannot be captured; test them by hand.
- The app still uses the real PipeWire: it creates the real "Vinheta" node for a few seconds and plays on the default output. Use silent files with `toggle-sound`.
- It needs `xvfb-run`, `dbus-run-session`, `xdotool`, and ImageMagick (`import`), which are development tools only.

Non-obvious points:

- `src/config.rs` is **generated** from `src/config.rs.in` by Meson and copied back into `src/`. Edit the `.in`, never the `.rs`. New build-time constants also need a `conf.set_quoted(...)` in `src/meson.build`.
- `cargo build` / `cargo check` / `cargo clippy` work directly for checking the code (as long as `src/config.rs` already exists), but the resulting binary does not run on its own: `main.rs` loads `vinheta.gresource` from `PKGDATADIR` (`/usr/share/vinheta` in the `.deb`) and panics if it is missing. To run the app, install it through Meson (see "Running in development") or install the `.deb`.
- cargo downloads crates during compilation (sources are not vendored), so builds need network access.
- The `gnome_47` (gtk4) and `v1_7` (libadwaita) features in `Cargo.toml` cap the APIs available in the Rust bindings; bump the features to use newer APIs.
- rust-analyzer needs the `rust-src` component. Without it, it reports errors that the compiler does not (`cannot apply unary operator` on `bool` or `i32`, `None` flagged as a variable name). The asdf Rust install does not ship it: download `rust-src-VERSION.tar.xz` from `static.rust-lang.org/dist` and run its `install.sh --prefix=` with the install directory of that Rust version. Trust `cargo clippy` over the editor when they disagree.

## Debian package

Packaging lives in `debian/` and uses debhelper with the Meson build system.

```sh
dpkg-buildpackage -us -uc -b   # writes ../vinheta_<version>_<arch>.deb
```

Prefer `scripts/build-deb.sh`: run in the working tree, `dpkg-buildpackage` leaves `obj-*/` and files under `debian/` behind (all git-ignored; `git clean -Xd debian obj-*` removes them), and rust-analyzer indexes the copy of `config.rs` in there.

- `debian/rules` forces `--buildtype=release`; debhelper's default (`plain`) would make `src/meson.build` produce an unoptimized debug binary.
- The minimum versions in `debian/control` (GTK 4.20, libadwaita 1.8) come from the `Cargo.toml` features, from `AdwShortcutsDialog` in `shortcuts-dialog.ui`, and from the media query in `style.css` (GTK 4.20). Keep them in sync when either changes. The package targets Ubuntu 26.04 (`resolute`).
- New native libraries (for example, for audio) must be added to `Build-Depends`; runtime library dependencies are filled in by `${shlibs:Depends}`.
- The version lives in four places: `meson.build`, `Cargo.toml` (and `Cargo.lock`), `debian/changelog`, and the `release` of the metainfo. `scripts/version.sh` fails when they differ.
- `lintian` reports two warnings that are accepted: `initial-upload-closes-no-bugs` and `no-manual-page`.

### Releasing

A release is a tag: `.github/workflows/release.yml` runs on a pushed `v*` tag (or by hand, with the tag as input), checks the tag against the version, runs the CI gate again, and creates the GitHub release with the `.deb`, its `.sha256`, and notes made by `scripts/release-notes.sh` from `CHANGELOG.md`. A second run for the same tag updates the release.

1. Bump the version in the four places, with the same date in `debian/changelog` and in the metainfo, and add a `## [VERSION]` section to `CHANGELOG.md`.
2. Run `scripts/check.sh --all` (the CI does not run the audio and app checks) and, when the packaging changed, `scripts/ci-container.sh`.
3. Commit, push `main`, wait for the CI, then `git tag vVERSION && git push origin vVERSION`.

## Architecture

- `src/main.rs`: sets up gettext, registers the gresource, and starts `VinhetaApplication`.
- `src/application.rs`: `adw::Application` subclass; registers the `app.*` actions in `setup_gactions()`. It owns the `AudioEngine` (started in `startup`, dropped in `shutdown`), consumes its events with `glib::spawn_future_local`, and keeps the map from `PlaybackId` to the `Sound` being played. `app.toggle-sound` (parameter: the absolute path of a sound of the library) follows the trigger mode (`trigger` in `src/pads.rs` with the `trigger-mode` key): start, stop, restart, or start and stop the others. With `app.play-sound` (what the search uses: `play` in `src/pads.rs`, which starts or restarts a sound and never stops one), `app.stop-sound`, and `app.stop-all` it is the only way sounds are started and stopped; the pads activate them too. `app.preferences` opens the preferences dialog and `app.call-guide` the call setup guide. `app.send-sounds-to-call` and `app.include-my-voice` are stateful actions made from the settings keys (`create_action`), which the accelerators flip.
- The application owns the pad settings (`PadStore`, loaded in `startup` from `pads.json` in `glib::user_data_dir()/vinheta`; a file that cannot be parsed is renamed to `pads.json.corrupt` and the user is told; when the rename fails nothing is saved in that session, so the damaged file is never overwritten; a failed save is told once per session). `update_sound` is the one way to change them: it updates the `Sound`, the store, and the running playback, and schedules a save (at most one write every 500 ms, flushed in `shutdown`). `app.toggle-loop`, `app.toggle-favorite`, and `app.reset-sound` (parameter: the path) and the sound dialog go through it; it also tells the window when a name or a favorite changed, because the sorted and filtered views do not watch their items. It keeps a pad key unique: a key given to a sound is taken from the entry that had it (`take_shortcut`), whose `Sound` and dialog are told. `app.set-shortcut` (parameter `(ss)`: the path and the key, empty for none) and `app.trigger-shortcut` (a key: what `app.toggle-sound` does for the sound that has it) are how scripts and the window reach the keys.
- Audio that is unavailable: `start_audio` starts an engine with the current settings and returns whether it did; on a failure, and when the engine reports `ConnectionLost` or `PipeWire`, `set_audio_error` drops the engine and keeps the reason as an `AudioFailure`, from which the window picks the sentence of its banner (never the `Display` text of the engine, which only goes to the log). `app.retry-audio` (the "Try Again" button of the banner, enabled only while audio is unavailable) calls `start_audio` again; there is no automatic retry. Each engine has a generation number, so what an old one still reports is ignored.
- Notices: `notify` shows a toast, or keeps it until there is a window (the pad file is read in `startup`, before the window exists; `activate` flushes them). A sticky notice stays until dismissed. `update_missing_devices` tells once when a chosen device goes from present to missing (after the engine listed its devices, and when the key changes), and `device_missing` is the lasting mark: the subtitle of the preferences rows, and the tooltip and `warning` class of the microphone icon of the bottom bar. `NoMicrophone` is told once per engine while "Include My Voice" is on.
- The call setup guide (`src/ui/call_guide_dialog.rs` + `call-guide-dialog.ui`) opens by itself in `activate` while the `call-guide-shown` key is false and audio works; the key is set when the dialog closes, however it closes.
- Pad keys are handled by one `GtkEventControllerKey` of the window (`setup_pad_keys` in `window.rs`), in the bubble phase, so text fields, menus, and dialogs get their keys first. It ignores keys with Shift, Ctrl, Alt, or Super and any key while a dialog is open, and remembers which keys are down, because a held key repeats as ordinary presses. A key no pad has is not consumed.
- The sounds folder is the `sounds-folder` key, or `sounds` inside the data directory of the app (`sounds_folder`). `app.trash-sound` moves a file to the system trash only when it is directly inside that folder; the "Move to Trash" item of a pad is hidden elsewhere.
- While a sound plays, one timer of the application (every 100 ms) asks the engine for the position of each playback and writes it to the `Sound`. It does not exist while nothing plays.
- The settings are the single source of truth for the mix: the application builds the engine `Config` from them and forwards every change of a key to the engine (`call-volume`, `monitor-volume`, `microphone`, `monitor-output`, `include-my-voice`, `send-sounds-to-call`, and `fade-out-on-stop`, which is a fade of 300 ms or none). The interface only binds widgets to keys and never calls the engine for these. The application also keeps the device lists reported by the engine and emits its `devices-changed` signal when they, or the audio availability, change.
- `src/ui/`: the interface components. A widget's `.rs` file lives here next to its `.ui` template, together with the `.ui` and `.css` files that have no Rust side. `application.rs` and `sound.rs` are not widgets and stay in `src/`.
- `src/ui/window.rs` + `src/ui/window.ui`: `adw::ApplicationWindow` subclass using a composite template; widgets from the `.ui` file are bound with `#[template_child]`. It owns the tabs (`AdwViewStack` with an `AdwInlineViewSwitcher`), the playing counter next to them, the `directories` setting (always written from the pages, in tab order), and the bottom bar. Without audio the pads are dimmed, not insensitive, so their menu still works.
- The window owns what every view of pads shares: one `GtkCustomSorter` (`compare` in `src/pads.rs` with the `sort-order` key, exposed as `win.sort-order`), and a `GtkFlattenListModel` over the stores of the folder pages, in tab order. The "Favorites" tab (page name `favorites`, always last, hidden while empty) and the search results are a `GtkFilterListModel` of it, sorted by the same sorter, so every view shows the same `Sound` objects.
- Search: the state is the `GtkSearchBar` being open plus the text of its entry (`win.search-mode` for the button and Ctrl+F, `win.search` with a text for scripts). While there is text the results replace the tab row and the tabs. A result activates `win.activate-result`: `app.play-sound`, close the search, `win.locate-sound` (show the tab, `scroll_to`, blink the pad). The search bar captures no keys of the window.
- Loose files: `win.import-files` (a list of paths) is the one entry point for "Add Sounds…" (`win.add-sounds`), the drop target of the window, and scripts. Folders are added to the library; files go through `import` of `src/library.rs` off the main thread, and the sounds folder is added as a tab (titled "Sounds" until renamed).
- Tabs: the title is the entry of the `folder-names` key, or the name of the directory. `win.rename-folder` (an `AdwAlertDialog` from `rename-dialog.ui`), `win.move-folder-left`, `win.move-folder-right`, and `win.remove-folder`, `win.find-folder` (the folder chooser of "Locate Folder…"), and `win.relocate-folder` (a path: the tab is replaced by a page of that folder at the same position, and its name and its pad settings move with it, through `PadStore::move_folder`) act on the folder tab being shown and are disabled for "Favorites" and during a search. The stack can only append, so reordering removes and adds pages again (`set_pages`); the pages are the same objects.
- Narrow layout: an `AdwBreakpoint` (`max-width: 780px`) switches the bottom bar, an `AdwMultiLayoutView`, to its stacked layout and raises the minimum height; its `apply` and `unapply` signals toggle the `narrow` CSS class, which makes the pads and the paddings smaller. The class `bottom-bar` is on both the bar of the toolbar view and its content, so its padding counts twice.
- The window does not know its application while it is constructed (`application` is set afterwards); `window.rs` uses the default application.
- `src/ui/preferences_dialog.rs` + `src/ui/preferences-dialog.ui`: the `AdwPreferencesDialog`.
- `src/ui/sound_dialog.rs` + `src/ui/sound-dialog.ui`: the `AdwDialog` that edits one pad (name, color, volume, loop, key). While its "Shortcut" row listens, a key controller in the capture phase takes every key, so Escape ends the listening instead of closing the dialog. Every control acts at once through `update_sound`, and the dialog follows changes made elsewhere.
- `src/ui/device_selector.rs`: not a widget. `bind` keeps a `GtkDropDown` or an `AdwComboRow` in sync with a device list of the application and with the key that stores the chosen device. It is the only place that writes `microphone` and `monitor-output`. A change of the key rebuilds the list in an idle callback, never at once: the key may be changing from inside the activation of an entry, and replacing the model there makes GTK log a critical.
- `src/devices.rs`: the entries of a device selector (system default, devices, a chosen device that is not connected), and whether a chosen device is missing. No GTK types, covered by unit tests.
- `src/ui/folder_page.rs` + `src/ui/folder-page.ui`: the content of one tab. It owns the unsorted store of the sounds of its folder and shows it sorted in a `SoundGrid`, or a status page when the folder is empty or missing. It watches the folder (`gio::FileMonitor` with `WATCH_MOVES`): any event schedules a rescan off the main thread 200 ms later, and `diff` of `src/library.rs` says which `Sound` objects to add and remove (the others are kept, so a playing pad keeps playing). The events that carry both paths (a rename, a move between watched folders) move the pad settings first.
- `src/ui/sound_grid.rs` + `src/ui/sound-grid.ui`: the grid of pads over any list model of `Sound`, used by the folder pages, the "Favorites" tab, and the search results. It activates an action with the path of the pad (`app.toggle-sound` unless told otherwise), handles the Menu key (the focus is on the grid cell), and locates a sound (scroll and blink).
- `src/sound.rs`: the `Sound` GObject: path, name (from the file), `playing`, what the user set (`display-name`, `color`, `volume`, `looping`, `favorite`, `shortcut`), the modification time of the file, and the position while it plays (`elapsed` and `duration` in milliseconds, -1 while unknown). Only the application writes the settings and the position. `src/ui/sound_pad.rs` + `src/ui/sound-pad.ui`: the widget of one pad, which follows the properties of its `Sound` and owns the context menu (its `pad.*` actions forward to the `app.*` and `win.*` ones with the path). A favorite pad shows a star, and a pad with a key shows it as a badge (the `keycap` class, shared with the dialog). `blink` highlights the pad for 1.2 seconds (the `located` class, a CSS animation in the text color of the window).
- `src/pads.rs`: the pad settings (`PadSettings`, `PadColor`, `PadStore` with the JSON file), the pad keys (`shortcut_key`, and the rule that a key belongs to one entry, applied by `take_shortcut` and on load), the trigger rule (`TriggerMode`, `trigger`, and `play` for the search), the search match (`matches`), the sort order (`SortOrder`, `compare`), the move of the settings of a folder (`move_folder`), and the time format of a playing pad. No GTK types, covered by unit tests.
- `src/ui/style.css`: custom styling, loaded by libadwaita from the `resource-base-path`. The palette is one class per color (`.pad.blue`, ...) built from the libadwaita palette variables; the dark style overrides sit in a `prefers-color-scheme` media query (`style-dark.css` is deprecated).
- `src/library.rs`: which files of a folder are sounds, what changed between two scans (`diff`), and the import of loose files (`import`: a copy under a hidden temporary name, never overwriting, with `std::fs::copy` so that the copy has a fresh modification time). No GTK types, covered by unit tests.
- `src/lib.rs`: library target that exposes `audio`, `devices`, `library`, and `pads`, so other binaries and the tests can use them. The app modules above stay in `main.rs`.
- `src/audio/`: the audio engine. `mod.rs` is the public API (`AudioEngine`, `Config`, `PlayOptions`, `Position`, `Event`, `Error`, `PlaybackId`, `Device`, `slider_gain`); `graph.rs` owns the PipeWire thread (virtual microphone node, registry, links); `player.rs` builds one GStreamer pipeline per sound. PipeWire objects never leave the engine thread: commands go in through a `pipewire::channel`, events come out through `async-channel`.
- `src/bin/vinheta-audio-test.rs`: diagnostic binary behind the `audio-test` cargo feature (`cargo run --features audio-test --bin vinheta-audio-test -- --help`). Meson does not build or install it.

Audio behavior that is easy to get wrong (details in `docs/audio.md`):

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

GObject conventions used here: each type has a `mod imp` holding the state struct and the subclass `impl`s, plus a public `glib::wrapper!`. UI is declared in XML (`.ui`), not built in code.

When adding files:

- New widgets go in `src/ui/` and are declared in `src/ui/mod.rs`.
- Every new `.ui` file must be listed in `src/vinheta.gresource.xml` (prefix `/io/github/wilfison/Vinheta`) with an `alias` that drops the `ui/` directory, so resource paths stay flat (`/io/github/wilfison/Vinheta/window.ui`) and, if it has translatable strings, in `po/POTFILES.in`. `.rs` files that call `gettext()` must be listed there too.
- `shortcuts-dialog.ui` is loaded automatically by libadwaita from the `resource-base-path`, which provides the `app.shortcuts` action; that is why it does not appear in `setup_gactions()`.

## App ID

The app ID is `io.github.wilfison.Vinheta`. It is spread across file names (`data/`, icons), the gresource prefix, the `resource-base-path`, `main.rs`, the GSettings schema, and the `.desktop`/metainfo/D-Bus service files. Changing the ID means updating all of these consistently.

## Language

UI strings are written in English in the code and marked as translatable (gettext, domain `vinheta`). `po/LINGUAS` lists `pt_BR`, and `po/pt_BR.po` must translate every string: the check "translations are complete" of `scripts/check.sh` fails otherwise.

After adding or changing a string:

```sh
meson compile -C build vinheta-pot            # writes po/vinheta.pot (git-ignored)
msgmerge --update --backup=none po/pt_BR.po po/vinheta.pot
# translate the new and the fuzzy entries of po/pt_BR.po, then:
scripts/check-translations.sh
scripts/screenshot.sh NAME --lang pt_BR ...   # read the result
```

- `xgettext` does not know Rust and reads the `.rs` files as C, with warnings. It has found every call so far; the check compares the number of `gettext(` and `ngettext(` calls of each file with the template.
- Product names ("Vinheta", "PipeWire") are not translated, the `{}` placeholders and the typographic quotes are kept, and an accelerator underscore must sit on a letter that no other item of the same menu uses.
- The desktop file and the metainfo are merged with the translations at build time. After a change of the `.po` alone, touch their `.in` files or the build keeps the old ones.
- The pictures of the metainfo and of the README (`data/screenshots/`) are made by `scripts/metainfo-screenshots.sh`.

## Planning

PRDs live in `tasks/` (git-ignored) and are the progress tracker of a phase. Write them with the project skill `prd-vinheta` (`.agents/skills/prd-vinheta/`), which knows how each kind of story is verified here.

## Writing style

- Never use the em dash character (U+2014) anywhere: code, comments, UI strings, commit messages, or documentation. Use a comma, colon, parentheses, or a separate sentence instead.
- Avoid long comments and comments that state the obvious. Only comment when the reason behind the code is not clear from the code itself, and keep it short.
- Never add `Co-Authored-By` lines or any other AI/tool attribution to commit messages or pull request descriptions.
