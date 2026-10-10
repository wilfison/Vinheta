# Checks

Run `scripts/check.sh` before committing. It prints one `PASS` or `FAIL` line per check and keeps the logs in `tmp/check/`.

```sh
scripts/check.sh           # rustfmt, clippy without warnings, a single glib version, no em dash, one version, complete translations, meson test
scripts/check.sh --audio   # plus the audio engine harness (fake devices, about 4 minutes, skipped when nothing it tests changed)
scripts/check.sh --app     # plus scripts/verify-app.sh on the installed app
scripts/check.sh --deb     # plus the package build (last)
scripts/check.sh --all
scripts/check.sh --all --force   # runs the audio harness even when nothing it tests changed
scripts/check.sh --only REGEX    # only the checks whose label matches (fails when none does)
```

- `scripts/verify-app.sh` checks the installed app end to end on a virtual display: playing and stopping through actions and through real clicks, the "Send sounds to call" switch, the call volume, and the limiter of the call ("Limit Call Level", with a 0 dBFS tone played with the monitor volume at 0) measured on what a fake call app records, the app the sounds are sent to, the monitor output (with a fake sink), the pad volume and loop, the trigger modes, a file removed while it plays, the settings of a renamed file, the saved pad settings, the settings of a deleted file dropped at the start, a pad key pressed for real, audio coming back after "Try Again" and by itself after a lost connection, and the cleanup on exit. It uses the real PipeWire and plays a quiet tone on the default output for a few seconds. The fake call app is a `pw-record` of a silent fake microphone, and `call-target` is set to it, so no real app and no real microphone takes part. "Try Again" is reached with a private PipeWire instance that is started after the app; the lost connection, with a private instance that is stopped while the app uses it and started again.
- While working on one behavior, `scripts/verify-app.sh --only REGEX` runs only the sections whose title matches (`--list` prints the titles); the start and the exit of the app are always checked. Every section starts and stops what it needs, so a new section must not depend on the one before it. Run all of it before committing.
- The audio harness is skipped (`SKIP` instead of `PASS`) when what it tests is the same as at its last pass on this machine: the files of `src/audio/`, the test binary, `Cargo.toml` and `Cargo.lock`, the harness scripts, and the versions of `rustc`, PipeWire, WirePlumber, and GStreamer (`audio_inputs` in `scripts/check.sh`; the fingerprint of the last pass is in `tmp/check/audio-harness.passed`). The engine uses no other module of the crate; if it starts to, add that module to `audio_inputs`.
- `scripts/check.sh --all` takes about 6 minutes when the audio harness runs and about a minute and a half when it is skipped. The harness and then the app check take most of it, and they cannot run at the same time (both create the `vinheta` node). The package builds last, after every check that uses PipeWire: next to the build, the graph stopped scheduling the nodes of the harness and of the app check three times (2026-10-10), even at the lowest priority. A TERM or a Ctrl-C stops `check.sh` and the check it is running at once, and the harness ends by itself with a `FAIL` when a playback lasts over a minute (a stuck graph: see `pw-top`). An agent whose commands time out before that runs it in the background and reads `tmp/check/` or its output when it ends.
- The code is formatted with `cargo fmt` (default settings); the check fails when it would change something.
- `scripts/build-deb.sh` builds the package from a copy under `tmp/deb`, so nothing is written to the working tree or to its parent directory.
- `scripts/version.sh` prints the version when its four places agree (see [packaging.md](packaging.md)), and `scripts/check-translations.sh` fails when a file with strings is missing from `po/POTFILES.in`, when a `gettext` call of a `.rs` file did not reach the template, or when a language of `po/LINGUAS` is incomplete. Both are checks of `scripts/check.sh`.
- GitHub Actions runs the checks of `scripts/check.sh --deb` as steps of their own (`scripts/check.sh --only LABEL`), after `scripts/ci.sh --setup`, and `scripts/ci-container.sh` rehearses the whole gate in a clean container: see [packaging.md](packaging.md). A check added to `scripts/check.sh` needs its step in `.github/workflows/ci.yml`. The audio and app checks need a PipeWire session and a display, so they never run on the CI.
- `scripts/dev-common.sh` holds what these scripts share (the local prefix environment, the virtual session, test sounds). New development scripts should source it from bash (it refuses any other shell, where it would compute the wrong root).
- Scripted checks never touch the user's settings or pad settings: `virtual_session` points `XDG_CONFIG_HOME` at its own directory and `XDG_DATA_HOME` at the `data` directory inside it.

The audio engine has its own harness, `scripts/verify-audio.sh`, described in [audio.md](audio.md), "Running it".

## Checking the interface

