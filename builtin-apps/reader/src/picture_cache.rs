//! The pictures fetched, kept as their servers sent them in the program's own `/cache/pictures`,
//! so that a picture is fetched once, shows again at the next launch and with no network, and
//! costs its site one request. A picture is kept under a name made from its address.
//!
//! `/cache` is what the system may throw away; the app also trims it itself at each launch, the
//! pictures least recently fetched first, so it does not grow without end.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where the pictures are kept between runs.
const FOLDER: &str = "/cache/pictures";

/// The most the pictures kept may take together, in bytes.
const MOST_KEPT: u64 = 150 * 1024 * 1024;

/// The picture fetched from `address`, where it is kept.
pub(crate) fn kept(address: &str) -> Option<Vec<u8>> {
    fs::read(path_of(Path::new(FOLDER), address)).ok()
}

/// Keeps `picture`, fetched from `address`; a failure is said and nothing kept.
pub(crate) fn keep(address: &str, picture: &[u8]) {
    let folder = Path::new(FOLDER);
    let written =
        fs::create_dir_all(folder).and_then(|()| fs::write(path_of(folder, address), picture));
    if let Err(error) = written {
        eprintln!("a picture is not kept: {error}");
    }
}

/// Removes the pictures least recently fetched until those kept take no more than [`MOST_KEPT`].
pub(crate) fn trim() {
    match trim_folder(Path::new(FOLDER), MOST_KEPT) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            eprintln!("the pictures kept are not trimmed: {error}");
        }
        _ => {}
    }
}

fn trim_folder(folder: &Path, most: u64) -> std::io::Result<()> {
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let written = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        kept.push((written, metadata.len(), entry.path()));
    }
    kept.sort();
    let mut total: u64 = kept.iter().map(|(_, length, _)| length).sum();
    for (_, length, path) in kept {
        if total <= most {
            break;
        }
        fs::remove_file(path)?;
        total -= length;
    }
    Ok(())
}

/// Where the picture fetched from `address` is kept in `folder`.
fn path_of(folder: &Path, address: &str) -> PathBuf {
    folder.join(format!("{:016x}", fnv1a(address)))
}

/// FNV-1a's 64-bit hash of `text`: a name for a file that stays the same across runs and builds.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_kept_under_a_name_of_its_address() {
        let folder = Path::new("/cache/pictures");
        let kept = path_of(folder, "https://example.org/a.jpg");
        assert_eq!(kept.parent(), Some(folder));
        assert_eq!(kept, path_of(folder, "https://example.org/a.jpg"));
        assert_ne!(kept, path_of(folder, "https://example.org/b.jpg"));
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn trimming_removes_the_oldest_pictures_first() {
        let folder = tempfile::tempdir().unwrap();
        for (name, age) in [("old", 300), ("middle", 200), ("new", 100)] {
            let path = folder.path().join(name);
            fs::write(&path, [0u8; 10]).unwrap();
            let written = SystemTime::now() - std::time::Duration::from_secs(age);
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(written)
                .unwrap();
        }
        trim_folder(folder.path(), 20).unwrap();
        assert!(!folder.path().join("old").exists());
        assert!(folder.path().join("middle").exists());
        assert!(folder.path().join("new").exists());
    }
}
