# The interface (`src/ui/`)

- `src/ui/`: the interface components. A widget's `.rs` file lives here next to its `.ui` template, together with the `.ui` and `.css` files that have no Rust side. `application.rs` and `sound.rs` are not widgets and stay in `src/`.
- UI is declared in XML (`.ui`), not built in code. The mockups in `mockups/` are the visual reference; [overview.md](overview.md) lists what differs from them.

## The window

- `src/ui/window.rs` + `src/ui/window.ui`: `adw::ApplicationWindow` subclass using a composite template; widgets from the `.ui` file are bound with `#[template_child]`. It owns the tabs (`AdwViewStack` with an `AdwInlineViewSwitcher`), the playing counter next to them, the `directories` setting (always written from the pages, in tab order), and the bottom bar. Without audio the pads are dimmed, not insensitive, so their menu still works.
- The window owns what every view of pads shares: one `GtkCustomSorter` (`compare` in `src/pads.rs` with the `sort-order` key, exposed as `win.sort-order`), and a `GtkFlattenListModel` over the stores of the folder pages, in tab order. The "Favorites" tab (page name `favorites`, always last, hidden while empty) and the search results are a `GtkFilterListModel` of it, sorted by the same sorter, so every view shows the same `Sound` objects.
- The window does not know its application while it is constructed (`application` is set afterwards); `window.rs` uses the default application.
- When the engine cannot start, the window opens with a banner that says why and has "Try Again"; playback failures are toasts.

## Tabs and folder pages

- Tabs: the title is the entry of the `folder-names` key, or the name of the directory. `win.rename-folder` (an `AdwAlertDialog` from `rename-dialog.ui`), `win.move-folder-left`, `win.move-folder-right`, and `win.remove-folder`, `win.find-folder` (the folder chooser of "Locate Folder…"), and `win.relocate-folder` (a path: the tab is replaced by a page of that folder at the same position, and its name and its pad settings move with it, through `PadStore::move_folder`) act on the folder tab being shown and are disabled for "Favorites" and during a search. The stack can only append, so reordering removes and adds pages again (`set_pages`); the pages are the same objects.
- `src/ui/folder_page.rs` + `src/ui/folder-page.ui`: the content of one tab. It owns the unsorted store of the sounds of its folder and shows it sorted in a `SoundGrid`, or a status page when the folder is empty or missing. It watches the folder (`gio::FileMonitor` with `WATCH_MOVES`): any event schedules a rescan off the main thread 200 ms later, and `diff` of `src/library.rs` says which `Sound` objects to add and remove (the others are kept, so a playing pad keeps playing). The events that carry both paths (a rename, a move between watched folders) move the pad settings first.
- Tabs are an `AdwViewStack` with an `AdwInlineViewSwitcher`, which needs the `v1_7` feature of the `libadwaita` crate. The switcher scrolls horizontally when the tabs do not fit.
- A folder is removed from the primary menu ("Remove Folder"), with an "Undo" toast. Removing a tab only removes the folder from the library, never the files.
- Tabs are moved with actions of the primary menu (Ctrl+Shift+Page Up and Page Down), not dragged. A tab name is a label; the folder on disk keeps its name.
- "Locate Folder…" on the "Folder Not Found" page moves a tab, its name, and its pad settings to another folder.

## Pads

- `src/ui/sound_grid.rs` + `src/ui/sound-grid.ui`: the grid of pads over any list model of `Sound`, used by the folder pages, the "Favorites" tab, and the search results. It activates an action with the path of the pad (`app.toggle-sound` unless told otherwise), handles the Menu key (the focus is on the grid cell), and locates a sound (scroll and blink).
- `src/sound.rs`: the `Sound` GObject: path, name (from the file), `playing`, what the user set (`display-name`, `color`, `volume`, `looping`, `favorite`, `shortcut`), the modification time of the file, and the position while it plays (`elapsed` and `duration` in milliseconds, -1 while unknown). Only the application writes the settings and the position. `src/ui/sound_pad.rs` + `src/ui/sound-pad.ui`: the widget of one pad, which follows the properties of its `Sound` and owns the context menu (its `pad.*` actions forward to the `app.*` and `win.*` ones with the path). A favorite pad shows a star, and a pad with a key shows it as a badge (the `keycap` class, shared with the dialog). `blink` highlights the pad for 1.2 seconds (the `located` class, a CSS animation in the text color of the window).
- `src/ui/sound_dialog.rs` + `src/ui/sound-dialog.ui`: the `AdwDialog` that edits one pad (name, color, volume, loop, key). While its "Shortcut" row listens, a key controller in the capture phase takes every key, so Escape ends the listening instead of closing the dialog. Every control acts at once through `update_sound`, and the dialog follows changes made elsewhere.
- `src/ui/style.css`: custom styling, loaded by libadwaita from the `resource-base-path`. The palette is one class per color (`.pad.blue`, ...) built from the libadwaita palette variables; the dark style overrides sit in a `prefers-color-scheme` media query (`style-dark.css` is deprecated).
- The color comes from a fixed palette of seven colors plus "no color", stored by name. There is no free color picker.
- Times are shown only while a pad plays. Idle pads show no duration.
- The trigger mode (overlap, restart, or stop the others) is one global setting, not a per-pad one. "Stop" is always in the context menu, since in the "Restart" mode a click no longer stops a pad.
- A favorite is a star on the pad, flipped from its context menu. The "Favorites" tab is last in the row and hidden while there is no favorite.
- "Move to Trash" is in the context menu only for the copies in the sounds folder. There is no confirmation and no undo in the app: the system trash is the undo.