`scripts/screenshot.sh` runs the installed app on a virtual display, with its own D-Bus session and its own settings, and captures the window:

```sh
meson install -C build
scripts/screenshot.sh grid --folder DIR --action "toggle-sound '/abs/path/sound.wav'"
```

- It writes `tmp/screenshots/NAME.png`. Screenshots are temporary: read them, then delete them (`scripts/screenshot.sh --clean`).
- `scripts/fixtures.sh` creates the folders these checks use in `tmp/fixtures` and prints them: `Fixture` (three silent pads), `Palette` with `palette.json` (one pad per color), `Loop` with `loop.json` (a 3 second pad that loops), `dialog.json` (one pad with every setting), `Many` (60 pads, enough to scroll), `Effects` (three pads, so that a search for "app" has results in two folders), `favorites.json` (two favorites in two folders and a pad named "Zebra"), and `shortcuts.json` (the keys 1 on "Air Horn", Q on "Applause", and W on "Bell" of `Effects`), `Broken` (one file that holds text, so playing it fails), `corrupt-pads.json` (not valid JSON, for `--pads`), `background.png` (a 512 by 384 picture), `photo.jpg` (a 4000 by 3000 photo, for `app.set-background`), `background.json` (`background.png` on "Applause", which is purple, and on "Air Horn"; it needs `--background`), and `crop.json` (the same, with the bottom right quarter of the picture as the crop of "Applause").
- A check that creates, renames, or deletes files works on a copy of a fixture (for example under `tmp/monitor-test`), never in `tmp/fixtures`.
- The virtual display has no window manager: the content of the window is 10 pixels narrower than the size given to `--size`. Use `--size 370,700` for the narrow layout at 360 pixels (dialogs need exactly 360).
- A scripted import lands in `tmp/screenshot-config/data/vinheta/sounds`, and a trashed file in `tmp/screenshot-config/data/Trash`.
- `--folder` fills the library, `--setting 'KEY VALUE'` sets any other key of the app's settings before it starts (`VALUE` is a GVariant: `0.5`, `false`, `"'name'"`), `--no-audio` makes the engine fail to start, `--light` uses the light style, `--lang pt_BR` runs the app in that language (every scripted session is in English otherwise, whatever the language of the user).
- Every run starts with `call-guide-shown` set to true, so the call setup guide does not open; `--first-run` leaves the key at its default.
- `--private-pipewire` starts a PipeWire instance of the script's own (no session manager, no devices) and points the app and the fake devices at it; the steps `--stop-pipewire` and `--start-pipewire` kill and start it while the app runs. It is how a lost connection is reached. A pad shows as playing at 00:00 there, and `--expect-playing` does not work with it.
- `--pads FILE` gives the app a pad settings file to start with: `{"version": 1, "pads": {"/abs/sound.wav": {"name": "Intro", "color": "purple", "volume": 0.8, "loop": true}}}`. The one the app wrote during the run stays in `tmp/screenshot-config/data/vinheta/pads.json`. `--background FILE` copies an image into the backgrounds directory of the run, keeping its name, so that the pad file can name it (`"background": "background.png"`).
- Steps run in the given order before the capture: `--action` activates an `app.*` or `win.*` action, `--click X,Y`, `--right-click X,Y`, and `--key KEYS` simulate the user (`xdotool`), `--drop 'X,Y FILE[;FILE]...'` drags the files from a window of a helper (a GTK drag source, as a file manager is) and drops them there, `--size W,H` resizes the window, `--wait SECONDS` waits, `--restart` quits the app and starts it again with the same settings (to check what is restored), `--exec COMMAND` runs a shell command (a status other than 0 is reported as a failed step).
- A step can check instead of showing: `--expect-setting 'KEY VALUE'` (the value as `gsettings get` prints it: `false`, `0.5`, `'text'`) and `--expect-playing N` (how many sounds play) wait up to 2 seconds and make the script exit with 1 when they fail. Prefer them to reading a picture or the debug output for anything that is a value.
- `--key` reaches the app with no click before it; to hold a key use `--exec "xdotool keydown q; sleep 1; xdotool keyup q"`. The display is 1100 by 800: scroll a tall dialog with `--exec 'xdotool mousemove 500 400 click --repeat 12 --delay 30 5'`.
- The scripts are bash. From zsh an unquoted `$VAR` holding several options is passed as one word: put a sequence of runs in a bash script (under `tmp/`) instead of typing it.
- One run can take several pictures: the step `--capture NAME` writes `tmp/screenshots/NAME.png` at that point, and `--crop WxH+X+Y` crops the captures that follow (`--crop full` undoes it). Prefer that to starting the app once per state. `--sheet` also writes `NAME-sheet.png`, every capture of the run stacked, to review them in one look.
- Warnings and criticals logged by the app are printed at the end of the run; treat them as findings. `--debug` also prints the app's debug messages (for example how long starting a sound took).
- Dialogs opened by an action (`--action preferences`) and open popovers are captured.
- The device lists come from the real PipeWire, so they differ between machines. For a known entry use a fake one: `--fake-sink 'NODE DESCRIPTION'` creates an output and `--fake-app 'NAME DESCRIPTION'` starts a call app (a recorder of a silent fake microphone) before the app starts, the steps `--plug-sink`, `--plug-app`, and `--unplug NODE` do it while the app runs. Name them `vinheta-shot-*`. The script removes them when it ends. A fake app needs the session manager, so it does not show with `--private-pipewire`.
- Portal dialogs (the folder chooser) do not work on the virtual display and cannot be captured; test them by hand.
- The app still uses the real PipeWire: it plays on the default output and into the real apps that are recording a microphone. Use silent files with `toggle-sound`, or set `call-target` to a fake app.
- It needs `xvfb-run`, `dbus-run-session`, `xdotool`, and ImageMagick (`import`), which are development tools only.

