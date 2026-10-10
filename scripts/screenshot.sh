#!/usr/bin/env bash
# Captures the app window in a known state, on a virtual display, with its own
# D-Bus session and settings. Nothing shows up on the desktop and the user's
# settings are not touched. The app still talks to the real PipeWire.
set -uo pipefail

usage() {
    cat >&2 <<'USAGE'
usage: screenshot.sh NAME [--folder DIR]... [--setting 'KEY VALUE']... [--pads FILE]
                     [--background FILE]...
                     [--fake-app 'NAME DESCRIPTION']... [--fake-sink 'NODE DESCRIPTION']...
                     [STEP]... [--no-audio] [--private-pipewire] [--first-run]
                     [--lang LOCALE] [--light] [--debug] [--sheet]
       screenshot.sh --clean

Writes tmp/screenshots/NAME.png from the app installed in build/install.
--folder DIR   adds DIR to the library before the app starts
--setting 'KEY VALUE'  sets a key of the app's settings before the app starts;
               VALUE is a GVariant such as 0.5, false, or "'text'"
--pads FILE    uses FILE as the pad settings (pads.json) the app starts with:
               {"version": 1, "pads": {"/abs/sound.wav": {"name": "Intro",
               "color": "purple", "volume": 0.8, "loop": true}}}
--background FILE  copies FILE into the backgrounds directory of the app,
               keeping its name, so that a pad file can name it
               ("background": "NAME")
--fake-sink 'NODE DESCRIPTION'
               creates a fake output before the app starts, for a known entry
               in the output list (use the node name prefix vinheta-shot-)
--fake-app 'NAME DESCRIPTION'
               starts a fake call app before the app starts, for a known
               entry in the list of apps the sounds are sent to: a recorder
               of a silent fake microphone (use the prefix vinheta-shot- too;
               NAME is what the call-target key stores). It needs the session
               manager, so it does not show with --private-pipewire
--no-audio     makes PipeWire unreachable, to capture the audio failure state
--private-pipewire  starts a PipeWire instance of its own (no session manager,
               no devices) and points the app, and every fake device, at it.
               A pad shows as playing at 00:00 there and nothing is heard, so
               --expect-playing cannot be used with it
--first-run    leaves call-guide-shown at its default, so the call setup guide
               opens by itself (every other run starts with it set to true)
--lang LOCALE  runs the app in that language (pt_BR)
--light        uses the light style instead of the dark one
--debug        prints the app's debug messages (playback start times) at the end
--sheet        also writes tmp/screenshots/NAME-sheet.png, every capture of
               the run stacked in one picture, to review them in one look
--clean        removes every picture of tmp/screenshots and exits

Steps run in the given order once the window is up, before the capture:
--action 'ACTION [PARAMETER]'  activates app.NAME (or NAME) or win.NAME;
                               PARAMETER is a GVariant such as "'text'"
--click X,Y    clicks at that position of the window
--right-click X,Y  clicks there with the secondary button
--drop 'X,Y FILE[;FILE]...'  drags the files from a helper window, as a file
               manager would, and drops them at that position of the window
--key KEYS     presses keys, in xdotool syntax (Return, ctrl+q, Tab)
--size W,H     resizes the window
--wait SECONDS waits
--plug-app 'NAME DESCRIPTION', --plug-sink 'NODE DESCRIPTION'
               starts a fake call app or creates a fake output while the app
               runs
--unplug NODE  destroys a fake output, or stops a fake call app
--stop-pipewire, --start-pipewire  kills and starts the instance of
               --private-pipewire while the app runs
--capture NAME writes tmp/screenshots/NAME.png at this point of the sequence
--crop WxH+X+Y crops the captures that follow (--crop full undoes it)
--restart      quits the app and starts it again with the same settings and
               pad settings, to check what is restored
--expect-setting 'KEY VALUE'  fails the run unless the key of the app's
               settings has that value, as "gsettings get" prints it (false,
               0.5, 'text'); it waits up to 2 seconds for it
--expect-pad 'FIELD VALUE SOUND'  fails the run unless the pad file the
               app wrote has that value for the absolute path SOUND. FIELD
               may be nested (crop.width); a number is compared with 3
               decimals (0.5, 0.386), a missing field is none, true and
               false are lower case. It waits up to 2 seconds (a change is
               saved within half a second)
--expect-playing N  fails the run unless N sounds are playing (the call
               streams of the app in PipeWire); it waits up to 2 seconds
--exec COMMAND runs a shell command; what it prints is shown at the end, and
               a status other than 0 is reported as a failed step (end it
               with "|| true" when that is expected)

Warnings and criticals logged by the app are printed at the end.

Fake outputs and call apps are removed when the script ends, whatever happens.
USAGE
    exit 2
}

