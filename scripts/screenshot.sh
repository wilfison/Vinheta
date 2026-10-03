#!/usr/bin/env bash
# Captures the app window in a known state, on a virtual display, with its own
# D-Bus session and settings. Nothing shows up on the desktop and the user's
# settings are not touched. The app still talks to the real PipeWire.
set -uo pipefail

usage() {
    cat >&2 <<'USAGE'
usage: screenshot.sh NAME [--folder DIR]... [--setting 'KEY VALUE']... [STEP]...
                     [--no-audio] [--light]

Writes tmp/screenshots/NAME.png from the app installed in build/install.
--folder DIR   adds DIR to the library before the app starts
--setting 'KEY VALUE'  sets a key of the app's settings before the app starts;
               VALUE is a GVariant such as 0.5, false, or "'text'"
--no-audio     makes PipeWire unreachable, to capture the audio failure state
--light        uses the light style instead of the dark one

Steps run in the given order once the window is up, before the capture:
--action 'ACTION [PARAMETER]'  activates app.NAME (or NAME) or win.NAME;
                               PARAMETER is a GVariant such as "'text'"
--click X,Y    clicks at that position of the window
--key KEYS     presses keys, in xdotool syntax (Return, ctrl+q, Tab)
--size W,H     resizes the window
--wait SECONDS waits
--exec COMMAND runs a shell command, for example to create a fake device
USAGE
    exit 2
}

. "$(dirname "$0")/dev-common.sh"

name=
folders=()
settings=()
steps=()
scheme=prefer-dark
no_audio=
while [ $# -gt 0 ]; do
    case $1 in
        --folder) [ $# -ge 2 ] || usage; folders+=("$(realpath "$2")"); shift ;;
        --setting) [ $# -ge 2 ] || usage; settings+=("$2"); shift ;;
        --action | --click | --key | --size | --wait | --exec)
            [ $# -ge 2 ] || usage
            steps+=("${1#--} $2")
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

require_tools xvfb-run dbus-run-session import gapplication gdbus gsettings xdotool
require_app

config="$root/tmp/screenshot-config"
output="$root/tmp/screenshots/$name.png"
log="$root/tmp/screenshot.log"
rm -rf "$config"
mkdir -p "$config" "$(dirname "$output")"
rm -f "$output"
directories=$(gvariant_strv "${folders[@]}")

session() {
    local step kind value setting
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
        esac || echo "step failed: $step" >&2
        sleep 0.5
    done
    sleep 1

    import -window root "$output"
    quit_app
}

# xvfb-run goes through sh, which drops exported functions, so the session is
# handed over as a script.
{
    echo ". '$root/scripts/dev-common.sh'"
    declare -p directories output settings steps
    declare -f session
    echo session
} >"$config/session.sh"

export ADW_DEBUG_COLOR_SCHEME=$scheme
[ -n "$no_audio" ] && export PIPEWIRE_REMOTE=vinheta-screenshot-no-audio
virtual_session "$config" bash "$config/session.sh" >"$log" 2>&1

if [ ! -s "$output" ]; then
    echo "no screenshot was taken, see $log" >&2
    exit 1
fi
grep -E "^step failed" "$log" >&2
echo "$output"
