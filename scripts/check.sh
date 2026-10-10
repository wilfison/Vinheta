#!/usr/bin/env bash
# Runs the project's checks and prints one line per check.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"

usage() {
    cat >&2 <<'USAGE'
usage: check.sh [--audio] [--app] [--deb] [--all] [--force] [--only REGEX]

Always: rustfmt, clippy without warnings, a single glib version, no em dash,
one version everywhere (scripts/version.sh), complete translations
(scripts/check-translations.sh), and meson test (desktop file, metainfo,
schema, Rust tests).
--audio  also runs scripts/verify-audio.sh (fake devices, about 4 minutes),
         unless nothing it tests changed since it last passed here
--app    also installs the app and runs scripts/verify-app.sh (real PipeWire,
         plays a quiet tone)
--deb    also builds the package with scripts/build-deb.sh, last
--all    all of the above
--force  runs the audio harness even when nothing it tests changed
--only   runs only the checks whose label matches REGEX (one step of the CI
         each), and fails when none does
USAGE
    exit 2
}

audio= app= deb= force= only=
while [ $# -gt 0 ]; do
    case $1 in
        --audio) audio=1 ;;
        --app) app=1 ;;
        --deb) deb=1 ;;
        --all) audio=1 app=1 deb=1 ;;
        --force) force=1 ;;
        --only) [ $# -ge 2 ] || usage; only=$2; shift ;;
        *) usage ;;
    esac
    shift
done

cd "$root" || exit 1
mkdir -p tmp/check
failures=0
matched=0

log_of() { echo "tmp/check/$(echo "$1" | tr -c 'a-z0-9\n' '-').log"; }

# wanted LABEL: whether --only selects the check, counted for the final test.
wanted() {
    [ -z "$only" ] || [[ $1 =~ $only ]] || return 1
    matched=$((matched + 1))
}

# report LABEL STATUS, and returns STATUS.
report() {
    if [ "$2" -eq 0 ]; then
        echo "PASS $1"
    else
        echo "FAIL $1 (see $(log_of "$1"))"
        tail -n 15 "$(log_of "$1")" | sed 's/^/    /'
        failures=$((failures + 1))
    fi
    return "$2"
}

# run LABEL COMMAND...: the output is kept in tmp/check and shown on failure.
# The command runs in the background and is waited for, so that a TERM or a
# Ctrl-C stops it at once instead of waiting for it to end.
current=
run() {
    local label=$1 status
    shift
    wanted "$label" || return 0
    "$@" >"$(log_of "$label")" 2>&1 &
    current=$!
    wait "$current"
    status=$?
    current=
    report "$label" "$status"
}

# A command started with setsid has a process group of its own, stopped as a
# whole; the others get the TERM alone.
stop_current() {
    [ -n "$current" ] || return 0
    kill -TERM -- "-$current" 2>/dev/null || kill -TERM "$current" 2>/dev/null
    wait "$current"
}
trap stop_current EXIT
trap 'exit 130' INT TERM

# What the result of the audio harness depends on: the engine and its test
# binary (which use nothing else of the crate), the harness, the dependencies,
# the compiler, and the audio stack of the system.
audio_inputs() {
    git ls-files -co --exclude-standard -- src/audio src/bin/vinheta-audio-test.rs \
        Cargo.toml Cargo.lock scripts/verify-audio.sh scripts/audio-common.sh |
        while read -r file; do [ -f "$file" ] && sha256sum "$file"; done
    rustc --version
    pipewire --version
    wireplumber --version
    gst-inspect-1.0 --version
}

# The harness takes minutes and most changes do not touch what it tests, so
# it is skipped when those inputs are the ones of its last pass.
audio_harness() {
    local passed=tmp/check/audio-harness.passed fingerprint
    wanted "audio engine harness" || return 0
    fingerprint=$(audio_inputs 2>&1 | sha256sum | cut -d' ' -f1)
    if [ -z "$force" ] && [ "$(cat "$passed" 2>/dev/null)" = "$fingerprint" ]; then
        echo "SKIP audio engine harness (nothing it tests changed since it passed, --force runs it)"
        return
    fi
    rm -f "$passed"
    run "audio engine harness" scripts/verify-audio.sh && echo "$fingerprint" >"$passed"
}

formatted() { cargo fmt --check; }
clippy() { cargo clippy --all-targets --features audio-test -- -D warnings; }
one_glib() { [ "$(cargo tree -i glib --depth 0 | grep -c '^glib ')" -eq 1 ]; }
# The mockups are drawings, not text of the project.
no_em_dash() { ! git grep -nIP '\x{2014}' -- . ':!mockups'; }
# The status of the pipeline cannot be used: with no untracked files xargs
# runs nothing and succeeds, like grep does when it finds the character.
untracked_em_dash() {
    local found
    found=$(git ls-files -o --exclude-standard -z | xargs -0 -r grep -nHIP '\x{2014}')
    [ -z "$found" ] || { echo "$found"; return 1; }
}

run "rustfmt has nothing to change" formatted
run "clippy without warnings" clippy
run "a single glib version" one_glib
run "no em dash in tracked files" no_em_dash
run "no em dash in new files" untracked_em_dash
run "one version everywhere" scripts/version.sh
run "translations are complete" scripts/check-translations.sh
run "meson test" meson test -C build
[ -n "$audio" ] && audio_harness
if [ -n "$app" ]; then
    run "install into the local prefix" install_app
    run "app end to end" scripts/verify-app.sh
fi
# Last, after every check that uses PipeWire: next to the build, the graph
# stopped scheduling the nodes of the harness and of the app check three
# times (2026-10-10), even at the lowest priority.
[ -n "$deb" ] && run "debian package" setsid scripts/build-deb.sh

if [ -n "$only" ] && [ "$matched" -eq 0 ]; then
    echo "no check matches --only $only" >&2
    exit 2
fi
if [ "$failures" -eq 0 ]; then
    echo "ALL CHECKS PASSED"
else
    echo "$failures CHECK(S) FAILED"
    exit 1
fi
