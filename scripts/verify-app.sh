#!/usr/bin/env bash
# Checks the installed app end to end, on a virtual display: playing and
# stopping through actions and through real clicks, the "Send sounds to call"
# switch and the call volume measured on what a fake call app records, the
# app the sounds are sent to, the monitor output, the pad settings (volume,
# loop), the trigger modes, files removed and renamed while the app runs, a
# pad key, audio coming back after "Try Again", and the cleanup on exit.
# It uses the real PipeWire: a quiet tone (-45 dBFS) plays on the default
# output for a few seconds. The sounds are sent to the fake call app only,
# which records a fake microphone, so no real app and no real microphone
# takes part.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"
. "$root/scripts/audio-common.sh"

# --only REGEX runs the sections whose title matches (the start and the exit
# of the app are always checked); --list prints the titles.
only=
case ${1:-} in
    --list)
        sed -n 's/^ *if section "\(.*\)"; then$/\1/p' "$0"
        exit 0
        ;;
    --only)
        [ $# -eq 2 ] || { echo "usage: verify-app.sh [--only REGEX | --list]" >&2; exit 2; }
        only=$2
        ;;
    "") ;;
    *) echo "usage: verify-app.sh [--only REGEX | --list]" >&2; exit 2 ;;
esac

require_tools xvfb-run dbus-run-session gapplication gdbus gsettings xdotool ffmpeg pw-record pw-link pw-dump pipewire
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
# One that is deleted while it plays, and one that is renamed.
gone="$work/Tones/tone-gone.wav"
kept="$work/Tones/tone-kept.wav"
moved="$work/Tones/tone-moved.wav"
cp "$tone" "$kept"
pads="$work/config/data/vinheta/pads.json"
mkdir -p "$(dirname "$pads")"
cat >"$pads" <<JSON
{"version": 1, "pads": {"$tone": {"shortcut": "q"}, "$quiet": {"volume": 0.5}, "$looped": {"loop": true}, "$kept": {"color": "red"}}}
JSON
test_sink=vinheta-app-test-sink
test_mic=vinheta-app-test-mic
# The fake call apps, as the app names them.
test_app=vinheta-app-test-call
other_app=vinheta-app-test-other

check() {
    local label=$1
    shift
    if "$@"; then echo "PASS $label"; else echo "FAIL $label"; fi
}

# The node of the engine, and a call stream (linked to that node at least).
node_exists() { pw-dump | grep -q '"node.name": "vinheta-drain-'; }
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
settings_moved() { [ "$(pad_field "$moved" color)" = '"red"' ] && [ "$(pad_field "$kept" color)" = "no entry" ]; }

# call_app APP FILE: a recorder of the fake microphone, as a call app is
# of the real one. Its process id is left in $recorder.
call_app() {
    pw-record --format s16 --rate 48000 --channels 2 --target "$test_mic" \
        -P "{ state.restore-props=false node.name=$1 application.name=\"Vinheta test app $1\" application.process.binary=$1 }" "$2" &
    recorder=$!
}
# Level of the 1000 Hz tone in what the fake call app records, in dBFS,
# over 2 seconds. The fake microphone is silent.
call_level() {
    local recorder
    call_app "$test_app" "$work/$1.wav"
    sleep 2
    kill -INT "$recorder"
    wait "$recorder" 2>/dev/null
    tone_level "$work/$1.wav"
}
# sent_to APP: a sound is linked into the recorder with that name.
sent_to() { pw-link -l | awk -v app="$1" '/^[^ ]/ { own = index($0, app ":input_") == 1 } own && /\|<- vinheta-call-/' | grep -q .; }
# monitor_on NODE: the monitor stream of the playing sound is linked to NODE.
monitor_on() { pw-link -l | awk '/^[^ ]/ { own = /^vinheta-monitor-/ } own && /\|->/' | grep -q " $1:"; }
monitor_linked() { monitor_on '[^ ]*'; }
# falls_by A B LOW HIGH: B is between LOW and HIGH dB below A.
falls_by() { python3 -c 'import sys; a, b, low, high = map(float, sys.argv[1:]); sys.exit(0 if low <= a - b <= high else 1)' "$@"; }

# at_least A B MARGIN: A is at least MARGIN dB above B.
at_least() { python3 -c 'import sys; a, b, m = map(float, sys.argv[1:]); sys.exit(0 if a - b >= m else 1)' "$@"; }

