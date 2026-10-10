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
//! the app keeps of one, the removal of copies no pad uses, and the geometry
//! of the area a pad shows. GdkPixbuf
//! needs no display, so this runs in the tests and off the main thread.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gtk::gdk_pixbuf::Pixbuf;
use gtk::glib;

use crate::pads::Crop;

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

/// A width and a height.
pub type Size = (f64, f64);
/// An x, a y, a width, and a height.
pub type Rect = (f64, f64, f64, f64);

/// The width over the height of a pad of the wide layout, 144 by 96: the
/// frame the background is adjusted in.
pub const PAD_ASPECT: f64 = 1.5;
pub const MIN_ZOOM: f64 = 1.0;
/// The copies are 512 pixels at most, so a larger zoom would be blurry.
pub const MAX_ZOOM: f64 = 3.0;

impl Crop {
    /// The largest centered crop of that aspect inside the picture.
    pub fn fit(image: Size, aspect: f64) -> Self {
        let image_aspect = image.0 / image.1;
        let (width, height) = if image_aspect > aspect {
            (aspect / image_aspect, 1.0)
        } else {
            (1.0, image_aspect / aspect)
        };
        Self {
            x: (1.0 - width) / 2.0,
            y: (1.0 - height) / 2.0,
            width,
            height,
        }
    }

    /// The crop of the fitted aspect that is `zoom` times smaller than the
    /// fitted one, around the same center, kept inside the picture.
    pub fn zoomed(self, image: Size, aspect: f64, zoom: f64) -> Self {
        let fit = Self::fit(image, aspect);
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let (width, height) = (fit.width / zoom, fit.height / zoom);
        let (center_x, center_y) = (self.x + self.width / 2.0, self.y + self.height / 2.0);
        Self {
            x: center_x - width / 2.0,
            y: center_y - height / 2.0,
            width,
            height,
        }
        .moved(0.0, 0.0)
    }

    /// How many times smaller than the fitted crop this one is.
    pub fn zoom(self, image: Size, aspect: f64) -> f64 {
        Self::fit(image, aspect).width / self.width
    }

    /// Dragged by pixels of a frame the crop covers: the picture follows the
    /// pointer, so the crop goes the other way.
    pub fn dragged(self, image: Size, frame: Size, dx: f64, dy: f64) -> Self {
        let (_, _, width, height) = cover(image, Some(self), frame);
        self.moved(-dx / width, -dy / height)
    }

    /// Moved by fractions of its own size, as the arrow keys do.
    pub fn nudged(self, dx: f64, dy: f64) -> Self {
        self.moved(dx * self.width, dy * self.height)
    }

    /// Moved by fractions of the picture, without leaving it.
    pub fn moved(self, dx: f64, dy: f64) -> Self {
        let clamp = |value: f64, size: f64| value.clamp(0.0, (1.0 - size).max(0.0));
        Self {
            x: clamp(self.x + dx, self.width),
            y: clamp(self.y + dy, self.height),
            ..self
        }
    }
}

/// Where the whole picture goes so that the crop (or the whole picture)
/// covers the target, centered, keeping its aspect.
pub fn cover(image: Size, crop: Option<Crop>, target: Size) -> Rect {
    let crop = crop.unwrap_or(Crop::WHOLE);
    let (crop_width, crop_height) = (crop.width * image.0, crop.height * image.1);
    let scale = (target.0 / crop_width).max(target.1 / crop_height);
    let (width, height) = (image.0 * scale, image.1 * scale);
    let x = target.0 / 2.0 - (crop.x + crop.width / 2.0) * width;
    let y = target.1 / 2.0 - (crop.y + crop.height / 2.0) * height;
    (x, y, width, height)
}

#[cfg(test)]
mod tests;