## Search and sorting

- Search: the state is the `GtkSearchBar` being open plus the text of its entry (`win.search-mode` for the button and Ctrl+F, `win.search` with a text for scripts). While there is text the results replace the tab row and the tabs. A result activates `win.activate-result`: `app.play-sound`, close the search, `win.locate-sound` (show the tab, `scroll_to`, blink the pad). The search bar captures no keys of the window.
- The search covers every folder and replaces the tabs with one grid of results. Activating a result plays the sound, never stops it, closes the search, shows the tab of the sound, scrolls to its pad, and blinks it twice.
- Sorting is one global setting with two orders: "Name" (the shown name) and "Recently Added" (the modification time of the file).

## Loose files

- Loose files: `win.import-files` (a list of paths) is the one entry point for "Add Sounds…" (`win.add-sounds`), the drop target of the window, and scripts. Folders are added to the library; files go through `import` of `src/library.rs` off the main thread, and the sounds folder is added as a tab (titled "Sounds" until renamed).
- A dropped folder is added as a library folder, not copied.

## The bottom bar and the device selectors

- `src/ui/device_selector.rs`: not a widget. `bind` keeps a `GtkDropDown` or an `AdwComboRow` in sync with a device list of the application and with the key that stores the chosen device. It is the only place that writes `microphone` and `monitor-output`. A change of the key rebuilds the list in an idle callback, never at once: the key may be changing from inside the activation of an entry, and replacing the model there makes GTK log a critical.
- `src/devices.rs`: the entries of a device selector (system default, devices, a chosen device that is not connected), and whether a chosen device is missing. No GTK types, covered by unit tests.
- The sliders go from 0% to 100% (no amplification) with a cubic curve from position to gain. The settings store the position.
- The monitor selector lives only in the preferences; the microphone selector is in the bottom bar and in the preferences.
- A chosen device that is not connected is shown as "(unavailable)" and replaced by the system default until it returns. For the monitor WirePlumber does that by itself; for the microphone the engine does.
- The microphone selector never offers the "Vinheta" node. Other virtual sources are offered as microphones.

## Dialogs

- `src/ui/preferences_dialog.rs` + `src/ui/preferences-dialog.ui`: the `AdwPreferencesDialog`.
- `Ctrl+,` opens the preferences. The "Sounds Folder" row has two buttons: one opens the folder, one chooses another.
- The call setup guide (`src/ui/call_guide_dialog.rs` + `call-guide-dialog.ui`) opens by itself in `activate` while the `call-guide-shown` key is false and audio works; the key is set when the dialog closes, however it closes.
- The shortcuts dialog and the pad keys are described in [shortcuts.md](shortcuts.md).

## Narrow layout

- Narrow layout: an `AdwBreakpoint` (`max-width: 780px`) switches the bottom bar, an `AdwMultiLayoutView`, to its stacked layout and raises the minimum height; its `apply` and `unapply` signals toggle the `narrow` CSS class, which makes the pads and the paddings smaller. The class `bottom-bar` is on both the bar of the toolbar view and its content, so its padding counts twice.
- In the narrow layout the pads are 136 pixels wide instead of 168, so that two columns fit at 360 pixels. The minimum size of the window is 360 by 294, and 360 by 382 in the narrow layout.
- On the virtual display of the checks a dialog needs a content of exactly 360 pixels.
