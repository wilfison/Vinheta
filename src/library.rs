/* library.rs
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

//! The sound library: which files of a folder are sounds and in what order,
//! what changed in a folder, and the import of loose files.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Formats the packaged GStreamer plugins are guaranteed to decode.
pub const EXTENSIONS: [&str; 6] = ["wav", "flac", "mp3", "ogg", "oga", "opus"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundFile {
    pub path: PathBuf,
    /// The file name without its extension.
    pub name: String,
    /// `UNIX_EPOCH` when it cannot be read.
    pub modified: SystemTime,
}

/// Lists the sounds directly inside `folder`, sorted by name. Subfolders are
/// not entered.
pub fn scan(folder: &Path) -> io::Result<Vec<SoundFile>> {
    let mut sounds = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        // `is_file` follows symbolic links and is false for broken ones.
        if !is_sound(&path) || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_stem() else {
            continue;
        };
        let modified = path.metadata().and_then(|metadata| metadata.modified());
        sounds.push(SoundFile {
            name: name.to_string_lossy().into_owned(),
            modified: modified.unwrap_or(SystemTime::UNIX_EPOCH),
            path,
        });
    }
    sounds.sort_by_cached_key(|sound| (sound.name.to_lowercase(), sound.path.clone()));
    Ok(sounds)
}

/// Whether the name is one of a supported sound: a known extension, and not
/// hidden.
pub fn is_sound(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'));
    let supported = path.extension().is_some_and(|extension| {
        EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    });
    supported && !hidden
}

/// The name a pad shows for a file name without its extension: `_`, `-`, and
/// `.` become spaces (a dot between digits stays), spaces are collapsed, and
/// the first letter is uppercase. A name with nothing else is kept as it is.
pub fn humanize(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let is_digit = |index: Option<usize>| {
        index
            .and_then(|index| chars.get(index))
            .is_some_and(char::is_ascii_digit)
    };
    let spaced: String = chars
        .iter()
        .enumerate()
        .map(|(index, &char)| match char {
            '_' | '-' => ' ',
            '.' if !(is_digit(index.checked_sub(1)) && is_digit(Some(index + 1))) => ' ',
            other => other,
        })
        .collect();
    let words: Vec<_> = spaced.split_whitespace().collect();
    let joined = words.join(" ");
    let mut rest = joined.chars();
    match rest.next() {
        Some(first) => first.to_uppercase().chain(rest).collect(),
        None => name.to_owned(),
    }
}

/// What a new scan of a folder changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<SoundFile>,
    pub removed: Vec<PathBuf>,
}

/// Compares the paths being shown with a new scan of the same folder.
pub fn diff(current: &[PathBuf], scanned: &[SoundFile]) -> Diff {
    let added = scanned
        .iter()
        .filter(|file| !current.contains(&file.path))
        .cloned()
        .collect();
    let removed = current
        .iter()
        .filter(|path| scanned.iter().all(|file| file.path != **path))
        .cloned()
        .collect();
    Diff { added, removed }
}

/// The name a copy gets: `file_name` when it is free, otherwise `Name (2).wav`,
/// `Name (3).wav`, and so on.
pub fn import_name(taken: &dyn Fn(&str) -> bool, file_name: &str) -> String {
    if !taken(file_name) {
        return file_name.to_owned();
    }
    let (stem, extension) = match file_name.rfind('.') {
        Some(dot) if dot > 0 => file_name.split_at(dot),
        _ => (file_name, ""),
    };
    (2u32..)
        .map(|number| format!("{stem} ({number}){extension}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| file_name.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    Unsupported,
    /// The file is already in the folder that receives the copies.
    AlreadyThere,
    Copy(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// The new files.
    pub copied: Vec<PathBuf>,
    pub skipped: Vec<(PathBuf, SkipReason)>,
}

/// Copies each supported file into `folder`, creating it when missing. The
/// sources are never touched and nothing in `folder` is overwritten.
pub fn import(files: &[PathBuf], folder: &Path) -> ImportReport {
    let mut report = ImportReport::default();
    for file in files {
        match import_one(file, folder) {
            Ok(copy) => report.copied.push(copy),
            Err(reason) => report.skipped.push((file.clone(), reason)),
        }
    }
    report
}

fn import_one(file: &Path, folder: &Path) -> Result<PathBuf, SkipReason> {
    let name = file.file_name().and_then(|name| name.to_str());
    let Some(name) = name.filter(|_| is_sound(file) && file.is_file()) else {
        return Err(SkipReason::Unsupported);
    };
    let same_folder = match (file.parent().map(Path::canonicalize), folder.canonicalize()) {
        (Some(Ok(parent)), Ok(folder)) => parent == folder,
        _ => false,
    };
    if same_folder {
        return Err(SkipReason::AlreadyThere);
    }
    let failed = |error: io::Error| SkipReason::Copy(error.to_string());
    std::fs::create_dir_all(folder).map_err(failed)?;
    let name = import_name(&|name| folder.join(name).exists(), name);
    let copy = folder.join(&name);
    // A hidden name, which `scan` skips, until the copy is complete.
    let temporary = folder.join(format!(".{name}.part"));
    // `std::fs::copy` gives the copy a fresh modification time.
    let result = std::fs::copy(file, &temporary).and_then(|_| std::fs::rename(&temporary, &copy));
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed(error));
    }
    Ok(copy)
}

#[cfg(test)]
mod tests;
