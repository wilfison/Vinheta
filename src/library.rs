/* library.rs
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

//! The sound library: which files of a folder are sounds, and in what order.

use std::io;
use std::path::{Path, PathBuf};

/// Formats the packaged GStreamer plugins are guaranteed to decode.
const EXTENSIONS: [&str; 6] = ["wav", "flac", "mp3", "ogg", "oga", "opus"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundFile {
    pub path: PathBuf,
    /// The file name without its extension.
    pub name: String,
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
        let Some(name) = path.file_stem() else { continue };
        sounds.push(SoundFile {
            name: name.to_string_lossy().into_owned(),
            path,
        });
    }
    sounds.sort_by_cached_key(|sound| (sound.name.to_lowercase(), sound.path.clone()));
    Ok(sounds)
}

fn is_sound(path: &Path) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(files: &[&str]) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = format!(
                "vinheta-library-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let dir = std::env::temp_dir().join(unique);
            fs::create_dir_all(&dir).unwrap();
            for file in files {
                fs::write(dir.join(file), b"").unwrap();
            }
            Self(dir)
        }

        fn names(&self) -> Vec<String> {
            let sounds = scan(&self.0).unwrap();
            sounds.into_iter().map(|sound| sound.name).collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn keeps_only_supported_extensions() {
        let fixture = Fixture::new(&[
            "a.wav", "b.flac", "c.mp3", "d.ogg", "e.oga", "f.opus", "g.txt", "h.m4a", "wav",
        ]);
        assert_eq!(fixture.names(), ["a", "b", "c", "d", "e", "f"]);
    }

    #[test]
    fn extension_case_does_not_matter() {
        let fixture = Fixture::new(&["a.WAV", "b.Mp3"]);
        assert_eq!(fixture.names(), ["a", "b"]);
    }

    #[test]
    fn skips_hidden_files() {
        let fixture = Fixture::new(&[".hidden.wav", "shown.wav"]);
        assert_eq!(fixture.names(), ["shown"]);
    }

    #[test]
    fn does_not_enter_subfolders() {
        let fixture = Fixture::new(&["top.wav"]);
        fs::create_dir(fixture.0.join("sub")).unwrap();
        fs::write(fixture.0.join("sub/inner.wav"), b"").unwrap();
        // A folder named like a sound is not a sound either.
        fs::create_dir(fixture.0.join("folder.wav")).unwrap();
        assert_eq!(fixture.names(), ["top"]);
    }

    #[test]
    fn follows_links_to_files_and_skips_broken_ones() {
        let fixture = Fixture::new(&["real.wav"]);
        std::os::unix::fs::symlink(fixture.0.join("real.wav"), fixture.0.join("link.wav")).unwrap();
        std::os::unix::fs::symlink(fixture.0.join("gone.wav"), fixture.0.join("broken.wav"))
            .unwrap();
        assert_eq!(fixture.names(), ["link", "real"]);
    }

    #[test]
    fn name_is_the_file_name_without_the_extension() {
        let fixture = Fixture::new(&["Air Horn.mp3", "ta.da.ogg"]);
        let sounds = scan(&fixture.0).unwrap();
        assert_eq!(sounds[0].name, "Air Horn");
        assert_eq!(sounds[0].path, fixture.0.join("Air Horn.mp3"));
        assert_eq!(sounds[1].name, "ta.da");
    }

    #[test]
    fn sorts_by_name_ignoring_case_with_the_file_name_as_tie_breaker() {
        let fixture = Fixture::new(&["bell.wav", "Zebra.wav", "apple.wav", "Bell.ogg", "bell.mp3"]);
        let sounds = scan(&fixture.0).unwrap();
        let files: Vec<_> = sounds
            .iter()
            .map(|sound| sound.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(
            files,
            ["apple.wav", "Bell.ogg", "bell.mp3", "bell.wav", "Zebra.wav"]
        );
    }

    #[test]
    fn empty_folder_has_no_sounds() {
        assert!(Fixture::new(&[]).names().is_empty());
    }

    #[test]
    fn missing_folder_is_an_error() {
        let fixture = Fixture::new(&[]);
        assert!(scan(&fixture.0.join("missing")).is_err());
    }
}
