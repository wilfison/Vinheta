/* pads.rs
 *
 * Copyright 2026 Will
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

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

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
}

impl Default for PadSettings {
    fn default() -> Self {
        Self {
            name: None,
            color: None,
            volume: 1.0,
            looping: false,
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
        }
        .normalized()
    }
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

    pub fn is_empty(&self) -> bool {
        self.pads.is_empty()
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
            }
        );
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
