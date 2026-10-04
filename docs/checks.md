# Checks

Run `scripts/check.sh` before committing. It prints one `PASS` or `FAIL` line per check and keeps the logs in `tmp/check/`.

```sh
scripts/check.sh           # rustfmt, clippy without warnings, a single glib version, no em dash, one version, complete translations, meson test
scripts/check.sh --audio   # plus the audio engine harness (fake devices, about 4 minutes)
scripts/check.sh --app     # plus scripts/verify-app.sh on the installed app
scripts/check.sh --deb     # plus the package build (in the background, while the other checks run)
scripts/check.sh --all
```

- `scripts/verify-app.sh` checks the installed app end to end on a virtual display: the virtual microphone, playing and stopping through actions and through real clicks, the "Send sounds to call" switch and the call volume measured on a recording, the voice switch, the monitor output (with a fake sink), the pad volume and loop, the trigger modes, a file removed while it plays, the settings of a renamed file, the saved pad settings, a pad key pressed for real, audio coming back after "Try Again", and the cleanup on exit. It uses the real PipeWire and plays a quiet tone on the default output for a few seconds. The levels are measured with the voice off, because a noisy real microphone hides the tone.
- While working on one behavior, `scripts/verify-app.sh --only REGEX` runs only the sections whose title matches (`--list` prints the titles); the start and the exit of the app are always checked. Every section starts and stops what it needs, so a new section must not depend on the one before it. Run all of it before committing.
- `scripts/check.sh --all` takes about 6 minutes, most of it the audio harness and then the app check, which cannot run at the same time (both create the `vinheta` node). The package builds alongside them at the lowest priority (`nice -n 19`), so it does not disturb the timed audio checks, and its result is printed last. An agent whose commands time out before that runs it in the background and reads `tmp/check/` or its output when it ends.
- The code is formatted with `cargo fmt` (default settings); the check fails when it would change something.
- `scripts/build-deb.sh` builds the package from a copy under `tmp/deb`, so nothing is written to the working tree or to its parent directory.
- `scripts/version.sh` prints the version when its four places agree (see [packaging.md](packaging.md)), and `scripts/check-translations.sh` fails when a file with strings is missing from `po/POTFILES.in`, when a `gettext` call of a `.rs` file did not reach the template, or when a language of `po/LINGUAS` is incomplete. Both are checks of `scripts/check.sh`.
- `scripts/ci.sh` is what GitHub Actions runs (`scripts/check.sh --deb`), and `scripts/ci-container.sh` rehearses it in a clean container: see [packaging.md](packaging.md). The audio and app checks need a PipeWire session and a display, so they never run on the CI.
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
- `scripts/fixtures.sh` creates the folders these checks use in `tmp/fixtures` and prints them: `Fixture` (three silent pads), `Palette` with `palette.json` (one pad per color), `Loop` with `loop.json` (a 3 second pad that loops), `dialog.json` (one pad with every setting), `Many` (60 pads, enough to scroll), `Effects` (three pads, so that a search for "app" has results in two folders), `favorites.json` (two favorites in two folders and a pad named "Zebra"), and `shortcuts.json` (the keys 1 on "Air Horn", Q on "Applause", and W on "Bell" of `Effects`), `Broken` (one file that holds text, so playing it fails), and `corrupt-pads.json` (not valid JSON, for `--pads`).
- A check that creates, renames, or deletes files works on a copy of a fixture (for example under `tmp/monitor-test`), never in `tmp/fixtures`.
- The virtual display has no window manager: the content of the window is 10 pixels narrower than the size given to `--size`. Use `--size 370,700` for the narrow layout at 360 pixels (dialogs need exactly 360).
- A scripted import lands in `tmp/screenshot-config/data/vinheta/sounds`, and a trashed file in `tmp/screenshot-config/data/Trash`.
- `--folder` fills the library, `--setting 'KEY VALUE'` sets any other key of the app's settings before it starts (`VALUE` is a GVariant: `0.5`, `false`, `"'name'"`), `--no-audio` makes the engine fail to start, `--light` uses the light style, `--lang pt_BR` runs the app in that language (every scripted session is in English otherwise, whatever the language of the user).
- Every run starts with `call-guide-shown` set to true, so the call setup guide does not open; `--first-run` leaves the key at its default.
- `--private-pipewire` starts a PipeWire instance of the script's own (no session manager, no devices) and points the app and the fake devices at it; the steps `--stop-pipewire` and `--start-pipewire` kill and start it while the app runs. It is how a lost connection and a system with no microphone are reached. A pad shows as playing at 00:00 there, and `--expect-playing` does not work with it.
- `--pads FILE` gives the app a pad settings file to start with: `{"version": 1, "pads": {"/abs/sound.wav": {"name": "Intro", "color": "purple", "volume": 0.8, "loop": true}}}`. The one the app wrote during the run stays in `tmp/screenshot-config/data/vinheta/pads.json`.
- Steps run in the given order before the capture: `--action` activates an `app.*` or `win.*` action, `--click X,Y`, `--right-click X,Y`, and `--key KEYS` simulate the user (`xdotool`), `--size W,H` resizes the window, `--wait SECONDS` waits, `--restart` quits the app and starts it again with the same settings (to check what is restored), `--exec COMMAND` runs a shell command (a status other than 0 is reported as a failed step).
- A step can check instead of showing: `--expect-setting 'KEY VALUE'` (the value as `gsettings get` prints it: `false`, `0.5`, `'text'`) and `--expect-playing N` (how many sounds play) wait up to 2 seconds and make the script exit with 1 when they fail. Prefer them to reading a picture or the debug output for anything that is a value.
- `--key` reaches the app with no click before it; to hold a key use `--exec "xdotool keydown q; sleep 1; xdotool keyup q"`. The display is 1100 by 800: scroll a tall dialog with `--exec 'xdotool mousemove 500 400 click --repeat 12 --delay 30 5'`.
- The scripts are bash. From zsh an unquoted `$VAR` holding several options is passed as one word: put a sequence of runs in a bash script (under `tmp/`) instead of typing it.
- One run can take several pictures: the step `--capture NAME` writes `tmp/screenshots/NAME.png` at that point, and `--crop WxH+X+Y` crops the captures that follow (`--crop full` undoes it). Prefer that to starting the app once per state. `--sheet` also writes `NAME-sheet.png`, every capture of the run stacked, to review them in one look.
- Warnings and criticals logged by the app are printed at the end of the run; treat them as findings. `--debug` also prints the app's debug messages (for example how long starting a sound took).
- Dialogs opened by an action (`--action preferences`) and open popovers are captured.
- The device lists come from the real PipeWire, so they differ between machines. For a known entry use a fake device: `--fake-mic 'NODE DESCRIPTION'` and `--fake-sink 'NODE DESCRIPTION'` create one before the app starts, the steps `--plug-mic`, `--plug-sink`, and `--unplug NODE` do it while the app runs. Name the nodes `vinheta-shot-*`. The script destroys them when it ends.
- Portal dialogs (the folder chooser) do not work on the virtual display and cannot be captured; test them by hand.
- The app still uses the real PipeWire: it creates the real "Vinheta" node for a few seconds and plays on the default output. Use silent files with `toggle-sound`.
- It needs `xvfb-run`, `dbus-run-session`, `xdotool`, and ImageMagick (`import`), which are development tools only.

