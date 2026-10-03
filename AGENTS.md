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

**Current state:** Phase 1 of `ROADMAP.md` is done. The window shows one tab per folder with a grid of pads, plays and stops sounds through the audio engine (`src/audio/`), and has the "Stop all" button and the "Send sounds to call" switch. Everything else in the mockups (volumes, device selection, preferences, per-pad features, search, shortcuts, loose files) belongs to later phases and is not in the window yet.

## Build and run

Meson is the real build system; it generates the configuration, compiles the resources, and invokes `cargo` underneath.

```sh
meson setup build            # configure (add --prefix=... to choose where it installs)
meson compile -C build       # compiles the gresource + cargo build
meson test -C build          # validates the .desktop file, metainfo (appstreamcli), GSettings schema, and runs the Rust tests
meson test -C build "Validate schema file"   # run a single test
```

The Rust tests are unit tests of code that needs no GTK, PipeWire, or display (today `src/library.rs`); run them directly with `cargo test --lib`. The validations are defined in `data/meson.build` and the Rust test in `src/meson.build`. debhelper runs `meson test` while building the package, so tests must not depend on a session. The audio engine is checked by `scripts/verify-audio-poc.sh` (see Architecture).

### Running in development

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

### Checking the interface

`scripts/screenshot.sh` runs the installed app on a virtual display, with its own D-Bus session and its own settings, and captures the window:

```sh
meson install -C build
scripts/screenshot.sh grid --folder DIR --action "toggle-sound '/abs/path/sound.wav'"
```

- It writes `tmp/screenshots/NAME.png`. Screenshots are temporary: read them, then delete them.
- `--folder` fills the library, `--action` activates an `app.*` or `win.*` action before the capture, `--no-audio` makes the engine fail to start, `--light` uses the light style.
- There is no simulated input, so states are reached through settings and actions. Dialogs (the folder chooser) cannot be captured.
- The app still uses the real PipeWire: it creates the real "Vinheta" node for a few seconds and plays on the default output. Use silent files with `toggle-sound`.
- It needs `xvfb-run`, `dbus-run-session`, and ImageMagick (`import`), which are development tools only.

Non-obvious points:

- `src/config.rs` is **generated** from `src/config.rs.in` by Meson and copied back into `src/`. Edit the `.in`, never the `.rs`. New build-time constants also need a `conf.set_quoted(...)` in `src/meson.build`.
- `cargo build` / `cargo check` / `cargo clippy` work directly for checking the code (as long as `src/config.rs` already exists), but the resulting binary does not run on its own: `main.rs` loads `vinheta.gresource` from `PKGDATADIR` (`/usr/share/vinheta` in the `.deb`) and panics if it is missing. To run the app, install it through Meson (see "Running in development") or install the `.deb`.
- The Flatpak manifest `io.github.wilfison.Vinheta.json` is a leftover from the GNOME Builder template and is only useful for building/running through GNOME Builder. It is not the distribution channel, and its module source (`file:///home/will/Projects`) is not valid for `flatpak-builder`.
- cargo downloads crates during compilation (sources are not vendored), so builds need network access.
- The `gnome_47` (gtk4) and `v1_7` (libadwaita) features in `Cargo.toml` cap the APIs available in the Rust bindings; bump the features to use newer APIs.

## Debian package

Packaging lives in `debian/` and uses debhelper with the Meson build system.

```sh
dpkg-buildpackage -us -uc -b   # writes ../vinheta_<version>_<arch>.deb
```

- `debian/rules` forces `--buildtype=release`; debhelper's default (`plain`) would make `src/meson.build` produce an unoptimized debug binary.
- The minimum versions in `debian/control` (GTK 4.16, libadwaita 1.8) come from the `Cargo.toml` features and from `AdwShortcutsDialog` in `shortcuts-dialog.ui`. Keep them in sync when either changes. The package targets Ubuntu 26.04 (`resolute`).
- New native libraries (for example, for audio) must be added to `Build-Depends`; runtime library dependencies are filled in by `${shlibs:Depends}`.
- The version is duplicated in `meson.build`, `Cargo.toml`, and `debian/changelog`; bump all three together.

## Architecture

