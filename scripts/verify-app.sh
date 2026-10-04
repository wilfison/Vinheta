#!/usr/bin/env bash
# Checks the installed app end to end, on a virtual display: the virtual
# microphone, playing and stopping through actions and through real clicks, the
# "Send sounds to call" switch and the call volume measured on a recording, the
# voice switch, the monitor output, the pad settings (volume, loop), the
# trigger modes, and the cleanup on exit.
# It uses the real PipeWire: a quiet tone (-45 dBFS) plays on the default
# output for a few seconds, and the real microphone is linked as usual.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"
. "$root/scripts/audio-poc-common.sh"

require_tools xvfb-run dbus-run-session gapplication gdbus gsettings xdotool ffmpeg pw-record pw-link pw-dump
require_app

work="$root/tmp/verify-app"
rm -rf "$work"
mkdir -p "$work/Tones"
tone="$work/Tones/tone.wav"
tone_sound "$tone" 60 || exit 1
# The same tone with a pad volume, and a short one that loops. The pad
# settings go into the data directory of the session.
quiet="$work/Tones/tone-quiet.wav"
looped="$work/Tones/tone-loop.wav"
cp "$tone" "$quiet"
tone_sound "$looped" 2 || exit 1
pads="$work/config/data/vinheta/pads.json"
mkdir -p "$(dirname "$pads")"
cat >"$pads" <<JSON
{"version": 1, "pads": {"$quiet": {"volume": 0.5}, "$looped": {"loop": true}}}
JSON
test_sink=vinheta-app-test-sink

check() {
    local label=$1
    shift
    if "$@"; then echo "PASS $label"; else echo "FAIL $label"; fi
}

node_exists() { pw-dump | grep -q '"node.name": "vinheta"'; }
call_linked() { pw-link -l | grep -q "vinheta-call-"; }
not() { ! "$@"; }
call_streams() { pw-dump | grep -c '"node.name": "vinheta-call-'; }
one_call_stream() { [ "$(call_streams)" -eq 1 ]; }
# within SECONDS COMMAND...: the command succeeds before the time is over.
within() {
    local i tries=$(($1 * 10))
    shift
    for i in $(seq "$tries"); do
        "$@" && return 0
        sleep 0.1
    done
    return 1
}
# pad_field PATH FIELD: the value stored for a pad, or nothing.
pad_field() {
    python3 -c 'import json, sys
pad = json.load(open(sys.argv[1]))["pads"].get(sys.argv[2])
print("no entry" if pad is None else json.dumps(pad.get(sys.argv[3])))' "$pads" "$1" "$2"
}
loop_saved() { [ "$(pad_field "$tone" loop)" = true ]; }
entry_removed() { [ "$(pad_field "$quiet" volume)" = "no entry" ]; }

# Level of the 1000 Hz tone on the virtual microphone, in dBFS, over 2 seconds.
call_level() {
    pw-record --format s16 --rate 48000 --channels 2 --target vinheta "$work/$1.wav" &
    local recorder=$!
    sleep 2
    kill -INT "$recorder"
    wait "$recorder" 2>/dev/null
    tone_level "$work/$1.wav"
}

# Links into the virtual microphone that do not come from a sound.
voice_links() { pw-link -l | awk '/^[^ ]/ { own = /^vinheta:input_/ } own && /\|<-/ && !/vinheta-call-/'; }
no_voice_links() { [ -z "$(voice_links)" ]; }
# monitor_on NODE: the monitor stream of the playing sound is linked to NODE.
monitor_on() { pw-link -l | awk '/^[^ ]/ { own = /^vinheta-monitor-/ } own && /\|->/' | grep -q " $1:"; }
monitor_linked() { monitor_on '[^ ]*'; }
# falls_by A B LOW HIGH: B is between LOW and HIGH dB below A.
falls_by() { python3 -c 'import sys; a, b, low, high = map(float, sys.argv[1:]); sys.exit(0 if low <= a - b <= high else 1)' "$@"; }

# at_least A B MARGIN: A is at least MARGIN dB above B.
at_least() { python3 -c 'import sys; a, b, m = map(float, sys.argv[1:]); sys.exit(0 if a - b >= m else 1)' "$@"; }

