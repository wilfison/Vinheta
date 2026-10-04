# Architecture

A map of the source tree. Each area has its own document.

| Path | What it is | Document |
| --- | --- | --- |
| `src/main.rs` | Sets up gettext, registers the gresource, and starts `VinhetaApplication` | [application.md](application.md) |
| `src/application.rs` | The `adw::Application`: actions, the audio engine, the pad settings, notices | [application.md](application.md) |
| `src/sound.rs` | The `Sound` GObject, one audio file of the library | [interface.md](interface.md) |
| `src/ui/` | The widgets, their `.ui` templates, and the CSS | [interface.md](interface.md), [shortcuts.md](shortcuts.md) |
| `src/library.rs` | Folder scan, diff, pad names, import of loose files | [library.md](library.md) |
| `src/pads.rs` | Pad settings and their file, keys, trigger rule, search match, sort order | [library.md](library.md) |
| `src/devices.rs` | The entries of a device selector | [interface.md](interface.md) |
| `src/editors.rs` | Which installed app opens a sound for editing | [application.md](application.md) |
| `src/audio/` | The audio engine (PipeWire and GStreamer) | [audio.md](audio.md) |
| `src/bin/vinheta-audio-test.rs` | Diagnostic binary of the engine | [audio.md](audio.md) |
| `data/` | Desktop file, metainfo, GSettings schema, D-Bus service, icons | [packaging.md](packaging.md) |
| `po/` | Translations | [translations.md](translations.md) |
| `debian/`, `.github/` | The package and its publication | [packaging.md](packaging.md) |
| `scripts/` | Development, check, and release scripts | [development.md](development.md), [checks.md](checks.md) |

## Two crates in one package

- `src/lib.rs`: library target that exposes `audio`, `devices`, `editors`, `library`, and `pads`, so other binaries and the tests can use them. The app modules above stay in `main.rs`.
- The modules of the library have no GTK types and are covered by unit tests. The tests of a module live in a `tests.rs` file of the directory named after it (`src/pads.rs` and `src/pads/tests.rs`).

## Threads

- The GTK main thread runs the application and the interface. Starting a sound takes about 1 to 3 ms there, so it is not moved elsewhere.
- The PipeWire loop of the engine runs on its own thread. PipeWire objects never leave it: commands go in through a `pipewire::channel`, events come out through `async-channel`, and the application consumes them with `glib::spawn_future_local`.
- Folder scans and file imports run off the main thread (`gio::spawn_blocking`).

## GObject conventions

GObject conventions used here: each type has a `mod imp` holding the state struct and the subclass `impl`s, plus a public `glib::wrapper!`. UI is declared in XML (`.ui`), not built in code.

## App ID

The app ID is `io.github.wilfison.Vinheta`. It is spread across file names (`data/`, icons), the gresource prefix, the `resource-base-path`, `main.rs`, the GSettings schema, and the `.desktop`/metainfo/D-Bus service files. Changing the ID means updating all of these consistently.
