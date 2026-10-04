# Building and running

Meson is the real build system; it generates the configuration, compiles the resources, and invokes `cargo` underneath.

```sh
meson setup build            # configure (add --prefix=... to choose where it installs)
meson compile -C build       # compiles the gresource + cargo build
meson test -C build          # validates the .desktop file, metainfo (appstreamcli), GSettings schema, and runs the Rust tests
meson test -C build "Validate schema file"   # run a single test
```

The Rust tests are unit tests of code that needs no GTK, PipeWire, or display (today `src/library.rs` with the scan, the folder diff, and the import, `src/devices.rs`, `src/pads.rs` with the pad file, the pad keys and their uniqueness, the trigger and play rules, the search match, and the sort order, and the pure functions of `src/audio/`: the slider curve and the gain product); run them directly with `cargo test --lib`. The validations are defined in `data/meson.build` and the Rust test in `src/meson.build`. debhelper runs `meson test` while building the package, so tests must not depend on a session. The audio engine is checked by `scripts/verify-audio.sh` (see [audio.md](audio.md)).

## Running in development

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

## Non-obvious points

- `src/config.rs` is **generated** from `src/config.rs.in` by Meson and copied back into `src/`. Edit the `.in`, never the `.rs`. New build-time constants also need a `conf.set_quoted(...)` in `src/meson.build`.
- `cargo build` / `cargo check` / `cargo clippy` work directly for checking the code (as long as `src/config.rs` already exists), but the resulting binary does not run on its own: `main.rs` loads `vinheta.gresource` from `PKGDATADIR` (`/usr/share/vinheta` in the `.deb`) and panics if it is missing. To run the app, install it through Meson (see "Running in development") or install the `.deb`.
- cargo downloads crates during compilation (sources are not vendored), so builds need network access.
- The `gnome_47` (gtk4) and `v1_7` (libadwaita) features in `Cargo.toml` cap the APIs available in the Rust bindings; bump the features to use newer APIs.
- rust-analyzer needs the `rust-src` component. Without it, it reports errors that the compiler does not (`cannot apply unary operator` on `bool` or `i32`, `None` flagged as a variable name). The asdf Rust install does not ship it: download `rust-src-VERSION.tar.xz` from `static.rust-lang.org/dist` and run its `install.sh --prefix=` with the install directory of that Rust version. Trust `cargo clippy` over the editor when they disagree.
- `gtk4`, `libadwaita`, and `gstreamer` must share the same `glib` version. Today that is `glib 0.20` with `gstreamer` 0.23; bumping `gtk4` means bumping `gstreamer` with it. `cargo tree -i glib` must list one version, which `scripts/check.sh` verifies.
- `scripts/dev-common.sh` holds what the development scripts share (the local prefix environment, the virtual session, test sounds). New development scripts should source it from bash (it refuses any other shell, where it would compute the wrong root).

The checks and the tools to look at the interface are in [checks.md](checks.md).
