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

**Current state:** the code is still the GNOME Builder template ("Hello, World!"). None of the behavior above is implemented: there is no audio dependency in `Cargo.toml`, and the GSettings schema is empty.

## Build and run

Meson is the real build system; it generates the configuration, compiles the resources, and invokes `cargo` underneath.

```sh
meson setup build            # configure (add --prefix=... to choose where it installs)
meson compile -C build       # compiles the gresource + cargo build
meson test -C build          # validates the .desktop file, metainfo (appstreamcli) and GSettings schema
meson test -C build "Validate schema file"   # run a single test
```

There are no Rust tests; the only tests are the validations above, defined in `data/meson.build`.

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
- Building requires `libgtk-4-dev` and `libadwaita-1-dev` (they pull in the GLib dev tools, including `glib-compile-resources`). Installing `libxml2-utils` silences the `xmllint` warning when compiling the gresource.

Non-obvious points:

- `src/config.rs` is **generated** from `src/config.rs.in` by Meson and copied back into `src/`. Edit the `.in`, never the `.rs`. New build-time constants also need a `conf.set_quoted(...)` in `src/meson.build`.
- `cargo build` / `cargo check` / `cargo clippy` work directly for checking the code (as long as `src/config.rs` already exists), but the resulting binary does not run on its own: `main.rs` loads `vinheta.gresource` from `PKGDATADIR` (`/usr/share/vinheta` in the `.deb`) and panics if it is missing. To run the app, install it through Meson (see "Running in development") or install the `.deb`.
- The Flatpak manifest `io.github.wilfison.Vinheta.json` is a leftover from the GNOME Builder template and is only useful for building/running through GNOME Builder. It is not the distribution channel, and its module source (`file:///home/will/Projects`) is not valid for `flatpak-builder`.
- cargo downloads crates during compilation (sources are not vendored), so builds need network access.
- The `gnome_47` (gtk4) and `v1_6` (libadwaita) features in `Cargo.toml` cap the APIs available in the Rust bindings; bump the features to use newer APIs.

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
- `src/application.rs`: `adw::Application` subclass; registers the `app.*` actions in `setup_gactions()`.
- `src/window.rs` + `src/window.ui`: `adw::ApplicationWindow` subclass using a composite template; widgets from the `.ui` file are bound with `#[template_child]`.

GObject conventions used here: each type has a `mod imp` holding the state struct and the subclass `impl`s, plus a public `glib::wrapper!`. UI is declared in XML (`.ui`), not built in code.

When adding files:

- Every new `.ui` file must be listed in `src/vinheta.gresource.xml` (prefix `/io/github/wilfison/Vinheta`) and, if it has translatable strings, in `po/POTFILES.in`. `.rs` files that call `gettext()` must be listed there too (currently only `window.ui` is).
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
