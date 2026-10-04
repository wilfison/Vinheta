#!/usr/bin/env bash
# Rehearses the CI on a developer machine: runs scripts/ci.sh --lintian in a
# clean ubuntu:26.04 container, on a copy of the tree (tracked and untracked
# files, not the ignored ones), then installs the package in a second clean
# container. The package ends in tmp/ci/vinheta/tmp/deb.
set -euo pipefail

. "$(dirname "$0")/dev-common.sh"
require_tools docker git tar

work="$root/tmp/ci"
# The first container writes as root.
[ ! -d "$work" ] || docker run --rm -v "$work:/work" ubuntu:26.04 rm -rf /work/vinheta
mkdir -p "$work"
git clone --quiet "$root" "$work/vinheta"
(cd "$root" && git ls-files -co --exclude-standard -z | tar cf - --null --ignore-failed-read -T - 2>/dev/null) | tar xf - -C "$work/vinheta"
# Files deleted in the working tree, and everything else, as the index.
(cd "$root" && git ls-files -d -z) | (cd "$work/vinheta" && xargs -0 -r rm -f)
git -C "$work/vinheta" add -A

docker run --rm -v "$work/vinheta:/src" -w /src ubuntu:26.04 scripts/ci.sh --lintian

deb=$(cd "$work/vinheta/tmp/deb" && ls vinheta_*.deb)
docker run --rm -v "$work/vinheta/tmp/deb:/deb:ro" ubuntu:26.04 sh -c "
    apt-get update -qq &&
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /deb/$deb >/dev/null &&
    vinheta --help >/dev/null &&
    echo 'the package installs in a clean container and vinheta --help exits with 0'"
echo "$work/vinheta/tmp/deb/$deb"