# section TITLE: says whether the section runs (see --only), and prints its
# title. Every section starts and stops what it needs, so any of them can run
# alone.
section() {
    if [ -n "$only" ] && ! [[ $1 =~ $only ]]; then
        return 1
    fi
    echo "== $1"
}

session() {
    local on off back half full sink mic recorder first second private private_pid saved=
    gsettings set "$app_id" directories "$(gvariant_strv "$work/Tones")" || return 1
    # With every app as the target, the tone would reach the real apps that
    # are recording.
    gsettings set "$app_id" call-target "'$test_app'" || return 1
    mic=$(create_node "$test_mic" "Vinheta app test microphone" Audio/Source/Virtual MONO)
    # The checks are not a first run: the call setup guide would be in the way.
    gsettings set "$app_id" call-guide-shown true || return 1
    start_app || { echo "FAIL the app starts"; return 1; }
    check "the node of the engine exists while the app runs" node_exists

    if section "play and stop through actions"; then
        # The first playback of a run can take longer to link.
        activate toggle-sound "'$tone'"
        check "a playing sound has a linked call stream" within 3 call_linked
        activate stop-all
        sleep 1
        check "stop all removes the call stream" not call_linked
    fi

    if section "send sounds to call"; then
        activate toggle-sound "'$tone'"
        sleep 1
        on=$(call_level on)
        gsettings set "$app_id" send-sounds-to-call false
        sleep 0.5
        off=$(call_level off)
        gsettings set "$app_id" send-sounds-to-call true
        sleep 0.5
        back=$(call_level back)
        activate stop-all
        check "switch off silences the call ($on dBFS on, $off dBFS off)" at_least "$on" "$off" 20
        check "switch on brings the sound back ($back dBFS)" at_least "$back" "$off" 20
    fi

    # The slider curve is cubic: half the slider is 18 dB quieter.
    if section "call volume"; then
        activate toggle-sound "'$tone'"
        sleep 1
        full=$(call_level call-full)
        gsettings set "$app_id" call-volume 0.5
        sleep 0.5
        half=$(call_level call-half)
        gsettings set "$app_id" call-volume 1.0
        activate stop-all
        check "call volume 0.5 is 18 dB quieter ($full dBFS at 1.0, $half dBFS at 0.5)" falls_by "$full" "$half" 15 21
    fi

    # Every app as the target would reach the real ones, so that case is
    # checked on the links alone, with the call volume at 0.
    if section "call target"; then
        call_app "$test_app" /dev/null
        first=$recorder
        call_app "$other_app" /dev/null
        second=$recorder
        activate toggle-sound "'$tone'"
        sleep 1.5
        check "the chosen app gets the sound" sent_to "$test_app"
        check "another app does not" not sent_to "$other_app"
        gsettings set "$app_id" call-target "'$other_app'"
        sleep 1
        check "choosing another app moves a playing sound" sent_to "$other_app"
        check "the first app no longer gets it" not sent_to "$test_app"
        gsettings set "$app_id" call-volume 0.0
        gsettings set "$app_id" call-target "''"
        sleep 1
        check "every app gets the sound without a chosen one" eval "sent_to '$test_app' && sent_to '$other_app'"
        gsettings set "$app_id" call-target "'$test_app'"
        sleep 0.5
        gsettings set "$app_id" call-volume 1.0
        kill -INT "$second"
        wait "$second" 2>/dev/null
        sleep 1
        check "the sound keeps playing after an app stops recording" eval "call_linked && sent_to '$test_app'"
        kill -INT "$first"
        wait "$first" 2>/dev/null
        activate stop-all
        sleep 1
    fi

    if section "monitor output"; then
        activate toggle-sound "'$tone'"
        sleep 1
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
    fi

    # The curve of the pad volume is the one of the sliders.
    if section "pad volume"; then
        activate toggle-sound "'$tone'"
        sleep 1
        full=$(call_level pad-full)
        activate stop-all
        activate toggle-sound "'$quiet'"
        sleep 1
        half=$(call_level pad-half)
        activate stop-all
        check "pad volume 0.5 is 18 dB quieter ($full dBFS at 1.0, $half dBFS at 0.5)" falls_by "$full" "$half" 15 21
        sleep 1
    fi

    # The file lasts 2 seconds.
    if section "loop"; then
        activate toggle-sound "'$looped'"
        sleep 5
        check "a looping pad still plays after 5 s" call_linked
        activate toggle-loop "'$looped'"
        check "it ends within 4 s once the loop is off" within 4 not call_linked
        activate toggle-loop "'$looped'"
    fi

    if section "trigger modes"; then
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
    fi

    # The folder is watched: the new file is known without a restart.
    if section "files that go away"; then
        cp "$tone" "$gone"
        sleep 1
        activate toggle-sound "'$gone'"
        sleep 1
        check "a file added while the app runs plays" call_linked
        rm "$gone"
        check "removed file stops" within 2 not call_linked
    fi

    # Written when the app quits, checked after the exit.
    if section "saved pad settings"; then
        mv "$kept" "$moved"
        sleep 1
        activate toggle-loop "'$tone'"
        activate reset-sound "'$quiet'"
        saved=1
    fi

    # The first pad is the first cell of the grid.
    if section "play and stop through clicks"; then
        click 129 177
        sleep 1
        check "clicking a pad plays it" call_linked
        click 129 177
        sleep 1
        check "clicking it again stops it" not call_linked
    fi

    # A real key press; the window gets the keys once the pointer is over it.
    if section "pad key"; then
        xdotool mousemove 500 400
        key q
        sleep 1
        on=$(call_level key-on)
        key q
        sleep 1
        off=$(call_level key-off)
        check "a pad key starts and stops a sound ($on dBFS, then $off dBFS)" at_least "$on" "$off" 20
    fi

    # The app starts while its PipeWire instance (a private one, with no
    # session manager, so nothing can be played there) is not running, so
    # without audio, and gets it back with "Try Again".
    if section "audio returns after a retry"; then
        quit_app
        within 2 not node_exists
        private="verify-app-pipewire-$$"
        PIPEWIRE_REMOTE=$private start_app || echo "FAIL the app starts without PipeWire"
        activate toggle-sound "'$tone'"
        sleep 1
        check "no call stream can start without audio" not call_linked
        PIPEWIRE_CORE=$private pipewire >"$work/private-pipewire.log" 2>&1 &
        private_pid=$!
        within 5 test -S "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$private"
        activate retry-audio
        check "the node of the engine exists after the retry" within 3 \
            eval "PIPEWIRE_REMOTE=$private pw-dump | grep -q '\"node.name\": \"vinheta-drain-'"
        quit_app
        kill "$private_pid" 2>/dev/null
        wait "$private_pid" 2>/dev/null
        rm -f "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$private"{,.lock,-manager,-manager.lock}
        start_app || echo "FAIL the app starts again"
    fi

    echo "== exit"
    quit_app
    sleep 0.5
    pw-cli destroy "$mic" >/dev/null
    check "nothing is left after the app quits" not node_exists
    check "the folders were saved" grep -q "directories=.*Tones" "$work/config/glib-2.0/settings/keyfile"
    if [ -n "$saved" ]; then
        check "a loop set through an action was saved" loop_saved
        check "a reset pad has no entry in the file" entry_removed
        check "moved settings" settings_moved
    fi
}

{
    echo ". '$root/scripts/dev-common.sh'"
    echo ". '$root/scripts/audio-common.sh'"
    declare -p work tone quiet looped gone kept moved pads test_sink test_mic test_app other_app only
    declare -f check node_exists call_linked not call_app call_level sent_to at_least \
        monitor_on monitor_linked falls_by call_streams one_call_stream within pad_field \
        loop_saved entry_removed settings_moved section session
    echo session
} >"$work/session.sh"

# The session daemons print to stdout too, so only the result lines are kept.
virtual_session "$work/config" bash "$work/session.sh" 2>"$work/session.log" |
    grep --line-buffered -E "^(PASS|FAIL|==)" | tee "$work/result.txt"

# In case the session ended before it removed its fake devices.
for node in "$test_sink" "$test_mic"; do
    leftover=$(node_id "$node")
    [ -n "$leftover" ] && pw-cli destroy "$leftover" >/dev/null
done

if grep -q "^FAIL" "$work/result.txt" || ! grep -q "^PASS nothing is left" "$work/result.txt"; then
    echo "CHECKS FAILED, see $work/session.log"
    exit 1
fi
echo "ALL CHECKS PASSED"
