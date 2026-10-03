#!/usr/bin/env bash
# Checks the installed app end to end, on a virtual display: the virtual
# microphone, playing and stopping through actions and through real clicks, the
# "Send sounds to call" switch measured on a recording, and the cleanup on exit.
# It uses the real PipeWire: a quiet tone (-45 dBFS) plays on the default
# output for a few seconds, and the real microphone is linked as usual.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"

require_tools xvfb-run dbus-run-session gapplication gdbus gsettings xdotool ffmpeg pw-record pw-link pw-dump
require_app

work="$root/tmp/verify-app"
rm -rf "$work"
mkdir -p "$work/Tones"
tone="$work/Tones/tone.wav"
tone_sound "$tone" 60 || exit 1

check() {
    local label=$1
    shift
    if "$@"; then echo "PASS $label"; else echo "FAIL $label"; fi
}

node_exists() { pw-dump | grep -q '"node.name": "vinheta"'; }
call_linked() { pw-link -l | grep -q "vinheta-call-"; }
not() { ! "$@"; }

# Level of the 1000 Hz tone on the virtual microphone, in dBFS, over 2 seconds.
call_level() {
    pw-record --format s16 --rate 48000 --channels 2 --target vinheta "$work/$1.wav" &
    local recorder=$!
    sleep 2
    kill -INT "$recorder"
    wait "$recorder" 2>/dev/null
    tone_level "$work/$1.wav"
}

# at_least A B MARGIN: A is at least MARGIN dB above B.
at_least() { python3 -c 'import sys; a, b, m = map(float, sys.argv[1:]); sys.exit(0 if a - b >= m else 1)' "$@"; }

session() {
    local on off back
    gsettings set "$app_id" directories "$(gvariant_strv "$work/Tones")" || return 1
    start_app || { echo "FAIL the app starts"; return 1; }
    check "the virtual microphone exists while the app runs" node_exists

    echo "== play and stop through actions"
    activate toggle-sound "'$tone'"
    sleep 1
    check "a playing sound is linked to the virtual microphone" call_linked

    echo "== send sounds to call"
    on=$(call_level on)
    gsettings set "$app_id" send-sounds-to-call false
    sleep 0.5
    off=$(call_level off)
    gsettings set "$app_id" send-sounds-to-call true
    sleep 0.5
    back=$(call_level back)
    check "switch off silences the call ($on dBFS on, $off dBFS off)" at_least "$on" "$off" 20
    check "switch on brings the sound back ($back dBFS)" at_least "$back" "$off" 20

    activate stop-all
    sleep 1
    check "stop all removes the call stream" not call_linked

    # The only pad is the first cell of the grid.
    echo "== play and stop through clicks"
    click 129 177
    sleep 1
    check "clicking a pad plays it" call_linked
    click 129 177
    sleep 1
    check "clicking it again stops it" not call_linked

    echo "== exit"
    quit_app
    sleep 0.5
    check "nothing is left after the app quits" not node_exists
    check "the folders were saved" grep -q "directories=.*Tones" "$work/config/glib-2.0/settings/keyfile"
}

{
    echo ". '$root/scripts/dev-common.sh'"
    declare -p work tone
    declare -f check node_exists call_linked not call_level at_least session
    echo session
} >"$work/session.sh"

# The session daemons print to stdout too, so only the result lines are kept.
virtual_session "$work/config" bash "$work/session.sh" 2>"$work/session.log" |
    grep --line-buffered -E "^(PASS|FAIL|==)" | tee "$work/result.txt"

if grep -q "^FAIL" "$work/result.txt" || ! grep -q "^PASS nothing is left" "$work/result.txt"; then
    echo "CHECKS FAILED, see $work/session.log"
    exit 1
fi
echo "ALL CHECKS PASSED"
