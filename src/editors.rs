/* editors.rs
 *
 * Copyright 2026 wilfison
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Which installed app opens a sound for editing. No GTK types, so it can
//! be tested.

/// An installed app that opens the formats of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The name of its desktop file, which is what the settings store.
    pub id: String,
    pub name: String,
    /// The `Categories` of its desktop file.
    pub categories: Vec<String>,
}

/// Whether the app edits audio. Video editors have the editing category
/// too, with "Video" or without "Audio".
pub fn is_editor(app: &App) -> bool {
    let has = |category: &str| app.categories.iter().any(|other| other == category);
    let editing = has("AudioVideoEditing") || has("X-AudioEditing");
    editing && has("Audio") && !has("Video")
}

/// The app that opens a sound: the chosen one while it is installed,
/// otherwise the first audio editor.
pub fn pick<'a>(apps: &'a [App], chosen: &str) -> Option<&'a App> {
    let found = apps
        .iter()
        .find(|app| !chosen.is_empty() && app.id == chosen);
    found.or_else(|| apps.iter().find(|app| is_editor(app)))
}

/// The apps a selector offers after its "Automatic" entry, the editors
/// first, and the position of the selected entry (0 is "Automatic").
pub fn selector_entries(apps: &[App], chosen: &str) -> (Vec<App>, usize) {
    let (editors, others): (Vec<_>, Vec<_>) = apps.iter().cloned().partition(is_editor);
    let entries: Vec<App> = editors.into_iter().chain(others).collect();
    let found = entries.iter().position(|app| app.id == chosen);
    let selected = found.filter(|_| !chosen.is_empty()).map_or(0, |at| at + 1);
    (entries, selected)
}

#[cfg(test)]
mod tests;
