# Shortcuts

Shortcuts only work while the window has the focus.

## Pad keys

- A pad key is chosen per pad in its dialog and is unique in the whole library; keys are not given by position. Giving a key to a pad takes it from the pad that had it.
- Pad keys are letters and digits only, with no modifier. They act like a click (the trigger mode applies), from any tab, and never while a text field, a menu, or a dialog has the keys. A held key triggers once.
- Pad keys are handled by one `GtkEventControllerKey` of the window (`setup_pad_keys` in `window.rs`), in the bubble phase, so text fields, menus, and dialogs get their keys first. It ignores keys with Shift, Ctrl, Alt, or Super and any key while a dialog is open, and remembers which keys are down, because a held key repeats as ordinary presses. A key no pad has is not consumed.
- The search does not capture plain keys (there is no type-to-search), so they stay free for the pad keys.
- `app.set-shortcut` and `app.trigger-shortcut` are how scripts and the window reach the keys ([application.md](application.md), "Pad settings").

## Accelerators

- "Stop All", "Send Sounds to Call", and "Include My Voice": Ctrl+Shift+S, Ctrl+Shift+L, Ctrl+Shift+M. The last two flip stateful actions made from the settings keys.
- Ctrl+F opens the search, `Ctrl+,` the preferences, and Ctrl+Shift+Page Up and Page Down move the tab being shown.
- `shortcuts-dialog.ui` is loaded automatically by libadwaita from the `resource-base-path`, which provides the `app.shortcuts` action; that is why it does not appear in `setup_gactions()`.

## Global shortcuts

Global shortcuts through the `GlobalShortcuts` portal were built (a portal client over `gio`, a "Shortcuts" group in the preferences, a fake portal for the checks) and then removed: the first try on a real session failed. With the portal the user picks the key in a system dialog and the app can only suggest one.

What was measured on GNOME 50 (xdg-desktop-portal 1.21.1), in case they come back:

- GTK registers the app ID by itself at startup and ignores the answer.
- The portal only accepts the ID when a desktop file named after it is installed and its `Exec` program is found. The installed package qualifies, the local prefix does not. Otherwise it refuses the session with "An app id is required", unless the launcher has an app scope of its own.
- The system dialog comes with the suggested keys (`CTRL+SHIFT+s`) filled in. "Add" answers 0 with a text such as "Press <Shift><Control>s" per shortcut, "Cancel" answers 2. An accepted list is granted again with no dialog.
- `ConfigureShortcuts` is not implemented.
- A real global key press cannot be simulated on Wayland, so that part is always a manual check.