. "$(dirname "$0")/dev-common.sh"
. "$root/scripts/audio-common.sh"

name=
folders=()
settings=()
steps=()
# "KIND NODE DESCRIPTION" of the devices created before the app starts, and
# the node names of every fake device, to destroy them at the end.
fakes=()
fake_names=()
captures=()
scheme=prefer-dark
no_audio=
pads=
backgrounds=()
debug=
sheet=
first_run=
lang=
private=
while [ $# -gt 0 ]; do
    case $1 in
        --folder) [ $# -ge 2 ] || usage; folders+=("$(realpath "$2")"); shift ;;
        --setting) [ $# -ge 2 ] || usage; settings+=("$2"); shift ;;
        --pads) [ $# -ge 2 ] || usage; pads=$(realpath "$2"); shift ;;
        --background) [ $# -ge 2 ] || usage; backgrounds+=("$(realpath "$2")"); shift ;;
        --fake-app | --fake-sink)
            [ $# -ge 2 ] || usage
            fakes+=("${1#--fake-} $2")
            fake_names+=("${2%% *}")
            shift
            ;;
        --action | --click | --right-click | --drop | --key | --size | --wait | --exec | --expect-setting | --expect-pad | --expect-playing | --plug-app | --plug-sink | --unplug | --capture | --crop)
            [ $# -ge 2 ] || usage
            steps+=("${1#--} $2")
            case $1 in
                --plug-*) fake_names+=("${2%% *}") ;;
                --capture) captures+=("$2") ;;
            esac
            shift
            ;;
        --restart) steps+=("restart -") ;;
        --stop-pipewire) steps+=("stop-pipewire -") ;;
        --start-pipewire) steps+=("start-pipewire -") ;;
        --private-pipewire) private=1 ;;
        --first-run) first_run=1 ;;
        --lang) [ $# -ge 2 ] || usage; lang=$2; shift ;;
        --debug) debug=1 ;;
        --sheet) sheet=1 ;;
        --clean)
            find "$(dirname "$0")/../tmp/screenshots" -maxdepth 1 -name '*.png' -delete 2>/dev/null
            exit 0
            ;;
        --no-audio) no_audio=1 ;;
        --light) scheme=prefer-light ;;
        -*) usage ;;
        *) [ -z "$name" ] || usage; name=$1 ;;
    esac
    shift
done
[ -n "$name" ] || usage

require_tools xvfb-run dbus-run-session import mogrify convert gapplication gdbus gsettings xdotool pw-cli pw-dump python3
require_app
captures+=("$name")

config="$root/tmp/screenshot-config"
shots="$root/tmp/screenshots"
log="$root/tmp/screenshot.log"
rm -rf "$config"
mkdir -p "$config" "$shots"
for capture in "${captures[@]}"; do rm -f "$shots/$capture.png"; done
if [ -n "$pads" ]; then
    mkdir -p "$config/data/vinheta"
    cp "$pads" "$config/data/vinheta/pads.json" || exit 1
