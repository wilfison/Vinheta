#!/usr/bin/env bash
# Creates the sound folders and pad settings the screenshot checks use, in
# tmp/fixtures, and prints what it made. Running it again changes nothing.
set -uo pipefail

. "$(dirname "$0")/dev-common.sh"
require_tools ffmpeg

fixtures="$root/tmp/fixtures"
mkdir -p "$fixtures/Fixture" "$fixtures/Palette" "$fixtures/Loop" "$fixtures/Many" "$fixtures/Effects" "$fixtures/Broken"

sound() { [ -f "$1" ] || silent_sound "$1" "$2" || exit 1; }

# Three pads, 20 seconds each.
for name in "Air Horn" Applause Crickets; do
    sound "$fixtures/Fixture/$name.wav" 20
done
# One pad per palette color and a plain one.
for name in A-Blue B-Green C-Yellow D-Orange E-Red F-Purple G-Brown H-Plain; do
    sound "$fixtures/Palette/$name.wav" 20
done
# Short enough to see a loop start again.
sound "$fixtures/Loop/Short.wav" 3
# Enough pads to need scrolling.
for number in $(seq -w 1 60); do
    sound "$fixtures/Many/Sound $number.wav" 5
done
# A second folder for the search: "app" matches here and in Fixture.
for name in "Applause Short" Bell "Drum Roll"; do
    sound "$fixtures/Effects/$name.wav" 5
done

# Text in a sound file: it is listed as a pad and cannot be played.
[ -f "$fixtures/Broken/Broken.wav" ] || echo "this is not audio" >"$fixtures/Broken/Broken.wav"
[ -f "$fixtures/corrupt-pads.json" ] || echo '{"version": 1, "pads": {' >"$fixtures/corrupt-pads.json"

# A background as the app stores it, and a photo the app has to reduce.
image() {
    [ -f "$2" ] || ffmpeg -loglevel error -f lavfi -i "$1" -frames:v 1 -q:v 2 "$2" || exit 1
}
image testsrc2=size=512x384 "$fixtures/background.png"
image smptehdbars=size=4000x3000 "$fixtures/photo.jpg"

cat >"$fixtures/palette.json" <<JSON
{"version": 1, "pads": {
  "$fixtures/Palette/A-Blue.wav": {"color": "blue"},
  "$fixtures/Palette/B-Green.wav": {"color": "green", "loop": true},
  "$fixtures/Palette/C-Yellow.wav": {"color": "yellow"},
  "$fixtures/Palette/D-Orange.wav": {"color": "orange"},
  "$fixtures/Palette/E-Red.wav": {"color": "red"},
  "$fixtures/Palette/F-Purple.wav": {"color": "purple", "name": "Intro Music", "loop": true},
  "$fixtures/Palette/G-Brown.wav": {"color": "brown"}
}}
JSON
cat >"$fixtures/dialog.json" <<JSON
{"version": 1, "pads": {
  "$fixtures/Fixture/Applause.wav": {"name": "Big Applause", "color": "purple", "volume": 0.54, "loop": true}
}}
JSON
cat >"$fixtures/loop.json" <<JSON
{"version": 1, "pads": {"$fixtures/Loop/Short.wav": {"loop": true}}}
JSON
cat >"$fixtures/favorites.json" <<JSON
{"version": 1, "pads": {
  "$fixtures/Fixture/Applause.wav": {"favorite": true},
  "$fixtures/Effects/Bell.wav": {"favorite": true},
  "$fixtures/Fixture/Crickets.wav": {"name": "Zebra"}
}}
JSON
cat >"$fixtures/shortcuts.json" <<JSON
{"version": 1, "pads": {
  "$fixtures/Fixture/Air Horn.wav": {"shortcut": "1"},
  "$fixtures/Fixture/Applause.wav": {"shortcut": "q"},
  "$fixtures/Effects/Bell.wav": {"shortcut": "w"}
}}
JSON

cat >"$fixtures/background.json" <<JSON
{"version": 1, "pads": {
  "$fixtures/Fixture/Applause.wav": {"background": "background.png", "color": "purple"},
  "$fixtures/Fixture/Air Horn.wav": {"background": "background.png"}
}}
JSON

cat <<LIST
$fixtures/Fixture       Air Horn, Applause, Crickets (20 s, silent)
$fixtures/Palette       eight pads; --pads $fixtures/palette.json colors seven
$fixtures/Loop          Short (3 s, silent); --pads $fixtures/loop.json makes it loop
$fixtures/Many          Sound 01 to Sound 60 (5 s, silent), enough to scroll
$fixtures/Effects       Applause Short, Bell, Drum Roll (5 s, silent)
$fixtures/Broken        Broken (text, not audio: playing it fails)
$fixtures/corrupt-pads.json  not valid JSON, for --pads
$fixtures/dialog.json   Applause with a name, purple, volume 0.54, and loop
$fixtures/favorites.json  Applause and Bell are favorites, Crickets is named "Zebra"
$fixtures/shortcuts.json  the keys 1 (Air Horn), Q (Applause), and W (Bell)
$fixtures/background.png  a 512 by 384 picture, for --background
$fixtures/photo.jpg     a 4000 by 3000 photo, for app.set-background
$fixtures/background.json  background.png on Applause (purple) and Air Horn
                        (no color); needs --background $fixtures/background.png
LIST
