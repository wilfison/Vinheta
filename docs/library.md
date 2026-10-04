# The library (`src/library.rs`, `src/pads.rs`)

## Model

- The user adds **directories**; each directory becomes a **tab**.
- Every audio file in the directory is shown as a pad. Nothing is copied: the app reads the files where they are, and follows the directory while it runs.
- The list of directories (and the tab order) is stored in GSettings, and so are the labels the user gave to tabs.
- Per-pad metadata (name, color, volume, loop, favorite, shortcut) is stored as JSON in `~/.local/share/vinheta`, keyed by file path. It follows a file renamed inside a library directory or moved between two of them.
- Loose files (dropped, or picked through "Add Sounds…") are always copied into the **sounds folder** ("Sounds Folder" in the preferences, `~/.local/share/vinheta/sounds` by default), which is itself a library directory and therefore a tab. Only these copies can be moved to the trash from the app.

## Modules

- `src/library.rs`: which files of a folder are sounds, what changed between two scans (`diff`), the name a pad shows for a file (`humanize`: separators become spaces, first letter uppercase), and the import of loose files (`import`: a copy under a hidden temporary name, never overwriting, with `std::fs::copy` so that the copy has a fresh modification time). No GTK types, covered by unit tests.
- `src/pads.rs`: the pad settings (`PadSettings`, `PadColor`, `PadStore` with the JSON file), the pad keys (`shortcut_key`, and the rule that a key belongs to one entry, applied by `take_shortcut` and on load), the trigger rule (`TriggerMode`, `trigger`, and `play` for the search), the search match (`matches`), the sort order (`SortOrder`, `compare`), the move of the settings of a folder (`move_folder`), and the time format of a playing pad. No GTK types, covered by unit tests.

## Following a folder

- The monitor events are not trusted as a complete log: any event schedules a rescan of the folder (200 ms later) and a diff. The page of a folder does this (`src/ui/folder_page.rs`, see [interface.md](interface.md)).
- A rename is shown as a removed pad and a new one with the same settings.
- A file that goes away while it plays is stopped.
- Only files directly inside a folder are listed; subfolders are not entered.
- A file copied by a file manager becomes a pad before the copy is complete; triggering it too early can fail with the "Could not play" toast. The app's own imports use a hidden temporary name.
- `GtkGridView` handles a folder with hundreds of files. Anything read per file (a duration, for example) must be asynchronous.

## The pad file

`pads.json` in the data directory of the app (`~/.local/share/vinheta`):

```json
{"version": 1, "pads": {"/abs/sound.wav": {"name": "Intro", "color": "purple", "volume": 0.8, "loop": true}}}
```

- The settings are keyed by the absolute path of the file. Fields that are not known are ignored, and the file has a `version`.
- A pad key is one more field of the entry, and belongs to one entry only ([shortcuts.md](shortcuts.md)).
- The volume is a slider position from 0 to 1 (no amplification), with the same cubic curve as the bottom bar.
- A file moved to a folder that is not in the library loses the link to its settings: they stay under the old path and are used again if the file comes back. The same holds for a trashed file.
- The sounds folder lives in the same data directory by default, so nothing there may collide with `pads.json` and `pads.json.corrupt`.
- What happens to a file that cannot be parsed or saved is in [application.md](application.md), "Pad settings".

## Tab names

- Tab names are the `folder-names` key, keyed by the path of the folder: a folder moved on disk starts again without a name, unless it is moved with "Locate Folder…".
