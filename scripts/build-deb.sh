#!/usr/bin/env bash
# Builds the .deb from a copy of the tracked and untracked (not ignored) files,
# under tmp/deb, so the working tree and its parent directory stay clean.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"
require_tools dpkg-buildpackage git tar

work="$root/tmp/deb"
rm -rf "$work"
mkdir -p "$work/vinheta"
(cd "$root" && git ls-files -co --exclude-standard -z | tar cf - --null -T -) | tar xf - -C "$work/vinheta"

# debhelper points HOME somewhere else, where asdf finds no .tool-versions and
# its rustc shim fails ("Unknown compiler(s): rustc" in Meson).
if command -v asdf >/dev/null && [ -z "${ASDF_RUST_VERSION:-}" ]; then
    ASDF_RUST_VERSION=$(rustc --version | cut -d' ' -f2)
    export ASDF_RUST_VERSION
fi

if ! (cd "$work/vinheta" && dpkg-buildpackage -us -uc -b) >"$work/build.log" 2>&1; then
    tail -n 30 "$work/build.log" >&2
    echo "the package build failed, see $work/build.log" >&2
    exit 1
fi

deb=$(ls "$work"/vinheta_*.deb)
echo "$deb"
echo "Depends: $(dpkg-deb -f "$deb" Depends)"
