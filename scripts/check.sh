#!/usr/bin/env bash
# Runs the project's checks and prints one line per check.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"

usage() {
    cat >&2 <<'USAGE'
usage: check.sh [--audio] [--app] [--deb] [--all]

Always: rustfmt, clippy without warnings, a single glib version, no em dash,
one version everywhere (scripts/version.sh), complete translations
(scripts/check-translations.sh), and meson test (desktop file, metainfo,
schema, Rust tests).
--audio  also runs scripts/verify-audio.sh (fake devices, about 4 minutes)
--app    also installs the app and runs scripts/verify-app.sh (real PipeWire,
         plays a quiet tone)
--deb    also builds the package with scripts/build-deb.sh, in the background
         while the other checks run
--all    all of the above
USAGE
    exit 2
}

audio= app= deb=
for arg in "$@"; do
    case $arg in
        --audio) audio=1 ;;
        --app) app=1 ;;
        --deb) deb=1 ;;
        --all) audio=1 app=1 deb=1 ;;
        *) usage ;;
    esac
done

cd "$root" || exit 1
mkdir -p tmp/check
failures=0

log_of() { echo "tmp/check/$(echo "$1" | tr -c 'a-z0-9\n' '-').log"; }

# report LABEL STATUS
report() {
    if [ "$2" -eq 0 ]; then
        echo "PASS $1"
    else
        echo "FAIL $1 (see $(log_of "$1"))"
        tail -n 15 "$(log_of "$1")" | sed 's/^/    /'
        failures=$((failures + 1))
    fi
}

# run LABEL COMMAND...: the output is kept in tmp/check and shown on failure.
run() {
    local label=$1
    shift
    "$@" >"$(log_of "$label")" 2>&1
    report "$label" $?
}

# start LABEL COMMAND...: like run, in the background; finish LABEL reports it.
# A background job ignores Ctrl-C, so it gets a process group to be killed
# with when the script ends early.
declare -A started
start() {
    local label=$1
    shift
    setsid "$@" >"$(log_of "$label")" 2>&1 &
    started[$label]=$!
}
finish() {
    wait "${started[$1]}"
    report "$1" $?
    unset "started[$1]"
}
stop_started() {
    local pid
    for pid in "${started[@]}"; do kill -- "-$pid" 2>/dev/null; done
}
trap stop_started EXIT
trap 'exit 130' INT TERM

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

# The package builds from its own copy and plays nothing, so it overlaps the
# rest. At the lowest priority, so it does not disturb the timed audio checks.
[ -n "$deb" ] && start "debian package" nice -n 19 scripts/build-deb.sh

run "rustfmt has nothing to change" formatted
run "clippy without warnings" clippy
run "a single glib version" one_glib
run "no em dash in tracked files" no_em_dash
run "no em dash in new files" untracked_em_dash
run "one version everywhere" scripts/version.sh
run "translations are complete" scripts/check-translations.sh
run "meson test" meson test -C build
[ -n "$audio" ] && run "audio engine harness" scripts/verify-audio.sh
if [ -n "$app" ]; then
    run "install into the local prefix" install_app
    run "app end to end" scripts/verify-app.sh
fi
[ -n "$deb" ] && finish "debian package"

if [ "$failures" -eq 0 ]; then
    echo "ALL CHECKS PASSED"
else
    echo "$failures CHECK(S) FAILED"
    exit 1
fi
