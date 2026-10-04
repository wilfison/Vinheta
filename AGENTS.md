# AGENTS.md

This file provides guidance to AI coding agents working with code in this repository. It holds the rules; the documentation lives in `docs/`.

## What the project is

Vinheta is a soundboard for GNOME (GTK4 + libadwaita, written in Rust, distributed as a `.deb` package for Ubuntu 26.04). The user adds folders, each folder becomes a tab, and every sound of a folder is a pad. A sound plays on the headphones **and** into a virtual microphone named "Vinheta" (for use in calls), which also carries the voice of the user.

## Documentation

Read the document of an area before changing it, and update it in the same change when what it says is no longer true.

| Document | Read it for |
| --- | --- |
| [docs/overview.md](docs/overview.md) | What the app does, the mockups and what differs from them, open items |
| [docs/architecture.md](docs/architecture.md) | The map of the source tree, the crates, the threads, the app ID |
| [docs/application.md](docs/application.md) | `src/application.rs`: actions, pad settings, audio failures, notices |
| [docs/interface.md](docs/interface.md) | `src/ui/`: window, tabs, pads, search, dialogs, device selectors, narrow layout |
| [docs/library.md](docs/library.md) | `src/library.rs` and `src/pads.rs`: folders, the pad file, imports |
| [docs/shortcuts.md](docs/shortcuts.md) | Pad keys, accelerators, and why there are no global shortcuts |
| [docs/audio.md](docs/audio.md) | `src/audio/`: the engine, its rules, the harness, the manual call checklist |
| [docs/development.md](docs/development.md) | Building, running in development, non-obvious points of the build |
| [docs/checks.md](docs/checks.md) | `scripts/check.sh`, `verify-app.sh`, `screenshot.sh` and its options, fixtures, what is tested by hand |
| [docs/packaging.md](docs/packaging.md) | The Debian package, the CI, releasing |
| [docs/translations.md](docs/translations.md) | Adding and translating strings |

## Commands

```sh
scripts/run-dev.sh            # builds, installs into build/install, and runs (--debug for debug messages)
cargo test --lib              # the Rust unit tests
scripts/check.sh              # the checks to run before committing
scripts/check.sh --all        # plus the audio harness, the app check, and the package build
scripts/screenshot.sh NAME …  # the installed app on a virtual display, captured to tmp/screenshots/
```

- Meson is the real build system and invokes `cargo` underneath. `cargo build`, `cargo check`, and `cargo clippy` work for checking the code, but the binary only runs when installed through Meson.
- Trust `cargo clippy` over the editor when they disagree.

## Rules

Before committing:

- Run `scripts/check.sh`. After changing audio code, also run all of `scripts/verify-audio.sh`; after changing what the app does with sounds, all of `scripts/verify-app.sh`. `--only REGEX` is for the work in between.
- `scripts/check.sh --all` takes more than 10 minutes: run it in the background and read `tmp/check/` or its output when it ends.
- Never run `scripts/ci.sh` on a developer machine (it installs packages); `scripts/ci-container.sh` rehearses it.
- Screenshots are temporary: read them, then delete them (`scripts/screenshot.sh --clean`).

Code:

- `src/config.rs` is **generated** from `src/config.rs.in`. Edit the `.in`, never the `.rs`. A new build-time constant also needs a `conf.set_quoted(...)` in `src/meson.build`.
- GObject conventions: each type has a `mod imp` holding the state struct and the subclass `impl`s, plus a public `glib::wrapper!`. UI is declared in XML (`.ui`), not built in code.
- Sounds are started and stopped only through the `app.*` actions, pad settings change only through `update_sound`, and the settings keys are the single source of truth for the mix: the interface binds widgets to keys and never calls the engine for them.
- Logic that needs no GTK goes in the library modules (`src/library.rs`, `src/pads.rs`, `src/devices.rs`) with unit tests. Tests must not depend on a session: debhelper runs them while building the package.
- Audio: never add `object.linger` to the node or the links, never set a gain before the `tee`, and read "Rules that are easy to get wrong" of `docs/audio.md` first.
- A scripted check never touches the user's settings, and one that creates, renames, or deletes files works on a copy of a fixture, never in `tmp/fixtures`. New development scripts are bash and source `scripts/dev-common.sh`.

When adding files:

- New widgets go in `src/ui/` and are declared in `src/ui/mod.rs`.
- Unit tests never sit in the implementation file: it ends with `#[cfg(test)] mod tests;` and the tests live in `tests.rs` of the directory named after the module (`src/pads.rs` and `src/pads/tests.rs`; `src/audio/tests.rs` for `src/audio/mod.rs`). They still see the private items and run with `cargo test --lib`.
- Every new `.ui` file must be listed in `src/vinheta.gresource.xml` (prefix `/io/github/wilfison/Vinheta`) with an `alias` that drops the `ui/` directory, so resource paths stay flat (`/io/github/wilfison/Vinheta/window.ui`) and, if it has translatable strings, in `po/POTFILES.in`. `.rs` files that call `gettext()` must be listed there too.

Things to keep in sync:

- The version lives in four places: `meson.build`, `Cargo.toml` (and `Cargo.lock`), `debian/changelog`, and the `release` of the metainfo.
- The minimum versions in `debian/control` (GTK 4.20, libadwaita 1.8) follow the `Cargo.toml` features and the APIs the `.ui` and `.css` files use. New native libraries go in `Build-Depends`.
- The app ID `io.github.wilfison.Vinheta` is spread across file names, the gresource prefix, the schema, and the desktop files: change all of them or none.
- UI strings are written in English and marked as translatable (gettext, domain `vinheta`), and `po/pt_BR.po` must translate every string: follow `docs/translations.md` after adding or changing one.

## Planning

PRDs live in `tasks/` (git-ignored) and are the progress tracker of a feature. Write them with the project skill `prd-vinheta` (`.agents/skills/prd-vinheta/`), which knows how each kind of story is verified here.

## Writing style

- Never use the em dash character (U+2014) anywhere: code, comments, UI strings, commit messages, or documentation. Use a comma, colon, parentheses, or a separate sentence instead.
- Avoid long comments and comments that state the obvious. Only comment when the reason behind the code is not clear from the code itself, and keep it short.
- Never add `Co-Authored-By` lines or any other AI/tool attribution to commit messages or pull request descriptions.
