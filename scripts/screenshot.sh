#!/usr/bin/env bash
# Captures the app window in a known state, on a virtual display, with its own
# D-Bus session and settings. Nothing shows up on the desktop and the user's
# settings are not touched. The app still talks to the real PipeWire.
set -uo pipefail

usage() {
    cat >&2 <<'EOF'
usage: screenshot.sh NAME [--folder DIR]... [--action 'ACTION [PARAMETER]']... [--no-audio] [--light]

Writes tmp/screenshots/NAME.png from the app installed in build/install.
--folder    adds DIR to the library before the app starts
--action    activates an action once the window is up; app.NAME (or NAME) and
            win.NAME are accepted, PARAMETER is a GVariant such as "'text'"
--no-audio  makes PipeWire unreachable, to capture the audio failure state
--light     uses the light style instead of the dark one
EOF
    exit 2
}

root=$(cd "$(dirname "$0")/.." && pwd)
prefix="$root/build/install"
app_id=io.github.wilfison.Vinheta
app_path=/io/github/wilfison/Vinheta

name=
folders=()
actions=()
scheme=prefer-dark
no_audio=
while [ $# -gt 0 ]; do
    case $1 in
        --folder) [ $# -ge 2 ] || usage; folders+=("$(realpath "$2")"); shift ;;
        --action) [ $# -ge 2 ] || usage; actions+=("$2"); shift ;;
        --no-audio) no_audio=1 ;;
        --light) scheme=prefer-light ;;
        -*) usage ;;
        *) [ -z "$name" ] || usage; name=$1 ;;
    esac
    shift
done
[ -n "$name" ] || usage

for tool in xvfb-run dbus-run-session import gapplication gdbus gsettings; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
[ -x "$prefix/bin/vinheta" ] || {
    echo "missing $prefix/bin/vinheta: install into the local prefix first (see AGENTS.md)" >&2
    exit 1
}

config="$root/tmp/screenshot-config"
output="$root/tmp/screenshots/$name.png"
rm -rf "$config"
mkdir -p "$config" "$(dirname "$output")"
rm -f "$output"

directories="["
for folder in "${folders[@]}"; do
    directories+="'${folder//\'/\\\'}', "
done
directories="${directories%, }]"

export GDK_BACKEND=x11
export GSETTINGS_BACKEND=keyfile
export XDG_CONFIG_HOME="$config"
export GSETTINGS_SCHEMA_DIR="$prefix/share/glib-2.0/schemas"
export XDG_DATA_DIRS="$prefix/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export ADW_DEBUG_COLOR_SCHEME=$scheme
[ -n "$no_audio" ] && export PIPEWIRE_REMOTE=vinheta-screenshot-no-audio

session() {
    local app action target parameter
    gsettings set "$app_id" directories "$directories" || return 1
    "$prefix/bin/vinheta" &
    app=$!
    gdbus wait --session --timeout 15 "$app_id" || { kill "$app" 2>/dev/null; return 1; }
    sleep 2

    for action in "${actions[@]}"; do
        target=${action%% *}
        parameter=
        [ "$target" != "$action" ] && parameter=${action#* }
        if [[ $target == win.* ]]; then
            gdbus call --session --dest "$app_id" --object-path "$app_path/window/1" \
                --method org.gtk.Actions.Activate "${target#win.}" "[${parameter:+<$parameter>}]" "{}" \
                >/dev/null || echo "action failed: $action" >&2
        else
            gapplication action "$app_id" "${target#app.}" ${parameter:+"$parameter"} ||
                echo "action failed: $action" >&2
        fi
        sleep 0.5
    done
    sleep 1

    import -window root "$output"

    gapplication action "$app_id" quit
    for _ in $(seq 30); do
        kill -0 "$app" 2>/dev/null || break
        sleep 0.1
    done
    kill "$app" 2>/dev/null
    wait "$app" 2>/dev/null
    return 0
}
# xvfb-run goes through sh, which drops exported functions, so the session is
# handed over as a script.
{
    declare -p app_id app_path prefix directories output actions
    declare -f session
    echo session
} >"$config/session.sh"

log="$root/tmp/screenshot.log"
xvfb-run -a -s "-screen 0 1100x800x24" dbus-run-session -- \
    bash "$config/session.sh" >"$log" 2>&1

if [ ! -s "$output" ]; then
    echo "no screenshot was taken, see $log" >&2
    exit 1
fi
grep -E "^action failed" "$log" >&2
echo "$output"