- `src/main.rs`: sets up gettext, registers the gresource, and starts `VinhetaApplication`.
- `src/application.rs`: `adw::Application` subclass; registers the `app.*` actions in `setup_gactions()`. It owns the `AudioEngine` (started in `startup`, dropped in `shutdown`), consumes its events with `glib::spawn_future_local`, and keeps the map from `PlaybackId` to the `Sound` being played. `app.toggle-sound` (parameter: the absolute path of a sound of the library) and `app.stop-all` are the only way sounds are started and stopped; the pads activate them too.
- `src/window.rs` + `src/window.ui`: `adw::ApplicationWindow` subclass using a composite template; widgets from the `.ui` file are bound with `#[template_child]`. It owns the tabs (`AdwViewStack` with an `AdwInlineViewSwitcher`), the `directories` setting, and the `win.add-folder` and `win.remove-folder` actions.
- `src/folder_page.rs` + `src/folder-page.ui`: the content of one tab. It scans its folder off the main thread and shows the pads in a `GtkGridView`, or a status page when the folder is empty or missing.
- `src/sound.rs`: the `Sound` GObject (path, name, `playing`). `src/sound_pad.rs` + `src/sound-pad.ui`: the widget of one pad.
- `src/style.css`: custom styling, loaded by libadwaita from the `resource-base-path`.
- `src/library.rs`: which files of a folder are sounds, and in what order. No GTK types, covered by unit tests.
- `src/lib.rs`: library target that exposes `audio` and `library`, so other binaries and the tests can use them. The app modules above stay in `main.rs`.
- `src/audio/`: the audio engine. `mod.rs` is the public API (`AudioEngine`, `Config`, `Event`, `Error`, `PlaybackId`); `graph.rs` owns the PipeWire thread (virtual microphone node, registry, links); `player.rs` builds one GStreamer pipeline per sound. PipeWire objects never leave the engine thread: commands go in through a `pipewire::channel`, events come out through `async-channel`.
- `src/bin/vinheta-audio-poc.rs`: diagnostic binary behind the `audio-poc` cargo feature (`cargo run --features audio-poc --bin vinheta-audio-poc -- --help`). Meson does not build or install it.

Audio behavior that is easy to get wrong (details in `docs/audio-poc.md`):

- WirePlumber does not route playback into the virtual microphone. The call branch sink uses `node.autoconnect=false` and `graph.rs` links any stream whose node name starts with `vinheta-call-`.
- The node and links are created without `object.linger`, so they vanish when the process ends. Do not add it.
- A playback that was stopped on request never reports an event afterwards; the interface relies on that, and on events carrying the `PlaybackId`.
- "Send sounds to call" mutes the `call-volume` element of each pipeline. The call stream and its links stay in place.
- After changing audio code, run `scripts/verify-audio-poc.sh rust`. It uses fake devices only (no real microphone or headphones) and needs `ffmpeg` and `python3`.

GObject conventions used here: each type has a `mod imp` holding the state struct and the subclass `impl`s, plus a public `glib::wrapper!`. UI is declared in XML (`.ui`), not built in code.

When adding files:

- Every new `.ui` file must be listed in `src/vinheta.gresource.xml` (prefix `/io/github/wilfison/Vinheta`) and, if it has translatable strings, in `po/POTFILES.in`. `.rs` files that call `gettext()` must be listed there too.
- `shortcuts-dialog.ui` is loaded automatically by libadwaita from the `resource-base-path`, which provides the `app.shortcuts` action; that is why it does not appear in `setup_gactions()`.
- The primary menu references `app.preferences`, which does not exist yet.

## App ID

The app ID is `io.github.wilfison.Vinheta`. It is spread across file names (`data/`, the manifest, icons), the gresource prefix, the `resource-base-path`, `main.rs`, the GSettings schema, and the `.desktop`/metainfo/D-Bus service files. Changing the ID means updating all of these consistently.

## Language

UI strings are written in English in the code and marked as translatable (gettext, domain `vinheta`); `po/LINGUAS` has no languages yet.

## Writing style

- Never use the em dash character (U+2014) anywhere: code, comments, UI strings, commit messages, or documentation. Use a comma, colon, parentheses, or a separate sentence instead.
- Avoid long comments and comments that state the obvious. Only comment when the reason behind the code is not clear from the code itself, and keep it short.
- Never add `Co-Authored-By` lines or any other AI/tool attribution to commit messages or pull request descriptions.
