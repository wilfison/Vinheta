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
use std::collections::{BTreeMap, BTreeSet};
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
    /// The file name of the stored copy of the image, inside the backgrounds
    /// directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// The area of the background the pad shows; none for the whole picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
}

/// A rectangle of a picture, in fractions of its width and height, so that
/// it does not depend on the size of the copy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// What the app writes may be off by a rounding error.
const CROP_TOLERANCE: f64 = 1e-6;

impl Crop {
    pub const WHOLE: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    /// Inside the picture, and not empty.
    pub fn is_valid(&self) -> bool {
        let Self {
            x,
            y,
            width,
            height,
        } = *self;
        [x, y, width, height].iter().all(|value| value.is_finite())
            && x >= -CROP_TOLERANCE
            && y >= -CROP_TOLERANCE
            && width > 0.0
            && height > 0.0
            && x + width <= 1.0 + CROP_TOLERANCE
            && y + height <= 1.0 + CROP_TOLERANCE
    }

    fn from_json(value: &Value) -> Option<Self> {
        let member = |name: &str| value.get(name).and_then(Value::as_f64);
        Some(Self {
            x: member("x")?,
            y: member("y")?,
            width: member("width")?,
            height: member("height")?,
        })
    }
}

/// "x y width height", how the `Sound` object exposes it.
impl fmt::Display for Crop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let number = |value: f64| {
            let text = format!("{value:.4}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            if text == "-0" {
                "0".to_owned()
            } else {
                text.to_owned()
            }
        };
        write!(
            f,
            "{} {} {} {}",
            number(self.x),
            number(self.y),
            number(self.width),
            number(self.height)
        )
    }
}

impl std::str::FromStr for Crop {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        let numbers: Vec<f64> = text
            .split(' ')
            .map(|part| part.parse::<f64>().map_err(|_| ()))
            .collect::<Result<_, _>>()?;
        match numbers[..] {
            [x, y, width, height] => Ok(Self {
                x,
                y,
                width,
                height,
            }),
            _ => Err(()),
        }
    }
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
            background: None,
            crop: None,
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
        self.background = self.background.filter(|name| is_file_name(name));
        self.crop = self
            .crop
            .filter(|crop| self.background.is_some() && crop.is_valid());
        self
    }

    /// With another background, or none. A crop is an area of the picture
    /// it was made for, so it only stays with the same picture.
    pub fn with_background(mut self, name: Option<String>) -> Self {
        if name.is_none() || name != self.background {
            self.crop = None;
        }
        self.background = name;
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
            background: value
                .get("background")
                .and_then(Value::as_str)
                .map(str::to_owned),
            crop: value.get("crop").and_then(Crop::from_json),
        }
        .normalized()
    }
}

/// A name inside a directory, never a path that leaves it.
fn is_file_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains('/')
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

    /// The file names of the backgrounds that some entry uses.
    pub fn backgrounds(&self) -> BTreeSet<String> {
        let names = self
            .pads
            .values()
            .filter_map(|settings| settings.background.clone());
        names.collect()
    }

    /// Removes the entries of files that are gone from a folder that is
    /// still there. The entries of a missing folder stay: it may be on a
    /// drive that is not mounted, or come back with "Locate Folder…".
    /// Returns how many entries were removed.
    pub fn prune_missing(&mut self) -> usize {
        let before = self.pads.len();
        self.pads.retain(|path, _| !is_gone(Path::new(path)));
        before - self.pads.len()
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

/// Only a definite answer counts: a file or folder that cannot be read is
/// not gone.
fn is_gone(path: &Path) -> bool {
    path.parent().is_some_and(Path::is_dir) && matches!(path.try_exists(), Ok(false))
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
mod tests;