fi
if [ ${#backgrounds[@]} -gt 0 ]; then
    mkdir -p "$config/data/vinheta/backgrounds"
    cp "${backgrounds[@]}" "$config/data/vinheta/backgrounds/" || exit 1
fi

# The private instance is known by its process id, never by its name: a
# "pkill pipewire" would kill the real one.
private_name=vinheta-screenshot-pipewire
private_pid_file="$config/pipewire.pid"
private_socket="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$private_name"
start_pipewire() {
    local i
    PIPEWIRE_REMOTE= PIPEWIRE_CORE=$private_name pipewire >>"$config/pipewire.log" 2>&1 &
    echo $! >"$private_pid_file"
    for i in $(seq 50); do
        [ -S "$private_socket" ] && return 0
        sleep 0.1
    done
    echo "the private PipeWire instance did not start" >&2
    return 1
}
stop_pipewire() {
    local pid i
    pid=$(cat "$private_pid_file" 2>/dev/null) || return 0
    kill "$pid" 2>/dev/null
    for i in $(seq 30); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done
    rm -f "$private_pid_file" "$private_socket" "$private_socket.lock" \
        "$private_socket-manager" "$private_socket-manager.lock"
}

# fake_device KIND NAME DESCRIPTION. An app is a recorder of a silent fake
# microphone, which is created with the first one.
app_mic=vinheta-shot-app-mic
app_pids="$config/app.pids"
fake_names+=("$app_mic")
fake_device() {
    if [ "$1" = sink ]; then
        create_node "$2" "$3" Audio/Sink "FL FR" >/dev/null
        return
    fi
    node_exists "$app_mic" ||
        create_node "$app_mic" "Vinheta screenshot microphone" Audio/Source/Virtual MONO >/dev/null || return 1
    pw-record --target "$app_mic" \
        -P "{ state.restore-props=false node.name=$2 application.name=\"$3\" application.process.binary=$2 }" \
        /dev/null >/dev/null 2>&1 &
    echo $! >>"$app_pids"
}

# How many sounds play: each has one call stream.
playing_count() { pw-dump | grep -c '"node.name": "vinheta-call-'; }

# pad_value FIELD SOUND: the value of a field of the entry of a sound in the
# pad file the app wrote, as --expect-pad compares it.
pad_value() {
    python3 -I - "$config/data/vinheta/pads.json" "$1" "$2" <<'PYTHON'
import json, sys

path, field, sound = sys.argv[1:]
try:
    with open(path) as file:
        value = json.load(file)["pads"].get(sound, {})
except (OSError, ValueError, KeyError):
    value = {}
for part in field.split("."):
    value = value.get(part) if isinstance(value, dict) else None
if value is None:
    print("none")
elif isinstance(value, bool):
    print(str(value).lower())
elif isinstance(value, (int, float)):
    print(f"{value:.3f}".rstrip("0").rstrip("."))
else:
    print(value if isinstance(value, str) else json.dumps(value))
PYTHON
}

# expect WHAT VALUE COMMAND...: the command prints VALUE within 2 seconds.
expect() {
    local what=$1 value=$2 got i
    shift 2
    for i in $(seq 20); do
        got=$("$@")
        [ "$got" = "$value" ] && return 0
        sleep 0.1
    done
    echo "expected $what to be $value, it is $got" >&2
    return 1
}

unplug() {
    local id
    id=$(node_id "$1")
    [ -z "$id" ] || pw-cli destroy "$id" >/dev/null
}

remove_fakes() {
    local node
    [ ! -f "$app_pids" ] || xargs -r kill <"$app_pids" 2>/dev/null
    for node in "${fake_names[@]}"; do unplug "$node"; done
    [ -z "$private" ] || stop_pipewire
}
trap remove_fakes EXIT
trap 'exit 130' INT TERM

if [ -n "$private" ]; then
    start_pipewire || exit 1
    export PIPEWIRE_REMOTE=$private_name
fi

for fake in "${fakes[@]}"; do
    kind=${fake%% *}
    rest=${fake#* }
    fake_device "$kind" "${rest%% *}" "${rest#* }" || exit 1
done
directories=$(gvariant_strv "${folders[@]}")

session() {
    local step kind value setting crop= point files

    capture() {
        import -window root "$shots/$1.png"
        [ -z "$crop" ] || mogrify -crop "$crop" +repage "$shots/$1.png"
    }
    gsettings set "$app_id" directories "$directories" || return 1
    [ -n "$first_run" ] || gsettings set "$app_id" call-guide-shown true || return 1
    for setting in "${settings[@]}"; do
        gsettings set "$app_id" "${setting%% *}" "${setting#* }" || return 1
    done
    start_app || return 1

    for step in "${steps[@]}"; do
        kind=${step%% *}
        value=${step#* }
        case $kind in
            action)
                if [ "${value%% *}" = "$value" ]; then
                    activate "$value"
                else
                    activate "${value%% *}" "${value#* }"
                fi
                ;;
            click) click "${value%,*}" "${value#*,}" ;;
            right-click) right_click "${value%,*}" "${value#*,}" ;;
            drop)
                point=${value%% *}
                IFS=';' read -ra files <<<"${value#* }"
                drop_files "${point%,*}" "${point#*,}" "${files[@]}"
                ;;
            key) key "$value" ;;
            size) resize_window "${value%,*}" "${value#*,}" ;;
            wait) sleep "$value" ;;
            # What it prints is shown at the end, with the failures.
            exec) bash -c "$value" 2>&1 | sed 's/^/exec: /'; [ "${PIPESTATUS[0]}" -eq 0 ] ;;
            expect-setting) expect "setting ${value%% *}" "${value#* }" gsettings get "$app_id" "${value%% *}" ;;
            expect-playing) expect "sounds playing" "$value" playing_count ;;
            expect-pad)
                setting=${value#* }
                expect "${setting#* }: ${value%% *}" "${setting%% *}" pad_value "${value%% *}" "${setting#* }"
                ;;
            plug-app) fake_device app "${value%% *}" "${value#* }" ;;
            plug-sink) fake_device sink "${value%% *}" "${value#* }" ;;
            unplug) unplug "$value" ;;
            stop-pipewire) stop_pipewire ;;
            start-pipewire) start_pipewire ;;
            capture) capture "$value" ;;
            restart) quit_app; start_app ;;
            crop) crop=$value; [ "$crop" != full ] || crop= ;;
        esac || echo "step failed: $step" >&2
        sleep 0.5
    done
    sleep 1

    capture "$name"
    quit_app
}

