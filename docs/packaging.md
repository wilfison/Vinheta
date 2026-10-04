# Packaging and releasing

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
- `Depends` lists `gstreamer1.0-pipewire`, `gstreamer1.0-plugins-base`, and `gstreamer1.0-plugins-good` by hand: the sink, the decoders, and the converters are plugins, which `${shlibs:Depends}` does not see.

## Continuous integration

- `scripts/ci.sh` is what GitHub Actions runs (`.github/workflows/ci.yml` on pushes to `main` and pull requests, on the `ubuntu-26.04` runner): it installs the build dependencies with `apt-get build-dep ./`, uses the Rust of the distribution (`/usr/bin` first in `PATH`), and runs `scripts/check.sh --deb`. It installs packages, so do not run it on a developer machine: `scripts/ci-container.sh` rehearses it in a clean `ubuntu:26.04` container (Docker) on a copy of the tree, with `lintian`, and installs the package in a second clean container. The audio and app checks need a PipeWire session and a display, so they never run on the CI.

## Releasing

A release is a tag: `.github/workflows/release.yml` runs on a pushed `v*` tag (or by hand, with the tag as input), checks the tag against the version, runs the CI gate again, and creates the GitHub release with the `.deb`, its `.sha256`, and notes made by `scripts/release-notes.sh` from `CHANGELOG.md`. A second run for the same tag updates the release.

1. Bump the version in the four places, with the same date in `debian/changelog` and in the metainfo, and add a `## [VERSION]` section to `CHANGELOG.md`.
2. Run `scripts/check.sh --all` (the CI does not run the audio and app checks) and, when the packaging changed, `scripts/ci-container.sh`.
3. Commit, push `main`, wait for the CI, then `git tag -a vVERSION -m "Release VERSION" && git push origin vVERSION`.

The project skill `release` (`.agents/skills/release/`) drives these steps, and its `scripts/bump.sh {major|minor|patch}` writes the new version to the four places.

There is no PPA: the `.deb` of a GitHub release is the only channel.
