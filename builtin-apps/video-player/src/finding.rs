//! Where the library finds its videos: the files of a type the app plays in a folder the user
//! added, and in the folders inside it two levels down, as a library of films keeps a folder per
//! film or per season. Hidden files and folders, whose names start with a dot, are passed over.
//!
//! The invariant: finding only reads folders; a folder that cannot be read, gone or no longer the
//! program's, gives no videos rather than failing the others.

use std::fs;
use std::path::{Path, PathBuf};

use crate::video_types::is_video;

/// How many levels of folders inside an added one are looked in.
const LEVELS_INSIDE: usize = 2;

/// The videos in `folder` and in its folders up to [`LEVELS_INSIDE`] down, each folder's own
/// files before its folders', each level in the order of the names.
pub(crate) fn videos_in(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    look_in(folder, LEVELS_INSIDE, &mut found);
    found
}

/// Adds the videos in `folder` to `found`, then those of its folders, `levels` more down.
fn look_in(folder: &Path, levels: usize, found: &mut Vec<PathBuf>) {
    let (files, folders) = entries_of(folder);
    found.extend(files.into_iter().filter(|file| is_video(file)));
    if levels == 0 {
        return;
    }
    for inside in folders {
        look_in(&inside, levels - 1, found);
    }
}

/// The files and the folders `folder` holds, hidden ones aside, each in the order of the names;
/// none where it cannot be read.
fn entries_of(folder: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return (Vec::new(), Vec::new());
    };
    let (mut files, mut folders) = (Vec::new(), Vec::new());
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => folders.push(entry.path()),
            Ok(kind) if kind.is_file() => files.push(entry.path()),
            _ => {}
        }
    }
    files.sort();
    folders.sort();
    (files, folders)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, under the system's temporary folder, with `files` made in it.
    fn folder_with(name: &str, files: &[&str]) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("video-player {name} {}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        for file in files {
            let path = folder.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "a clip").unwrap();
        }
        folder
    }

    #[test]
    fn videos_are_found_by_type_two_levels_down_hidden_ones_aside() {
        let folder = folder_with(
            "finding",
            &[
                "b.mp4",
                "a.MOV",
                "notes.txt",
                ".hidden.mp4",
                "Season 1/e1.m4v",
                "Season 1/Extras/making.mp4",
                "Season 1/Extras/Deeper/too-deep.mp4",
                ".cache/c.mp4",
            ],
        );
        let found: Vec<_> = videos_in(&folder)
            .into_iter()
            .map(|path| path.strip_prefix(&folder).unwrap().to_path_buf())
            .collect();
        assert_eq!(
            found,
            [
                Path::new("a.MOV"),
                Path::new("b.mp4"),
                Path::new("Season 1/e1.m4v"),
                Path::new("Season 1/Extras/making.mp4")
            ]
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_folder_that_cannot_be_read_has_no_videos() {
        assert!(videos_in(Path::new("/no such folder anywhere")).is_empty());
    }
}
