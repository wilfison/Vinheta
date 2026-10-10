# Helpers shared by the development scripts (run-dev, check, screenshot,
# verify-app, fixtures). Meant to be sourced, from bash.

# Another shell has no BASH_SOURCE, and the root below would be wrong.
if [ -z "${BASH_VERSION:-}" ]; then
    echo "scripts/dev-common.sh must be sourced from bash" >&2
    return 1 2>/dev/null || exit 1
fi

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
prefix="$root/build/install"
app_id=io.github.wilfison.Vinheta
app_path=/io/github/wilfison/Vinheta

require_tools() {
    local tool
    for tool in "$@"; do
        command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
    done
}

# Configures build/ with the local prefix when needed, then builds and installs.
install_app() {
    if [ ! -d "$root/build" ]; then
        meson setup "$root/build" "$root" --prefix="$prefix" || return 1
    fi
    meson install -C "$root/build" >"$root/build/install.log" 2>&1 || {
        tail -n 40 "$root/build/install.log" >&2
        return 1
    }
}

require_app() {
    [ -x "$prefix/bin/vinheta" ] || {
        echo "missing $prefix/bin/vinheta: run scripts/run-dev.sh or meson install -C build first" >&2
        exit 1
    }
}

# What the binary needs to find its schema and icon in the local prefix.
app_env() {
    export GSETTINGS_SCHEMA_DIR="$prefix/share/glib-2.0/schemas"
    export XDG_DATA_DIRS="$prefix/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
}

# virtual_session CONFIG_DIR COMMAND...: runs COMMAND on a virtual display,
# with its own D-Bus session, with settings kept in CONFIG_DIR and data (the
# pad settings, vinheta/pads.json) in CONFIG_DIR/data, so nothing shows up on
# the desktop and the user's settings and pads are not touched. PipeWire is
# still the real one. The app runs in English whatever the language of the
# user, unless session_language is set (pt_BR).
virtual_session() {
    local config=$1 language=${session_language:-}
    shift
    app_env
    LANGUAGE=$language LANG=${language:-C}.UTF-8 LC_ALL=${language:-C}.UTF-8 \
        GDK_BACKEND=x11 GSETTINGS_BACKEND=keyfile XDG_CONFIG_HOME="$config" XDG_DATA_HOME="$config/data" \
        xvfb-run -a -s "-screen 0 1100x800x24" dbus-run-session -- "$@"
}

# gvariant_strv ITEM...: prints the items as a GVariant array of strings.
gvariant_strv() {
    local item out="["
    for item in "$@"; do
        out+="'${item//\'/\\\'}', "
    done
    echo "${out%, }]"
}

# The functions below are for use inside a virtual session.

start_app() {
    "$prefix/bin/vinheta" &
    app_pid=$!
    gdbus wait --session --timeout 15 "$app_id" || { kill "$app_pid" 2>/dev/null; return 1; }
    sleep 2
}

quit_app() {
    local i
    gapplication action "$app_id" quit
    for i in $(seq 30); do
        kill -0 "$app_pid" 2>/dev/null || break
        sleep 0.1
    done
    kill "$app_pid" 2>/dev/null
    wait "$app_pid" 2>/dev/null
}

# activate NAME [PARAMETER]: app.NAME (or a bare NAME) goes through
# gapplication; win.NAME is only exported on the window's own object path.
activate() {
    local name=$1 parameter=${2:-}
    if [[ $name == win.* ]]; then
        gdbus call --session --dest "$app_id" --object-path "$app_path/window/1" \
            --method org.gtk.Actions.Activate "${name#win.}" "[${parameter:+<$parameter>}]" "{}" >/dev/null
    else
        gapplication action "$app_id" "${name#app.}" ${parameter:+"$parameter"}
    fi
}

