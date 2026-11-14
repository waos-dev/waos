//! The photos the app keeps, as files in one folder of its own `/data`: each named as a camera
//! names its pictures, `IMG_0001.jpg`, by a number one past the highest there, so the names say
//! the order they were taken in whatever the clock says; when one was taken is when its file was
//! written.
//!
//! The invariant a later editor will break: a photo is written whole under a draft's name, which
//! no listing shows, and renamed to its own, so a listing never names a half-written file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a photo's name starts with, before its number.
const PREFIX: &str = "IMG_";

/// What a draft's name ends with, until it is whole.
const DRAFT_SUFFIX: &str = ".saving";

/// The folder the photos are kept in.
#[derive(Clone, Debug)]
pub(crate) struct Library {
    folder: PathBuf,
}

impl Library {
    pub(crate) fn at(folder: impl Into<PathBuf>) -> Library {
        Library {
            folder: folder.into(),
        }
    }

    /// The photos kept, the newest first; none where the folder cannot be read, as before the
    /// first photo is kept.
    pub(crate) fn photos(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        let mut numbered: Vec<(u32, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter_map(|path| Some((number_of(&path)?, path)))
            .collect();
        numbered.sort_by(|(a, _), (b, _)| b.cmp(a));
        numbered.into_iter().map(|(_, path)| path).collect()
    }

    /// Keeps `encoded`, a photo in the format files ending in `extension` hold, under the next
    /// name; answers where.
    pub(crate) fn keep(&self, encoded: &[u8], extension: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.folder)?;
        let name = format!("{PREFIX}{:04}.{extension}", self.next_number());
        let (path, draft) = (
            self.folder.join(&name),
            self.folder.join(format!("{name}{DRAFT_SUFFIX}")),
        );
        fs::write(&draft, encoded)?;
        fs::rename(&draft, &path).inspect_err(|_| {
            // Nothing lists a draft; one left behind is only clutter.
            let _ = fs::remove_file(&draft);
        })?;
        Ok(path)
    }

    /// Deletes the photo at `path`, for good.
    pub(crate) fn delete(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn next_number(&self) -> u32 {
        let highest = self.photos().first().and_then(|path| number_of(path));
        highest.map_or(1, |number| number + 1)
    }
}

/// When the photo at `path` was taken: when its file was written; none where it cannot be read.
pub(crate) fn taken_at(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// The name a photo goes by: its file's.
pub(crate) fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The number in a photo's name; none for a file that is not a photo of the library's, a draft
/// among them.
fn number_of(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let (stem, _extension) = name.strip_prefix(PREFIX)?.split_once('.')?;
    if name.ends_with(DRAFT_SUFFIX) {
        return None;
    }
    stem.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(library: &Library) -> Vec<String> {
        library.photos().iter().map(|path| name_of(path)).collect()
    }

    #[test]
    fn photos_are_numbered_in_the_order_kept_and_listed_newest_first() {
        let folder = tempfile::tempdir().unwrap();
        let library = Library::at(folder.path().join("photos"));
        assert!(library.photos().is_empty(), "before the folder is made");

        let first = library.keep(b"one", "jpg").unwrap();
        library.keep(b"two", "heic").unwrap();
        assert_eq!(names(&library), ["IMG_0002.heic", "IMG_0001.jpg"]);
        assert_eq!(fs::read(first).unwrap(), b"one");
    }

    #[test]
    fn a_number_is_never_given_twice_while_a_later_photo_is_kept() {
        let folder = tempfile::tempdir().unwrap();
        let library = Library::at(folder.path());
        let first = library.keep(b"one", "jpg").unwrap();
        library.keep(b"two", "jpg").unwrap();

        library.delete(&first).unwrap();
        library.keep(b"three", "jpg").unwrap();
        assert_eq!(names(&library), ["IMG_0003.jpg", "IMG_0002.jpg"]);
    }

    #[test]
    fn drafts_and_other_files_are_not_photos() {
        let folder = tempfile::tempdir().unwrap();
        let library = Library::at(folder.path());
        for other in [
            "IMG_0007.jpg.saving",
            "notes.txt",
            "IMG_seven.jpg",
            "IMG_0009",
        ] {
            fs::write(folder.path().join(other), b"").unwrap();
        }
        assert!(library.photos().is_empty());
        assert_eq!(
            name_of(&library.keep(b"one", "jpg").unwrap()),
            "IMG_0001.jpg"
        );
    }
}