session() {
    local on off back half sink had_voice full
    gsettings set "$app_id" directories "$(gvariant_strv "$work/Tones")" || return 1
    start_app || { echo "FAIL the app starts"; return 1; }
    check "the virtual microphone exists while the app runs" node_exists

    echo "== play and stop through actions"
    activate toggle-sound "'$tone'"
    sleep 1
    check "a playing sound is linked to the virtual microphone" call_linked

    # The levels are measured with the voice off, so the noise of the real
    # microphone cannot hide the quiet tone.
    echo "== include my voice"
    had_voice=$(voice_links)
    gsettings set "$app_id" include-my-voice false
    sleep 1
    check "voice off leaves only sounds linked to the virtual microphone" no_voice_links

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

    # The slider curve is cubic: half the slider is 18 dB quieter.
    echo "== call volume"
    gsettings set "$app_id" call-volume 0.5
    sleep 0.5
    half=$(call_level half)
    gsettings set "$app_id" call-volume 1.0
    check "call volume 0.5 is 18 dB quieter ($back dBFS at 1.0, $half dBFS at 0.5)" falls_by "$back" "$half" 15 21

    gsettings set "$app_id" include-my-voice true
    sleep 1
    if [ -n "$had_voice" ]; then
        check "voice on links the microphone again" not no_voice_links
    else
        echo "== skipped: this machine has no microphone to link"
    fi

    echo "== monitor output"
    sink=$(create_node "$test_sink" "Vinheta app test sink" Audio/Sink "FL FR")
    gsettings set "$app_id" monitor-output "'$test_sink'"
    sleep 1.5
    check "choosing an output moves the playing sound" monitor_on "$test_sink"
    gsettings set "$app_id" monitor-output "''"
    sleep 1.5
    check "the system default moves it back" not monitor_on "$test_sink"

    # The chosen output does not exist when the sound starts.
    pw-cli destroy "$sink" >/dev/null
    activate stop-all
    gsettings set "$app_id" monitor-output "'$test_sink'"
    sleep 1
    activate toggle-sound "'$tone'"
    sleep 1.5
    check "a missing output falls back to another one" monitor_linked
    sink=$(create_node "$test_sink" "Vinheta app test sink" Audio/Sink "FL FR")
    sleep 1.5
    check "the chosen output is used when it comes back" monitor_on "$test_sink"
    gsettings set "$app_id" monitor-output "''"
    pw-cli destroy "$sink" >/dev/null

    activate stop-all
    sleep 1
    check "stop all removes the call stream" not call_linked

    # The curve of the pad volume is the one of the sliders. Voice off, as
    # for the other levels.
    echo "== pad volume"
    gsettings set "$app_id" include-my-voice false
    activate toggle-sound "'$tone'"
    sleep 1
    full=$(call_level pad-full)
    activate stop-all
    activate toggle-sound "'$quiet'"
    sleep 1
    half=$(call_level pad-half)
    activate stop-all
    gsettings set "$app_id" include-my-voice true
    check "pad volume 0.5 is 18 dB quieter ($full dBFS at 1.0, $half dBFS at 0.5)" falls_by "$full" "$half" 15 21

    # The file lasts 2 seconds.
    echo "== loop"
    sleep 1
    activate toggle-sound "'$looped'"
    sleep 5
    check "a looping pad still plays after 5 s" call_linked
    activate toggle-loop "'$looped'"
    check "it ends within 4 s once the loop is off" within 4 not call_linked

    echo "== trigger modes"
    gsettings set "$app_id" trigger-mode "'restart'"
    activate toggle-sound "'$tone'"
    sleep 1
    activate toggle-sound "'$tone'"
    sleep 1
    check "restart: a second trigger keeps the sound playing" one_call_stream
    activate stop-sound "'$tone'"
    sleep 1
    check "restart: stop-sound stops it" not call_linked
    gsettings set "$app_id" trigger-mode "'stop-others'"
    activate toggle-sound "'$tone'"
    sleep 0.5
    activate toggle-sound "'$quiet'"
    sleep 1.5
    check "stop others: one sound is left after starting a second one" one_call_stream
    activate stop-all
    gsettings set "$app_id" trigger-mode "'overlap'"
    sleep 1

    # Written when the app quits, checked below.
    activate toggle-loop "'$tone'"
    activate reset-sound "'$quiet'"

    # The first pad is the first cell of the grid.
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
    check "a loop set through an action was saved" loop_saved
    check "a reset pad has no entry in the file" entry_removed
}

{
    echo ". '$root/scripts/dev-common.sh'"
    echo ". '$root/scripts/audio-poc-common.sh'"
    declare -p work tone quiet looped pads test_sink
    declare -f check node_exists call_linked not call_level at_least voice_links no_voice_links \
        monitor_on monitor_linked falls_by call_streams one_call_stream within pad_field \
        loop_saved entry_removed session
    echo session
} >"$work/session.sh"

# The session daemons print to stdout too, so only the result lines are kept.
virtual_session "$work/config" bash "$work/session.sh" 2>"$work/session.log" |
    grep --line-buffered -E "^(PASS|FAIL|==)" | tee "$work/result.txt"

# In case the session ended before it removed its fake sink.
leftover=$(node_id "$test_sink")
[ -n "$leftover" ] && pw-cli destroy "$leftover" >/dev/null

if grep -q "^FAIL" "$work/result.txt" || ! grep -q "^PASS nothing is left" "$work/result.txt"; then
    echo "CHECKS FAILED, see $work/session.log"
    exit 1
fi
echo "ALL CHECKS PASSED"