## Limits of the scripted checks

- The app sees the apps installed in the system, so `app.open-in-editor` in a scripted session would start a real editor. Run it with a fake one: a desktop file (with `MimeType=audio/x-wav;` and the categories `Audio;AudioVideoEditing;`) in an `applications` directory of its own, `update-desktop-database` on that directory (GIO reads the types from `mimeinfo.cache`), and `XDG_DATA_DIRS=DIR/share:/usr/share` in front of the script, which also hides the Flatpak apps.
- If another Vinheta instance is open, the engine fails because the node name is taken. For the same reason two checks that start the app (screenshots, `verify-app.sh`) cannot run at the same time.
- The fallback of a removed or missing device is the real default device. A check of that fallback must play nothing audible (monitor volume at 0, or a silent file) and record nothing from the real microphone.
- A noisy real microphone hides quiet test tones on the virtual microphone; turn "Include My Voice" off before measuring.
- Real clicks on a list entry, drags, and typing are not simulated reliably. Check the state (the setting, the action) and test the gesture by hand.

## Tested by hand only

- The folder and file choosers ("Add Folder…", "Add Sounds…", "Locate Folder…", choosing the sounds folder), and the button that opens the sounds folder.
- The "Undo" of a removed tab.
- A real audio editor: "Open in Audio Editor", exporting over the file (the pad keeps its settings and moves in the "Recently Added" order), and picking an entry of the "Audio Editor" row.
- Dropping files and folders on the window, and the drop highlight.
- Dragging a volume slider with the mouse, in the bottom bar and in the pad dialog while the sound plays, and the controls of the narrow bottom bar.
- Switching between two real outputs.
- In the pad dialog: typing a name, clicking a swatch, and listening for a key with the keyboard only (Tab to the button, Enter, a key, Escape, Backspace).
- The long press and the Menu key on a pad.
- Picking an entry of the trigger mode row.
- Typing in the search and pressing Enter, and typing a tab name.
- A keyboard layout where digits need Shift.
- The blink of a located pad with animations turned off in the system.
- A real call: the manual call checklist of [audio.md](audio.md).