# The window sits at the top left corner of the display, so window and screen
# coordinates are the same.
click() { xdotool mousemove "$1" "$2" click 1; }
right_click() { xdotool mousemove "$1" "$2" click 3; }
key() { xdotool key "$@"; }
resize_window() {
    xdotool search --onlyvisible --name '^Vinheta$' | head -n 1 | xargs -I{} xdotool windowsize {} "$1" "$2"
}

# drop_files X Y FILE...: a real drag and drop of the files from a small
# window that a helper opens at the top left corner, over the app window, to
# the point X,Y. The helper offers the files as a GTK file list, as a file
# manager does.
drop_files() {
    local x=$1 y=$2 helper
    shift 2
    helper=$(
        cat <<'PY'
import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk

files = [Gio.File.new_for_path(path) for path in sys.argv[1:]]


def activate(app):
    window = Gtk.ApplicationWindow(application=app, title="drag-source")
    window.set_decorated(False)
    window.set_default_size(60, 40)
    source = Gtk.DragSource(actions=Gdk.DragAction.COPY)
    source.connect("prepare", lambda *_: Gdk.ContentProvider.new_for_value(Gdk.FileList.new_from_list(files)))
    source.connect("drag-end", lambda *_: GLib.timeout_add(300, app.quit))
    window.add_controller(source)
    window.present()
    GLib.timeout_add_seconds(15, app.quit)


app = Gtk.Application(flags=Gio.ApplicationFlags.NON_UNIQUE)
app.connect("activate", activate)
app.run([])
PY
    )
    GDK_BACKEND=x11 python3 -c "$helper" "$@" &
    local pid=$!
    xdotool search --sync --onlyvisible --name '^drag-source$' >/dev/null || return 1
    # The motion in steps lets GTK see a drag start, then cross the window.
    xdotool mousemove 30 20 mousedown 1 sleep 0.3 mousemove 50 40 sleep 0.3 \
        mousemove $(((x + 30) / 2)) $(((y + 20) / 2)) sleep 0.3 \
        mousemove "$((x - 4))" "$((y - 4))" sleep 0.3 mousemove "$x" "$y" sleep 0.5 mouseup 1
    wait "$pid"
}

# silent_sound FILE [SECONDS]: a sound that can be played without being heard.
silent_sound() {
    ffmpeg -v error -y -f lavfi -i anullsrc=r=48000:cl=stereo -t "${2:-30}" "$1"
}

# tone_level FILE: level of the 1000 Hz component of the left channel of a
# 16 bit WAV file, in dBFS. Measuring a single frequency over the whole file
# keeps microphone noise out of the result.
tone_level() {
    python3 - "$1" <<'PY'
import array, math, sys, wave

with wave.open(sys.argv[1]) as wav:
    rate, channels = wav.getframerate(), wav.getnchannels()
    samples = array.array("h", wav.readframes(wav.getnframes()))[::channels]
w = 2 * math.pi * 1000 / rate
re = sum(s * math.cos(w * i) for i, s in enumerate(samples))
im = sum(s * math.sin(w * i) for i, s in enumerate(samples))
amplitude = 2 * math.hypot(re, im) / max(len(samples), 1) / 32768
print(f"{20 * math.log10(max(amplitude, 1e-10)):.1f}")
PY
}

# tone_sound FILE [SECONDS]: a 1000 Hz tone at -45 dBFS, quiet but measurable.
tone_sound() {
    ffmpeg -v error -y -f lavfi -i "sine=frequency=1000:duration=${2:-30}" \
        -af "volume=-45dB" -ac 2 -ar 48000 "$1"
}

# loud_sound FILE [SECONDS]: a 1000 Hz tone at 0 dBFS, for the call limiter.
# Play it only with the monitor volume at 0 and a fake call app as the target.
loud_sound() {
    ffmpeg -v error -y -f lavfi -i "aevalsrc=sin(2*PI*1000*t):s=48000:d=${2:-30}" \
        -af "pan=stereo|c0=c0|c1=c0" "$1"
}
