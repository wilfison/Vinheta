# The application (`src/application.rs`)

- `src/main.rs`: sets up gettext, registers the gresource, and starts `VinhetaApplication`.
- `src/application.rs`: `adw::Application` subclass; registers the `app.*` actions in `setup_gactions()`. It owns the `AudioEngine` (started in `startup`, dropped in `shutdown`), consumes its events with `glib::spawn_future_local`, and keeps the map from `PlaybackId` to the `Sound` being played. `app.toggle-sound` (parameter: the absolute path of a sound of the library) follows the trigger mode (`trigger` in `src/pads.rs` with the `trigger-mode` key): start, stop, restart, or start and stop the others. With `app.play-sound` (what the search uses: `play` in `src/pads.rs`, which starts or restarts a sound and never stops one), `app.stop-sound`, and `app.stop-all` it is the only way sounds are started and stopped; the pads activate them too. `app.preferences` opens the preferences dialog and `app.call-guide` the call setup guide. `app.send-sounds-to-call` and `app.include-my-voice` are stateful actions made from the settings keys (`create_action`), which the accelerators flip.

## Pad settings

- The application owns the pad settings (`PadStore`, loaded in `startup` from `pads.json` in `glib::user_data_dir()/vinheta`; a file that cannot be parsed is renamed to `pads.json.corrupt` and the user is told; when the rename fails nothing is saved in that session, so the damaged file is never overwritten; a failed save is told once per session). `update_sound` is the one way to change them: it updates the `Sound`, the store, and the running playback, and schedules a save (at most one write every 500 ms, flushed in `shutdown`). `app.toggle-loop`, `app.toggle-favorite`, and `app.reset-sound` (parameter: the path) and the sound dialog go through it; it also tells the window when a name or a favorite changed, because the sorted and filtered views do not watch their items. It keeps a pad key unique: a key given to a sound is taken from the entry that had it (`take_shortcut`), whose `Sound` and dialog are told. `app.set-shortcut` (parameter `(ss)`: the path and the key, empty for none) and `app.trigger-shortcut` (a key: what `app.toggle-sound` does for the sound that has it) are how scripts and the window reach the keys.

## Audio that is unavailable

- Audio that is unavailable: `start_audio` starts an engine with the current settings and returns whether it did; on a failure, and when the engine reports `ConnectionLost` or `PipeWire`, `set_audio_error` drops the engine and keeps the reason as an `AudioFailure`, from which the window picks the sentence of its banner (never the `Display` text of the engine, which only goes to the log). `app.retry-audio` (the "Try Again" button of the banner, enabled only while audio is unavailable) calls `start_audio` again; there is no automatic retry. Each engine has a generation number, so what an old one still reports is ignored.
- The engine tells its failures apart (`Unreachable`, `ConnectionLost`, `NodeExists`) and can be started again in the same process, which is what "Try Again" does.

## Notices

- Notices: `notify` shows a toast, or keeps it until there is a window (the pad file is read in `startup`, before the window exists; `activate` flushes them). A sticky notice stays until dismissed. `update_missing_devices` tells once when a chosen device goes from present to missing (after the engine listed its devices, and when the key changes), and `device_missing` is the lasting mark: the subtitle of the preferences rows, and the tooltip and `warning` class of the microphone icon of the bottom bar. `NoMicrophone` is told once per engine while "Include My Voice" is on.

## The mix

- The settings are the single source of truth for the mix: the application builds the engine `Config` from them and forwards every change of a key to the engine (`call-volume`, `monitor-volume`, `microphone`, `monitor-output`, `include-my-voice`, `send-sounds-to-call`, and `fade-out-on-stop`, which is a fade of 300 ms or none). The interface only binds widgets to keys and never calls the engine for these. The application also keeps the device lists reported by the engine and emits its `devices-changed` signal when they, or the audio availability, change.
- While a sound plays, one timer of the application (every 100 ms) asks the engine for the position of each playback and writes it to the `Sound`. It does not exist while nothing plays.

## The sounds folder

- The sounds folder is the `sounds-folder` key, or `sounds` inside the data directory of the app (`sounds_folder`). `app.trash-sound` moves a file to the system trash only when it is directly inside that folder; the "Move to Trash" item of a pad is hidden elsewhere.
- Changing the sounds folder moves nothing: the old folder stays as a normal tab.

## The audio editor

- `app.open-in-editor` (parameter: the path) opens the file of a sound in another app, for editing. There is no default audio editor in the system (the default app of an audio file is a player), so `audio_editor` picks one: the app of the `audio-editor` key (the name of a desktop file) while it is installed, otherwise the first audio editor among the apps registered for the formats of the library (`audio_apps`). `is_editor` of `src/editors.rs` says what an audio editor is: the category `AudioVideoEditing` or `X-AudioEditing`, with `Audio` and without `Video` (Kdenlive has the editing category too). Without one the action shows a toast.
- The editor writes the file of the user: nothing is copied. The folder page sees the change like any other, keeps the pad and its settings, and updates the modification time of its `Sound` (`sounds_modified` tells the window, for the "Recently Added" order).

## The call setup guide

- The application opens it in `activate` on the first run: see [interface.md](interface.md), "Dialogs".
