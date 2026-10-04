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

    fn file(path: &str) -> SoundFile {
        SoundFile {
            path: PathBuf::from(path),
            name: String::new(),
            modified: SystemTime::UNIX_EPOCH,
        }
    }

    fn paths(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn scan_reads_the_modification_time() {
        let fixture = Fixture::new(&["a.wav"]);
        let past = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let file = fs::File::options()
            .write(true)
            .open(fixture.0.join("a.wav"))
            .unwrap();
        file.set_modified(past).unwrap();
        assert_eq!(scan(&fixture.0).unwrap()[0].modified, past);
    }

    #[test]
    fn diff_of_an_unchanged_folder_is_empty() {
        let current = paths(&["/f/a.wav", "/f/b.wav"]);
        let scanned = [file("/f/a.wav"), file("/f/b.wav")];
        assert_eq!(diff(&current, &scanned), Diff::default());
        assert_eq!(diff(&[], &[]), Diff::default());
    }

    #[test]
    fn diff_finds_added_and_removed_files() {
        let current = paths(&["/f/a.wav", "/f/b.wav"]);
        let added = diff(
            &current,
            &[file("/f/a.wav"), file("/f/b.wav"), file("/f/c.wav")],
        );
        assert_eq!(added.added, [file("/f/c.wav")]);
        assert!(added.removed.is_empty());
        let removed = diff(&current, &[file("/f/b.wav")]);
        assert!(removed.added.is_empty());
        assert_eq!(removed.removed, paths(&["/f/a.wav"]));
    }

    #[test]
    fn diff_shows_a_rename_as_removed_and_added() {
        let current = paths(&["/f/a.wav", "/f/b.wav"]);
        let renamed = diff(&current, &[file("/f/b.wav"), file("/f/z.wav")]);
        assert_eq!(renamed.added, [file("/f/z.wav")]);
        assert_eq!(renamed.removed, paths(&["/f/a.wav"]));
    }

    #[test]
    fn import_names() {
        let taken = |names: &'static [&'static str]| move |name: &str| names.contains(&name);
        assert_eq!(import_name(&taken(&[]), "Horn.wav"), "Horn.wav");
        assert_eq!(
            import_name(&taken(&["Horn.wav"]), "Horn.wav"),
            "Horn (2).wav"
        );
        assert_eq!(
            import_name(&taken(&["Horn.wav", "Horn (2).wav"]), "Horn.wav"),
            "Horn (3).wav"
        );
        assert_eq!(import_name(&taken(&["Horn"]), "Horn"), "Horn (2)");
        assert_eq!(
            import_name(&taken(&["Horn (2).wav"]), "Horn (2).wav"),
            "Horn (2) (2).wav"
        );
        assert_eq!(import_name(&taken(&["a.b.ogg"]), "a.b.ogg"), "a.b (2).ogg");
    }

    fn all_names(folder: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn import_copies_into_a_folder_it_creates() {
        let source = Fixture::new(&[]);
        fs::write(source.0.join("a.wav"), b"first").unwrap();
        fs::write(source.0.join("b.ogg"), b"second").unwrap();
        let folder = source.0.join("deep/sounds");
        let files = [source.0.join("a.wav"), source.0.join("b.ogg")];
        let report = import(&files, &folder);
        assert_eq!(report.copied, [folder.join("a.wav"), folder.join("b.ogg")]);
        assert!(report.skipped.is_empty());
        // Nothing but the copies, so no temporary file is left.
        assert_eq!(all_names(&folder), ["a.wav", "b.ogg"]);
        assert_eq!(fs::read(folder.join("b.ogg")).unwrap(), b"second");
        // The sources are untouched.
        assert_eq!(fs::read(source.0.join("a.wav")).unwrap(), b"first");
        assert_eq!(fs::read(source.0.join("b.ogg")).unwrap(), b"second");
    }

    #[test]
    fn import_skips_what_is_not_a_sound() {
        let source = Fixture::new(&["notes.txt", ".hidden.wav"]);
        fs::create_dir(source.0.join("folder.wav")).unwrap();
        let folder = source.0.join("sounds");
        let files = [
            source.0.join("notes.txt"),
            source.0.join(".hidden.wav"),
            source.0.join("folder.wav"),
            source.0.join("missing.wav"),
        ];
        let report = import(&files, &folder);
        assert!(report.copied.is_empty());
        let reasons: Vec<_> = report.skipped.iter().map(|(_, reason)| reason).collect();
        assert_eq!(reasons, [&SkipReason::Unsupported; 4]);
        // Nothing to copy, so the folder was not even created.
        assert!(!folder.exists());
    }

    #[test]
    fn import_skips_a_file_already_in_the_folder() {
        let folder = Fixture::new(&["a.wav"]);
        let report = import(&[folder.0.join("a.wav")], &folder.0);
        assert!(report.copied.is_empty());
        assert_eq!(
            report.skipped,
            [(folder.0.join("a.wav"), SkipReason::AlreadyThere)]
        );
        assert_eq!(all_names(&folder.0), ["a.wav"]);
    }

    #[test]
    fn import_never_overwrites() {
        let source = Fixture::new(&[]);
        fs::write(source.0.join("a.wav"), b"new").unwrap();
        let folder = Fixture::new(&[]);
        fs::write(folder.0.join("a.wav"), b"old").unwrap();
        let files = [source.0.join("a.wav"), source.0.join("a.wav")];
        let report = import(&files, &folder.0);
        assert_eq!(
            report.copied,
            [folder.0.join("a (2).wav"), folder.0.join("a (3).wav")]
        );
        assert_eq!(fs::read(folder.0.join("a.wav")).unwrap(), b"old");
        assert_eq!(fs::read(folder.0.join("a (2).wav")).unwrap(), b"new");
        assert_eq!(all_names(&folder.0), ["a (2).wav", "a (3).wav", "a.wav"]);
    }

    #[test]
    fn import_reports_a_copy_error() {
        let source = Fixture::new(&["a.wav"]);
        // A file where the folder should be.
        let folder = source.0.join("a.wav").join("sounds");
        let report = import(&[source.0.join("a.wav")], &folder);
        assert!(report.copied.is_empty());
        assert!(matches!(report.skipped[0].1, SkipReason::Copy(_)));
    }

    #[test]
    fn an_imported_copy_has_a_fresh_modification_time() {
        let source = Fixture::new(&["a.wav"]);
        let past = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let file = fs::File::options()
            .write(true)
            .open(source.0.join("a.wav"))
            .unwrap();
        file.set_modified(past).unwrap();
        drop(file);
        let folder = Fixture::new(&[]);
        let before = SystemTime::now() - std::time::Duration::from_secs(60);
        let report = import(&[source.0.join("a.wav")], &folder.0);
        let copy = fs::metadata(&report.copied[0]).unwrap().modified().unwrap();
        assert!(copy > before);
        let source = fs::metadata(source.0.join("a.wav")).unwrap();
        assert_eq!(source.modified().unwrap(), past);
    }
}
