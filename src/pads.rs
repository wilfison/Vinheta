/* pads.rs
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

//! What the user set for each pad, the file that stores it, the trigger
//! rule, and the time format of a playing pad. No GTK types.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;

const VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadColor {
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    Purple,
    Brown,
}

impl PadColor {
    pub const ALL: [Self; 7] = [
        Self::Blue,
        Self::Green,
        Self::Yellow,
        Self::Orange,
        Self::Red,
        Self::Purple,
        Self::Brown,
    ];

    /// The name used in the file and as the CSS class.
    pub fn name(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Orange => "orange",
            Self::Red => "red",
            Self::Purple => "purple",
            Self::Brown => "brown",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.name() == name)
    }
}

impl Serialize for PadColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

fn is_full(volume: &f64) -> bool {
    *volume == 1.0
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PadSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<PadColor>,
    /// A slider position, 0.0 to 1.0.
    #[serde(skip_serializing_if = "is_full")]
    pub volume: f64,
    #[serde(rename = "loop", skip_serializing_if = "is_false")]
    pub looping: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub favorite: bool,
    /// The key that triggers the pad: a lower case letter or a digit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<char>,
}

impl Default for PadSettings {
    fn default() -> Self {
        Self {
            name: None,
            color: None,
            volume: 1.0,
            looping: false,
            favorite: false,
            shortcut: None,
        }
    }
}

impl PadSettings {
    /// A name that is empty or only spaces is no name, and the volume stays
    /// between 0 and 1.
    fn normalized(mut self) -> Self {
        self.name = self
            .name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        self.volume = if self.volume.is_nan() {
            1.0
        } else {
            self.volume.clamp(0.0, 1.0)
        };
        self.shortcut = self.shortcut.and_then(shortcut_key);
        self
    }

    /// A field of the wrong type goes back to its default.
    fn from_json(value: &Value) -> Self {
        Self {
            name: value.get("name").and_then(Value::as_str).map(str::to_owned),
            color: value
                .get("color")
                .and_then(Value::as_str)
                .and_then(PadColor::from_name),
            volume: value.get("volume").and_then(Value::as_f64).unwrap_or(1.0),
            looping: value.get("loop").and_then(Value::as_bool).unwrap_or(false),
            favorite: value
                .get("favorite")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            shortcut: value
                .get("shortcut")
                .and_then(Value::as_str)
                .and_then(single_char),
        }
        .normalized()
    }
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    chars.next().filter(|_| chars.next().is_none())
}

/// The pad key a character stands for: letters in lower case and digits.
pub fn shortcut_key(c: char) -> Option<char> {
    c.is_ascii_alphanumeric().then(|| c.to_ascii_lowercase())
}

/// The key as a pad shows it.
pub fn shortcut_label(key: char) -> String {
    key.to_ascii_uppercase().to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadError {
    Read(String),
    /// Not the JSON this app writes.
    Invalid(String),
    /// Written by a newer version of the app.
    Version(u64),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(message) => write!(f, "could not read the file: {message}"),
            Self::Invalid(message) => write!(f, "not a valid pad file: {message}"),
            Self::Version(version) => write!(f, "unknown file version {version}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// The settings of every pad, by absolute file path. Only pads that differ
/// from the default have an entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadStore {
    pads: BTreeMap<String, PadSettings>,
}

#[derive(Serialize)]
struct File<'a> {
    version: u64,
    pads: &'a BTreeMap<String, PadSettings>,
}

impl PadStore {
    pub fn get(&self, path: &str) -> PadSettings {
        self.pads.get(path).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, path: &str, settings: PadSettings) {
        let settings = settings.normalized();
        if settings == PadSettings::default() {
            self.pads.remove(path);
        } else {
            self.pads.insert(path.to_owned(), settings);
        }
    }

    /// Moves the settings of a file that was renamed, replacing what `to`
    /// had. Returns whether anything changed.
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        match self.pads.remove(from) {
            Some(settings) => {
                self.pads.insert(to.to_owned(), settings);
                true
            }
            None => false,
        }
    }

    /// Moves the settings of every file inside the folder `from` to the same
    /// relative path inside `to`, replacing what was there. Returns how many
    /// entries moved.
    pub fn move_folder(&mut self, from: &str, to: &str) -> usize {
        let from = format!("{}/", from.trim_end_matches('/'));
        let to = format!("{}/", to.trim_end_matches('/'));
        if from == to {
            return 0;
        }
        let paths = self.pads.keys().filter(|path| path.starts_with(&from));
        let paths: Vec<String> = paths.cloned().collect();
        for path in &paths {
            if let Some(settings) = self.pads.remove(path) {
                self.pads
                    .insert(format!("{to}{}", &path[from.len()..]), settings);
            }
        }
        paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pads.is_empty()
    }

    /// The path of the entry that has the key.
    pub fn shortcut_owner(&self, key: char) -> Option<&str> {
        let key = shortcut_key(key)?;
        let mut pads = self.pads.iter();
        pads.find(|(_, settings)| settings.shortcut == Some(key))
            .map(|(path, _)| path.as_str())
    }

    /// Clears the key from every entry but `except`, so that a key belongs
    /// to one pad. Returns the paths that lost it.
    pub fn take_shortcut(&mut self, key: char, except: &str) -> Vec<String> {
        let Some(key) = shortcut_key(key) else {
            return Vec::new();
        };
        let owners = self.pads.iter();
        let owners = owners
            .filter(|(path, settings)| settings.shortcut == Some(key) && path.as_str() != except);
        let paths: Vec<String> = owners.map(|(path, _)| path.clone()).collect();
        for path in &paths {
            let mut settings = self.get(path);
            settings.shortcut = None;
            self.set(path, settings);
        }
        paths
    }

    /// A missing file is an empty store.
    pub fn load(file: &Path) -> Result<Self, LoadError> {
        match std::fs::read_to_string(file) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(LoadError::Read(error.to_string())),
        }
    }

    fn parse(text: &str) -> Result<Self, LoadError> {
        let root: Value =
            serde_json::from_str(text).map_err(|error| LoadError::Invalid(error.to_string()))?;
        let Some(root) = root.as_object() else {
            return Err(LoadError::Invalid("the top level is not an object".into()));
        };
        if let Some(version) = root.get("version").and_then(Value::as_u64) {
            if version > VERSION {
                return Err(LoadError::Version(version));
            }
        }
        let mut store = Self::default();
        if let Some(pads) = root.get("pads").and_then(Value::as_object) {
            for (path, value) in pads {
                store.set(path, PadSettings::from_json(value));
            }
        }
        // A key belongs to the first path that has it.
        let mut keys = Vec::new();
        for settings in store.pads.values_mut() {
            match settings.shortcut {
                Some(key) if keys.contains(&key) => settings.shortcut = None,
                Some(key) => keys.push(key),
                None => {}
            }
        }
        store
            .pads
            .retain(|_, settings| *settings != PadSettings::default());
        Ok(store)
    }

    /// Writes the whole file through a temporary one, so a crash never
    /// leaves half a file.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        let contents = File {
            version: VERSION,
            pads: &self.pads,
        };
        let mut text = serde_json::to_string_pretty(&contents)?;
        text.push('\n');
        if let Some(directory) = file.parent() {
            std::fs::create_dir_all(directory)?;
        }
        let temporary = file.with_extension("json.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, file)
    }
}

/// What a click on a pad does, a global setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TriggerMode {
    #[default]
    Overlap,
    Restart,
    StopOthers,
}

impl TriggerMode {
    pub const ALL: [Self; 3] = [Self::Overlap, Self::Restart, Self::StopOthers];

    /// Anything unknown is `Overlap`.
    pub fn from_name(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == name)
            .unwrap_or_default()
    }

    /// The value of the `trigger-mode` setting.
    pub fn name(self) -> &'static str {
        match self {
            Self::Overlap => "overlap",
            Self::Restart => "restart",
            Self::StopOthers => "stop-others",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Start,
    /// Start the pad and stop every other sound.
    StartAlone,
    Stop,
    Restart,
}

/// What triggering a pad does, from the mode and whether the pad is playing.
pub fn trigger(mode: TriggerMode, playing: bool) -> Trigger {
    match (mode, playing) {
        (TriggerMode::Restart, true) => Trigger::Restart,
        (_, true) => Trigger::Stop,
        (TriggerMode::StopOthers, false) => Trigger::StartAlone,
        (_, false) => Trigger::Start,
    }
}

/// What "make sure this sound plays" does, for the search: like `trigger`,
/// but a playing sound is never stopped.
pub fn play(mode: TriggerMode, playing: bool) -> Option<Trigger> {
    match trigger(mode, playing) {
        Trigger::Stop => None,
        other => Some(other),
    }
}

/// Whether every word of `query` is in the shown name or in the file name,
/// ignoring case. An empty query matches nothing.
pub fn matches(query: &str, display_name: &str, file_name: &str) -> bool {
    let (display_name, file_name) = (display_name.to_lowercase(), file_name.to_lowercase());
    let query = query.to_lowercase();
    let mut words = query.split_whitespace().peekable();
    words.peek().is_some()
        && words.all(|word| display_name.contains(word) || file_name.contains(word))
}

/// The order of the pads, a global setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortOrder {
    #[default]
    Name,
    /// The newest file first.
    Recent,
}

impl SortOrder {
    /// Anything unknown is `Name`.
    pub fn from_name(name: &str) -> Self {
        match name {
            "recent" => Self::Recent,
            _ => Self::Name,
        }
    }

    /// The value of the `sort-order` setting.
    pub fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Recent => "recent",
        }
    }
}

/// What the order of a pad depends on.
#[derive(Debug, Clone, Copy)]
pub struct SortKey<'a> {
    pub display_name: &'a str,
    pub file_name: &'a str,
    pub modified: SystemTime,
}

pub fn compare(order: SortOrder, a: SortKey, b: SortKey) -> Ordering {
    let by_name = || {
        let names = a
            .display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase());
        names.then_with(|| a.file_name.cmp(b.file_name))
    };
    match order {
        SortOrder::Name => by_name(),
        SortOrder::Recent => b.modified.cmp(&a.modified).then_with(by_name),
    }
}

/// `00:23`, `01:12`, or `1:02:03` from one hour up, rounded down.
pub fn format_time(time: Duration) -> String {
    let seconds = time.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// The time left, with a leading minus: `-01:12`.
pub fn format_remaining(time: Duration) -> String {
    format!("-{}", format_time(time))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> PadStore {
        PadStore::parse(text).unwrap()
    }

    fn pad(fields: &str) -> PadSettings {
        parse(&format!(
            r#"{{"version": 1, "pads": {{"/a.wav": {{{fields}}}}}}}"#
        ))
        .get("/a.wav")
    }

    #[test]
    fn a_pad_without_an_entry_has_the_default_settings() {
        let settings = PadStore::default().get("/a.wav");
        assert_eq!(settings, PadSettings::default());
        assert_eq!(settings.volume, 1.0);
        assert!(!settings.looping && settings.name.is_none() && settings.color.is_none());
    }

    #[test]
    fn default_settings_remove_the_entry() {
        let mut store = PadStore::default();
        store.set(
            "/a.wav",
            PadSettings {
                looping: true,
                ..Default::default()
            },
        );
        assert!(!store.is_empty());
        store.set("/a.wav", PadSettings::default());
        assert!(store.is_empty());
    }

    #[test]
    fn every_field_is_read() {
        let settings = pad(r#""name": "Intro", "color": "purple", "volume": 0.8, "loop": true"#);
        assert_eq!(
            settings,
            PadSettings {
                name: Some("Intro".into()),
                color: Some(PadColor::Purple),
                volume: 0.8,
                looping: true,
                favorite: false,
                shortcut: None,
            }
        );
    }

    #[test]
    fn the_shortcut_is_read_and_left_out_when_none() {
        assert_eq!(pad(r#""shortcut": "q""#).shortcut, Some('q'));
        assert_eq!(pad(r#""shortcut": "Q""#).shortcut, Some('q'));
        assert_eq!(pad(r#""shortcut": "7""#).shortcut, Some('7'));
        assert_eq!(pad(r#""loop": true"#).shortcut, None);
        assert_eq!(pad(r#""shortcut": 7, "loop": true"#).shortcut, None);
        assert_eq!(pad(r#""shortcut": "qw", "loop": true"#).shortcut, None);
        assert_eq!(pad(r#""shortcut": "", "loop": true"#).shortcut, None);
        assert_eq!(pad(r#""shortcut": "-", "loop": true"#).shortcut, None);

        let mut store = PadStore::default();
        let keyed = PadSettings {
            shortcut: Some('q'),
            ..Default::default()
        };
        store.set("/a.wav", keyed.clone());
        let looping = PadSettings {
            looping: true,
            ..Default::default()
        };
        store.set("/b.wav", looping);
        let text = serde_json::to_string(&File {
            version: VERSION,
            pads: &store.pads,
        })
        .unwrap();
        assert_eq!(text.matches("\"shortcut\":\"q\"").count(), 1);
        assert_eq!(text.matches("shortcut").count(), 1);
        assert_eq!(parse(&text).get("/a.wav"), keyed);
    }

    fn folder_store() -> PadStore {
        parse(
            r#"{"version": 1, "pads": {
                "/a/F/one.wav": {"color": "red", "shortcut": "q"},
                "/a/F/sub/two.wav": {"name": "Two"},
                "/a/Fx/three.wav": {"shortcut": "w"},
                "/b/G/one.wav": {"shortcut": "e", "loop": true}
            }}"#,
        )
    }

    #[test]
    fn move_folder_moves_the_entries_inside_it() {
        let mut store = folder_store();
        assert_eq!(store.move_folder("/a/F", "/c/New"), 2);
        assert_eq!(store.get("/a/F/one.wav"), PadSettings::default());
        let one = store.get("/c/New/one.wav");
        assert_eq!(one.color, Some(PadColor::Red));
        assert_eq!(one.shortcut, Some('q'));
        assert_eq!(store.get("/c/New/sub/two.wav").name.as_deref(), Some("Two"));
    }

    #[test]
    fn move_folder_respects_the_path_boundary() {
        let mut store = folder_store();
        store.move_folder("/a/F/", "/c/New");
        assert_eq!(store.get("/a/Fx/three.wav").shortcut, Some('w'));
        assert_eq!(store.shortcut_owner('w'), Some("/a/Fx/three.wav"));
    }

    #[test]
    fn move_folder_replaces_the_destination_and_keeps_keys_unique() {
        let mut store = folder_store();
        assert_eq!(store.move_folder("/a/F", "/b/G"), 2);
        let one = store.get("/b/G/one.wav");
        assert_eq!(one.shortcut, Some('q'));
        assert!(!one.looping);
        assert_eq!(store.shortcut_owner('e'), None);
        assert_eq!(store.shortcut_owner('q'), Some("/b/G/one.wav"));
    }

    #[test]
    fn move_folder_to_itself_changes_nothing() {
        let mut store = folder_store();
        assert_eq!(store.move_folder("/a/F", "/a/F/"), 0);
        assert_eq!(store, folder_store());
    }

    #[test]
    fn shortcut_keys() {
        assert_eq!(shortcut_key('q'), Some('q'));
        assert_eq!(shortcut_key('Q'), Some('q'));
        assert_eq!(shortcut_key('7'), Some('7'));
        assert_eq!(shortcut_key('é'), None);
        assert_eq!(shortcut_key(' '), None);
        assert_eq!(shortcut_key('-'), None);
        assert_eq!(shortcut_label('q'), "Q");
        assert_eq!(shortcut_label('7'), "7");
    }

    fn keyed(key: char) -> PadSettings {
        PadSettings {
            shortcut: Some(key),
            ..Default::default()
        }
    }

    #[test]
    fn a_shortcut_is_taken_from_the_others() {
        let mut store = PadStore::default();
        assert_eq!(store.shortcut_owner('q'), None);
        assert!(store.take_shortcut('q', "/a.wav").is_empty());

        store.set("/b.wav", keyed('q'));
        assert_eq!(store.shortcut_owner('Q'), Some("/b.wav"));
        assert_eq!(store.take_shortcut('q', "/a.wav"), ["/b.wav"]);
        assert_eq!(store.shortcut_owner('q'), None);
        // The entry had nothing else.
        assert!(store.is_empty());

        store.set("/a.wav", keyed('q'));
        assert!(store.take_shortcut('q', "/a.wav").is_empty());
        assert_eq!(store.shortcut_owner('q'), Some("/a.wav"));
    }

    #[test]
    fn load_keeps_a_shortcut_unique() {
        let store = parse(
            r#"{"version": 1, "pads": {
                "/c.wav": {"shortcut": "q"},
                "/a.wav": {"shortcut": "Q", "loop": true},
                "/b.wav": {"shortcut": "q", "loop": true},
                "/d.wav": {"shortcut": "w"}
            }}"#,
        );
        assert_eq!(store.get("/a.wav").shortcut, Some('q'));
        assert_eq!(store.get("/b.wav").shortcut, None);
        assert!(store.get("/b.wav").looping);
        assert_eq!(store.get("/d.wav").shortcut, Some('w'));
        // Nothing is left of an entry that only had the key.
        assert_eq!(store.pads.len(), 3);
    }

    #[test]
    fn rename_carries_the_shortcut() {
        let mut store = PadStore::default();
        store.set("/a.wav", keyed('q'));
        store.set("/b.wav", keyed('w'));
        assert!(store.rename("/a.wav", "/b.wav"));
        assert_eq!(store.shortcut_owner('q'), Some("/b.wav"));
        assert_eq!(store.shortcut_owner('w'), None);
    }

    #[test]
    fn the_favorite_is_read_and_left_out_when_false() {
        assert!(pad(r#""favorite": true"#).favorite);
        assert!(!pad(r#""loop": true"#).favorite);
        assert!(!pad(r#""favorite": "yes", "loop": true"#).favorite);
        let mut store = PadStore::default();
        let favorite = PadSettings {
            favorite: true,
            ..Default::default()
        };
        store.set("/a.wav", favorite.clone());
        let looping = PadSettings {
            looping: true,
            ..Default::default()
        };
        store.set("/b.wav", looping);
        let text = serde_json::to_string(&File {
            version: VERSION,
            pads: &store.pads,
        })
        .unwrap();
        assert_eq!(text.matches("\"favorite\":true").count(), 1);
        assert_eq!(parse(&text).get("/a.wav"), favorite);
    }

    #[test]
    fn rename_moves_the_settings() {
        let looping = PadSettings {
            looping: true,
            ..Default::default()
        };
        let favorite = PadSettings {
            favorite: true,
            ..Default::default()
        };
        let mut store = PadStore::default();
        assert!(!store.rename("/a.wav", "/b.wav"));
        assert!(store.is_empty());

        store.set("/a.wav", looping.clone());
        assert!(store.rename("/a.wav", "/b.wav"));
        assert_eq!(store.get("/a.wav"), PadSettings::default());
        assert_eq!(store.get("/b.wav"), looping);

        store.set("/c.wav", favorite);
        assert!(store.rename("/b.wav", "/c.wav"));
        assert_eq!(store.get("/c.wav"), looping);
        assert_eq!(store.pads.len(), 1);
    }

    #[test]
    fn a_missing_file_is_an_empty_store() {
        let file = std::env::temp_dir().join("vinheta-test-missing/pads.json");
        assert_eq!(PadStore::load(&file), Ok(PadStore::default()));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let store = parse(
            r#"{"version": 1, "later": 1, "pads": {"/a.wav": {"loop": true, "hotkey": "F1"}}}"#,
        );
        assert!(store.get("/a.wav").looping);
    }

    #[test]
    fn the_volume_is_clamped() {
        assert_eq!(pad(r#""volume": 7"#).volume, 1.0);
        assert_eq!(pad(r#""volume": -1, "loop": true"#).volume, 0.0);
    }

    #[test]
    fn a_bad_field_resets_that_field_only() {
        let settings = pad(r#""color": "pink", "loop": true"#);
        assert_eq!(settings.color, None);
        assert!(settings.looping);
        let settings = pad(r#""name": 3, "volume": "loud", "loop": "yes", "color": "red""#);
        assert_eq!(
            settings,
            PadSettings {
                color: Some(PadColor::Red),
                ..Default::default()
            }
        );
    }

    #[test]
    fn an_empty_name_is_no_name() {
        assert_eq!(pad(r#""name": "", "loop": true"#).name, None);
        assert_eq!(pad(r#""name": "   ", "loop": true"#).name, None);
        assert_eq!(pad(r#""name": " Intro ""#).name.as_deref(), Some("Intro"));
    }

    #[test]
    fn a_broken_file_is_an_error() {
        assert!(matches!(PadStore::parse("{"), Err(LoadError::Invalid(_))));
        assert!(matches!(PadStore::parse("[]"), Err(LoadError::Invalid(_))));
        assert_eq!(
            PadStore::parse(r#"{"version": 2, "pads": {}}"#),
            Err(LoadError::Version(2))
        );
    }

    #[test]
    fn a_saved_store_loads_back() {
        let directory = std::env::temp_dir().join(format!("vinheta-test-{}", std::process::id()));
        let file = directory.join("nested/pads.json");
        let mut store = PadStore::default();
        store.set(
            "/sounds/a b.wav",
            PadSettings {
                name: Some("Intro “1”".into()),
                color: Some(PadColor::Brown),
                volume: 0.25,
                looping: true,
                favorite: true,
                shortcut: Some('q'),
            },
        );
        store.set(
            "/sounds/c.ogg",
            PadSettings {
                color: Some(PadColor::Blue),
                ..Default::default()
            },
        );
        store.save(&file).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let loaded = PadStore::load(&file);
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(loaded, Ok(store));
        // Fields at their default are left out.
        assert_eq!(text.matches("\"volume\"").count(), 1);
        assert_eq!(text.matches("\"loop\"").count(), 1);
        assert!(text.contains("\"version\": 1"));
    }

    #[test]
    fn color_names_are_stable() {
        let names: Vec<_> = PadColor::ALL.iter().map(|color| color.name()).collect();
        assert_eq!(
            names,
            ["blue", "green", "yellow", "orange", "red", "purple", "brown"]
        );
        assert_eq!(PadColor::from_name("purple"), Some(PadColor::Purple));
        assert_eq!(PadColor::from_name("Purple"), None);
    }

    #[test]
    fn trigger_mode_names() {
        assert_eq!(TriggerMode::from_name("overlap"), TriggerMode::Overlap);
        assert_eq!(TriggerMode::from_name("restart"), TriggerMode::Restart);
        assert_eq!(
            TriggerMode::from_name("stop-others"),
            TriggerMode::StopOthers
        );
        assert_eq!(TriggerMode::from_name("bogus"), TriggerMode::Overlap);
    }

    #[test]
    fn trigger_rule() {
        assert_eq!(trigger(TriggerMode::Overlap, true), Trigger::Stop);
        assert_eq!(trigger(TriggerMode::Overlap, false), Trigger::Start);
        assert_eq!(trigger(TriggerMode::Restart, true), Trigger::Restart);
        assert_eq!(trigger(TriggerMode::Restart, false), Trigger::Start);
        assert_eq!(trigger(TriggerMode::StopOthers, true), Trigger::Stop);
        assert_eq!(trigger(TriggerMode::StopOthers, false), Trigger::StartAlone);
    }

    #[test]
    fn play_rule_never_stops() {
        assert_eq!(play(TriggerMode::Overlap, true), None);
        assert_eq!(play(TriggerMode::Overlap, false), Some(Trigger::Start));
        assert_eq!(play(TriggerMode::Restart, true), Some(Trigger::Restart));
        assert_eq!(play(TriggerMode::Restart, false), Some(Trigger::Start));
        assert_eq!(play(TriggerMode::StopOthers, true), None);
        assert_eq!(
            play(TriggerMode::StopOthers, false),
            Some(Trigger::StartAlone)
        );
    }

    #[test]
    fn search_match() {
        assert!(matches("horn", "Air Horn", "Air Horn.wav"));
        assert!(matches("horn air", "Air Horn", "Air Horn.wav"));
        assert!(matches("  AIR   hOrN ", "Air Horn", "Air Horn.wav"));
        // Only the file name has it.
        assert!(matches("crick", "Zebra", "Crickets.wav"));
        // One word in each.
        assert!(matches("zeb crick", "Zebra", "Crickets.wav"));
        assert!(matches("é", "CAFÉ", "x.wav"));
        assert!(matches("CAFÉ", "café", "x.wav"));
        assert!(!matches("bell", "Air Horn", "Air Horn.wav"));
        assert!(!matches("air bell", "Air Horn", "Air Horn.wav"));
        assert!(!matches("", "Air Horn", "Air Horn.wav"));
        assert!(!matches("   ", "Air Horn", "Air Horn.wav"));
    }

    #[test]
    fn sort_order_names() {
        assert_eq!(SortOrder::from_name("name"), SortOrder::Name);
        assert_eq!(SortOrder::from_name("recent"), SortOrder::Recent);
        assert_eq!(SortOrder::from_name("bogus"), SortOrder::Name);
        assert_eq!(SortOrder::Recent.name(), "recent");
    }

    fn key<'a>(display_name: &'a str, file_name: &'a str, seconds: u64) -> SortKey<'a> {
        SortKey {
            display_name,
            file_name,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
        }
    }

    #[test]
    fn sort_by_name() {
        let order = SortOrder::Name;
        let less = |a, b| compare(order, a, b) == Ordering::Less;
        assert!(less(
            key("apple", "apple.wav", 1),
            key("Bell", "Bell.wav", 9)
        ));
        assert!(less(key("Bell", "Bell.ogg", 1), key("bell", "bell.mp3", 1)));
        assert_eq!(
            compare(order, key("a", "a.wav", 1), key("a", "a.wav", 2)),
            Ordering::Equal
        );
        // A custom name moves the pad.
        assert!(less(
            key("Air Horn", "Air Horn.wav", 1),
            key("Crickets", "Crickets.wav", 1)
        ));
        assert!(less(
            key("Crickets", "Crickets.wav", 1),
            key("Zulu", "Air Horn.wav", 1)
        ));
    }

    #[test]
    fn sort_by_recent() {
        let order = SortOrder::Recent;
        let less = |a, b| compare(order, a, b) == Ordering::Less;
        assert!(less(key("z", "z.wav", 9), key("a", "a.wav", 1)));
        // The same time falls back to the name, then to the file name.
        assert!(less(key("a", "z.wav", 5), key("b", "a.wav", 5)));
        assert!(less(key("a", "a.ogg", 5), key("a", "a.wav", 5)));
    }

    #[test]
    fn times() {
        assert_eq!(format_time(Duration::ZERO), "00:00");
        assert_eq!(format_time(Duration::from_secs_f64(59.9)), "00:59");
        assert_eq!(format_time(Duration::from_secs(60)), "01:00");
        assert_eq!(format_time(Duration::from_secs(72)), "01:12");
        assert_eq!(format_time(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(format_time(Duration::from_secs(3723)), "1:02:03");
        assert_eq!(format_remaining(Duration::ZERO), "-00:00");
        assert_eq!(format_remaining(Duration::from_secs(72)), "-01:12");
    }
}
