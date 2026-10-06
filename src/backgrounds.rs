/* backgrounds.rs
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

//! The background images of pads: which files are images, the reduced copy
//! the app keeps of one, and the removal of copies no pad uses. GdkPixbuf
//! needs no display, so this runs in the tests and off the main thread.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gtk::gdk_pixbuf::Pixbuf;
use gtk::glib;

/// The longer side of a stored copy, enough for a pad at twice the scale.
pub const MAX_SIZE: i32 = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NotAnImage,
    Read(String),
    Write(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnImage => write!(f, "not an image"),
            Self::Read(message) => write!(f, "could not read the image: {message}"),
            Self::Write(message) => write!(f, "could not write the copy: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// The extensions of the formats the installed loaders can read.
fn extensions() -> &'static [String] {
    static EXTENSIONS: OnceLock<Vec<String>> = OnceLock::new();
    EXTENSIONS.get_or_init(|| {
        let formats = Pixbuf::formats()
            .into_iter()
            .filter(|format| !format.is_disabled());
        let extensions = formats.flat_map(|format| format.extensions());
        extensions
            .map(|extension| extension.to_lowercase())
            .collect()
    })
}

pub fn is_image(path: &Path) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str());
    extension.is_some_and(|extension| {
        let extension = extension.to_lowercase();
        extensions().contains(&extension)
    })
}

/// Separates the images from the other paths, keeping their order.
pub fn split(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    paths.into_iter().partition(|path| is_image(path))
}

/// Writes into `dir` a copy of the image that is at most `MAX_SIZE` on its
/// longer side and upright, and returns its file name. The name comes from
/// the bytes, so the same image is stored once.
pub fn store(source: &Path, dir: &Path) -> Result<String, Error> {
    let (_, width, height) = Pixbuf::file_info(source).ok_or(Error::NotAnImage)?;
    // Loading at scale would also enlarge a smaller image.
    let pixbuf = if width.max(height) > MAX_SIZE {
        Pixbuf::from_file_at_scale(source, MAX_SIZE, MAX_SIZE, true)
    } else {
        Pixbuf::from_file(source)
    };
    let pixbuf = pixbuf.map_err(|error| Error::Read(error.to_string()))?;
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    let (kind, extension, options): (_, _, &[(&str, &str)]) = if pixbuf.has_alpha() {
        ("png", "png", &[])
    } else {
        ("jpeg", "jpg", &[("quality", "85")])
    };
    let bytes = pixbuf
        .save_to_bufferv(kind, options)
        .map_err(|error| Error::Write(error.to_string()))?;
    let checksum = glib::compute_checksum_for_data(glib::ChecksumType::Sha256, &bytes)
        .ok_or_else(|| Error::Write("no checksum".into()))?;
    let name = format!("{}.{extension}", &checksum[..16]);

    let file = dir.join(&name);
    if file.is_file() {
        return Ok(name);
    }
    let failed = |error: std::io::Error| Error::Write(error.to_string());
    std::fs::create_dir_all(dir).map_err(failed)?;
    let temporary = dir.join(format!(".{name}.part"));
    let result =
        std::fs::write(&temporary, &bytes).and_then(|_| std::fs::rename(&temporary, &file));
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed(error));
    }
    Ok(name)
}

/// Removes the files of `dir` that are not in `referenced`, and returns how
/// many it removed. It must not run while a `store` into `dir` is going on.
pub fn sweep(dir: &Path, referenced: &BTreeSet<String>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        let name = entry.file_name();
        let used = name.to_str().is_some_and(|name| referenced.contains(name));
        if is_file && !used && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests;
