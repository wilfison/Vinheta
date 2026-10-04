#!/usr/bin/env bash
# Writes the pictures the metainfo and the README point at, in
# data/screenshots, from the app installed in build/install: the main window,
# the preferences, and the call setup guide, in the light style.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"
require_tools ffmpeg
require_app

library="$root/tmp/metainfo-library"
out="$root/data/screenshots"
shots="$root/tmp/screenshots"
rm -rf "$library"
mkdir -p "$library/Meetings" "$library/Music" "$out"
for name in "Air Horn" Applause Crickets "Drum Roll" "Laugh Track" Rimshot "Sad Trombone" "Ta-Da"; do
    silent_sound "$library/Meetings/$name.wav" 20 || exit 1
done
for name in "Hold Music" Intro Jingle Outro; do
    silent_sound "$library/Music/$name.wav" 20 || exit 1
done
m="$library/Meetings"
cat >"$library/pads.json" <<JSON
{"version": 1, "pads": {
  "$m/Air Horn.wav": {"color": "red", "shortcut": "1"},
  "$m/Applause.wav": {"color": "green", "shortcut": "2", "favorite": true},
  "$m/Crickets.wav": {"color": "brown"},
  "$m/Drum Roll.wav": {"color": "orange", "shortcut": "q"},
  "$m/Laugh Track.wav": {"color": "yellow", "shortcut": "w"},
  "$m/Rimshot.wav": {"color": "blue", "shortcut": "e"},
  "$m/Sad Trombone.wav": {"color": "purple", "favorite": true},
  "$library/Music/Hold Music.wav": {"color": "blue", "loop": true},
  "$library/Music/Intro.wav": {"color": "purple", "favorite": true}
}}
JSON

# The window is at the top left corner of the display, at its default size.
window=1000x700+0+0
shot() {
    local name=$1
    shift
    "$root/scripts/screenshot.sh" "metainfo-$name" --light --crop "$window" \
        --folder "$library/Meetings" --folder "$library/Music" --pads "$library/pads.json" "$@" || exit 1
    mv "$shots/metainfo-$name.png" "$out/$name.png"
    echo "$out/$name.png"
}
shot main --action "toggle-sound '$m/Applause.wav'" --wait 4
shot preferences --action preferences
shot call-guide --action call-guide
rm -rf "$library"
