#!/usr/bin/env bash
# What the CI runs, on a GitHub runner or in a container of Ubuntu 26.04:
# installs the build dependencies and runs the gate, scripts/check.sh --deb.
# It installs packages, so it is not meant for a developer machine: use
# scripts/ci-container.sh there.
# --lintian also runs lintian on the package and fails on an error.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

lintian=
[ "${1:-}" = --lintian ] && lintian=lintian

sudo=
[ "$(id -u)" -eq 0 ] || sudo=sudo
export DEBIAN_FRONTEND=noninteractive
$sudo apt-get update
# shellcheck disable=SC2086
$sudo apt-get install -y git ca-certificates rustfmt rust-clippy $lintian
$sudo apt-get build-dep -y ./

# As root in a container the checkout belongs to another user, and git (used
# by the em dash checks and by the package build) refuses it.
[ "$(id -u)" -ne 0 ] || git config --global --add safe.directory "$root"

# The Rust of the distribution, also where the image ships a newer one: the
# same compiler a user who rebuilds the package gets.
export PATH="/usr/bin:$PATH"
cargo --version

[ -d build ] || meson setup build --prefix="$root/build/install"
scripts/check.sh --deb

deb=$(ls tmp/deb/vinheta_*.deb)
echo "built $deb"
if [ -n "$lintian" ]; then
    lintian "$deb" | tee tmp/deb/lintian.log
    ! grep -q '^E:' tmp/deb/lintian.log
fi