# xvfb-run goes through sh, which drops exported functions, so the session is
# handed over as a script.
{
    echo ". '$root/scripts/dev-common.sh'"
    echo ". '$root/scripts/audio-common.sh'"
    declare -p directories shots name settings steps first_run private_name private_pid_file private_socket config app_mic app_pids
    declare -f fake_device unplug playing_count pad_value expect start_pipewire stop_pipewire session
    echo session
} >"$config/session.sh"

export ADW_DEBUG_COLOR_SCHEME=$scheme
[ -n "$no_audio" ] && export PIPEWIRE_REMOTE=vinheta-screenshot-no-audio
[ -n "$debug" ] && export G_MESSAGES_DEBUG=vinheta
session_language=$lang
virtual_session "$config" bash "$config/session.sh" >"$log" 2>&1

status=0
for capture in "${captures[@]}"; do
    if [ -s "$shots/$capture.png" ]; then
        echo "$shots/$capture.png"
    else
        echo "no screenshot $capture was taken, see $log" >&2
        status=1
    fi
done
if [ -n "$sheet" ] && [ "$status" -eq 0 ]; then
    files=()
    for capture in "${captures[@]}"; do files+=("$shots/$capture.png"); done
    convert "${files[@]}" -append "$shots/$name-sheet.png" && echo "$shots/$name-sheet.png"
fi
grep -E "^(expected|step failed|exec: )" "$log" >&2
grep -q "^step failed: expect-" "$log" && status=1
# What the app itself complained about (and its debug lines with --debug).
grep -E "^\(vinheta:[0-9]+\): .*-(WARNING|CRITICAL)${debug:+|^\(vinheta:[0-9]+\): vinheta-DEBUG}" "$log" >&2
exit "$status"