## Limits of the scripted checks

- The app sees the apps installed in the system, so `app.open-in-editor` in a scripted session would start a real editor. Run it with a fake one: a desktop file (with `MimeType=audio/x-wav;` and the categories `Audio;AudioVideoEditing;`) in an `applications` directory of its own, `update-desktop-database` on that directory (GIO reads the types from `mimeinfo.cache`), and `XDG_DATA_DIRS=DIR/share:/usr/share` in front of the script, which also hides the Flatpak apps.
- Two engines can run at the same time (each links only the streams of its own process), but the checks look for `vinheta-call-*` and `vinheta-drain-*` nodes of any process: close the app, and do not run two checks that start it (screenshots, `verify-app.sh`, the audio harness) at the same time.
- The fallback of a removed or missing device is the real default device. A check of that fallback must play nothing audible (monitor volume at 0, or a silent file) and record nothing from the real microphone.
- With every app as the target, which is the default, a sound reaches the real apps that are recording. A check that plays something audible sets the target to a fake call app first (`call-target`, or `--target` of the test binary); the case of every app is checked on the links, with the call volume at 0.
- A level is measured on what a fake call app records from a silent fake microphone, so the real microphone never hides a quiet tone.
- `loud_sound` of `scripts/dev-common.sh` makes a 0 dBFS tone, for the limiter. It plays only with the monitor volume at 0 and a fake call app as the target.
- Real clicks on a list entry and typing are not simulated reliably. Check the state (the setting, the action) and test the gesture by hand. A click on the switch of an `AdwSwitchRow` is reliable (`--click X,Y` then `--expect-setting`). A mouse drag inside the app is real when it is one `--exec` step with the pointer moved in steps: `xdotool mousemove X Y mousedown 1 sleep 0.2 mousemove X2 Y sleep 0.2 mouseup 1` moved the volume slider of the pad dialog and the picture of "Adjust Background" (2026-10-10); check the result on a value (the pad file), not only on a picture. The wheel is `xdotool click 4` and `click 5`. A drop of files from outside the app is real with `--drop` (`drop_files` of `scripts/dev-common.sh`).

## Tested by hand only

- The folder and file choosers ("Add Folder…", "Add Sounds…", "Locate Folder…", choosing the sounds folder, choosing the background of a pad), and the button that opens the sounds folder.
- The "Undo" of a removed tab.
- A real audio editor: "Open in Audio Editor", exporting over the file (the pad keeps its settings and moves in the "Recently Added" order), and picking an entry of the "Audio Editor" row.
- Dropping from a real file manager (the scripted drop uses a helper window).
- Dragging a volume slider with the mouse, in the bottom bar and in the pad dialog while the sound plays, and the controls of the narrow bottom bar.
- Switching between two real outputs.
- In the pad dialog: typing a name, clicking a swatch, and listening for a key with the keyboard only (Tab to the button, Enter, a key, Escape, Backspace).
- The long press and the Menu key on a pad.
- A touchpad scroll and a touch drag on the picture of "Adjust Background" (the mouse wheel and a mouse drag are scripted).
- Picking an entry of the trigger mode row.
- Typing in the search and pressing Enter, and typing a tab name.
- A keyboard layout where digits need Shift.
- The blink of a located pad with animations turned off in the system.
- How a loud sound sounds on a real call with the limiter on and off (step 9 of the manual call checklist of [audio.md](audio.md)). The click on the "Limit Call Level" switch is scriptable: `--click` on the switch, then `--expect-setting 'limit-call-level false'`.
- A real call: the manual call checklist of [audio.md](audio.md).
