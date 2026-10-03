#!/usr/bin/env bash
# Captures the app window in a known state, on a virtual display, with its own
# D-Bus session and settings. Nothing shows up on the desktop and the user's
# settings are not touched. The app still talks to the real PipeWire.
set -uo pipefail

usage() {
    cat >&2 <<'USAGE'
usage: screenshot.sh NAME [--folder DIR]... [--setting 'KEY VALUE']...
                     [--fake-mic 'NODE DESCRIPTION']... [--fake-sink 'NODE DESCRIPTION']...
                     [STEP]... [--no-audio] [--light]

Writes tmp/screenshots/NAME.png from the app installed in build/install.
--folder DIR   adds DIR to the library before the app starts
--setting 'KEY VALUE'  sets a key of the app's settings before the app starts;
               VALUE is a GVariant such as 0.5, false, or "'text'"
--fake-mic 'NODE DESCRIPTION', --fake-sink 'NODE DESCRIPTION'
               creates a fake device before the app starts, for a known entry
               in the device lists (use the node name prefix vinheta-shot-)
--no-audio     makes PipeWire unreachable, to capture the audio failure state
--light        uses the light style instead of the dark one

Steps run in the given order once the window is up, before the capture:
--action 'ACTION [PARAMETER]'  activates app.NAME (or NAME) or win.NAME;
                               PARAMETER is a GVariant such as "'text'"
--click X,Y    clicks at that position of the window
--key KEYS     presses keys, in xdotool syntax (Return, ctrl+q, Tab)
--size W,H     resizes the window
--wait SECONDS waits
--plug-mic 'NODE DESCRIPTION', --plug-sink 'NODE DESCRIPTION'
               creates a fake device while the app runs
--unplug NODE  destroys a fake device
--capture NAME writes tmp/screenshots/NAME.png at this point of the sequence
--crop WxH+X+Y crops the captures that follow (--crop full undoes it)
--exec COMMAND runs a shell command

Fake devices are destroyed when the script ends, whatever happens.
USAGE
    exit 2
}

. "$(dirname "$0")/dev-common.sh"
. "$root/scripts/audio-poc-common.sh"

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
while [ $# -gt 0 ]; do
    case $1 in
        --folder) [ $# -ge 2 ] || usage; folders+=("$(realpath "$2")"); shift ;;
        --setting) [ $# -ge 2 ] || usage; settings+=("$2"); shift ;;
        --fake-mic | --fake-sink)
            [ $# -ge 2 ] || usage
            fakes+=("${1#--fake-} $2")
            fake_names+=("${2%% *}")
            shift
            ;;
        --action | --click | --key | --size | --wait | --exec | --plug-mic | --plug-sink | --unplug | --capture | --crop)
            [ $# -ge 2 ] || usage
            steps+=("${1#--} $2")
            case $1 in
                --plug-*) fake_names+=("${2%% *}") ;;
                --capture) captures+=("$2") ;;
            esac
            shift
            ;;
        --no-audio) no_audio=1 ;;
        --light) scheme=prefer-light ;;
        -*) usage ;;
        *) [ -z "$name" ] || usage; name=$1 ;;
    esac
    shift
done
[ -n "$name" ] || usage

require_tools xvfb-run dbus-run-session import mogrify gapplication gdbus gsettings xdotool pw-cli pw-dump python3
require_app
captures+=("$name")

config="$root/tmp/screenshot-config"
shots="$root/tmp/screenshots"
log="$root/tmp/screenshot.log"
rm -rf "$config"
mkdir -p "$config" "$shots"
for capture in "${captures[@]}"; do rm -f "$shots/$capture.png"; done

# fake_device KIND NODE DESCRIPTION
fake_device() {
    local class=Audio/Sink positions="FL FR"
    [ "$1" = mic ] && class=Audio/Source/Virtual positions=MONO
    create_node "$2" "$3" "$class" "$positions" >/dev/null
}

unplug() {
    local id
    id=$(node_id "$1")
    [ -z "$id" ] || pw-cli destroy "$id" >/dev/null
}

remove_fakes() {
    local node
    for node in "${fake_names[@]}"; do unplug "$node"; done
}
trap remove_fakes EXIT
trap 'exit 130' INT TERM

for fake in "${fakes[@]}"; do
    kind=${fake%% *}
    rest=${fake#* }
    fake_device "$kind" "${rest%% *}" "${rest#* }" || exit 1
done
directories=$(gvariant_strv "${folders[@]}")

session() {
    local step kind value setting crop=

    capture() {
        import -window root "$shots/$1.png"
        [ -z "$crop" ] || mogrify -crop "$crop" +repage "$shots/$1.png"
    }
    gsettings set "$app_id" directories "$directories" || return 1
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
            key) key "$value" ;;
            size) resize_window "${value%,*}" "${value#*,}" ;;
            wait) sleep "$value" ;;
            exec) bash -c "$value" ;;
            plug-mic) fake_device mic "${value%% *}" "${value#* }" ;;
            plug-sink) fake_device sink "${value%% *}" "${value#* }" ;;
            unplug) unplug "$value" ;;
            capture) capture "$value" ;;
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
    echo ". '$root/scripts/audio-poc-common.sh'"
    declare -p directories shots name settings steps
    declare -f fake_device unplug session
    echo session
} >"$config/session.sh"

export ADW_DEBUG_COLOR_SCHEME=$scheme
[ -n "$no_audio" ] && export PIPEWIRE_REMOTE=vinheta-screenshot-no-audio
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
grep -E "^step failed" "$log" >&2
exit "$status"
