//! Each running program's temporary folder, which it sees at `/tmp`: a folder of its own beneath
//! the engine's folder of them (`WaOSConfig::temporary_folders`), the app's own among its caches,
//! on the volume of the programs' `/data`, made as the program starts and removed with all it holds once the program and its workers have
//! ended, however they ended. A folder a crash of WaOS itself left behind is removed when the next
//! engine starts on the same folder.
//!
//! Each folder is held by a lock on a file beside it, `<folder>.lock`, for as long as its program
//! runs: an engine starting removes each folder whose lock it can take, whichever process left it,
//! and leaves those another engine holds, in this process or another.
//!
//! The invariant: a folder is removed only by the one guard that made it, or by an engine that took
//! its lock, which no running program's guard lets go of; a folder whose lock is missing is left
//! where it is, never taken for one nobody holds.

use std::fs::{self, File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, Result};
use tracing::warn;

/// What a folder's lock file is named after it.
const LOCK: &str = "lock";

/// The folders made so far by this process, every engine's in it, which name each new one apart
/// from another engine's on the same folder.
static MADE: AtomicU64 = AtomicU64::new(0);

/// How many names a folder is tried under before it is given up: a name is taken only where a
/// folder of an earlier process of the same id is still held.
const ATTEMPTS: usize = 8;

/// The engine's folder of temporary folders, one per running program.
pub(crate) struct TemporaryFolders {
    root: PathBuf,
}

/// One program's temporary folder, held while the program runs and removed when it is dropped.
pub(crate) struct TemporaryFolder {
    path: PathBuf,
    lock_path: PathBuf,
    /// Taken while the folder is the program's; let go of as it is dropped, after the folder goes.
    _lock: File,
}

impl TemporaryFolders {
    /// The folders at `root`, made where it is not yet, with what a crash left there removed.
    pub(crate) fn new(root: PathBuf) -> Result<TemporaryFolders> {
        fs::create_dir_all(&root).with_context(|| format!("making {}", root.display()))?;
        remove_left_behind(&root);
        Ok(TemporaryFolders { root })
    }

    /// A new folder for the program `name`, empty and held; none, with a warning, where the
    /// system makes none.
    pub(crate) fn make(&self, name: &str) -> Option<TemporaryFolder> {
        let mut made = Err(io::ErrorKind::AlreadyExists.into());
        for _ in 0..ATTEMPTS {
            made = TemporaryFolder::make(self.root.join(new_name(name)));
            if !matches!(&made, Err(error) if error.kind() == io::ErrorKind::AlreadyExists) {
                break;
            }
        }
        if let Err(error) = &made {
            warn!(program = name, %error, "the program has no /tmp: its folder was not made");
        }
        made.ok()
    }
}

impl TemporaryFolder {
    /// Takes the lock beside `path`, then makes the folder at `path`, both new.
    fn make(path: PathBuf) -> io::Result<TemporaryFolder> {
        let lock_path = lock_path_of(&path);
        let lock = File::create_new(&lock_path)?;
        lock.lock()?;
        if let Err(error) = fs::create_dir(&path) {
            let _ = fs::remove_file(&lock_path);
            return Err(error);
        }
        Ok(TemporaryFolder {
            path,
            lock_path,
            _lock: lock,
        })
    }

    /// Where the folder is on the machine.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryFolder {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            warn!(folder = %self.path.display(), %error, "a program's /tmp was not removed");
        }
        let _ = fs::remove_file(&self.lock_path);
    }
}

/// Removes each folder in `root` whose lock nobody holds, with its lock file.
fn remove_left_behind(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let lock_path = entry.path();
        if lock_path
            .extension()
            .is_none_or(|extension| extension != LOCK)
        {
            continue;
        }
        let Ok(lock) = File::open(&lock_path) else {
            continue;
        };
        match lock.try_lock() {
            Ok(()) => {
                let folder = lock_path.with_extension("");
                if let Err(error) = fs::remove_dir_all(&folder)
                    && error.kind() != io::ErrorKind::NotFound
                {
                    warn!(folder = %folder.display(), %error, "a /tmp left behind was not removed");
                    continue;
                }
                let _ = fs::remove_file(&lock_path);
            }
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(error)) => {
                warn!(lock = %lock_path.display(), %error, "a /tmp's lock was not read");
            }
        }
    }
}

/// A name for a new folder of the program `name`: its name, this process's id and a number of its
/// own.
fn new_name(name: &str) -> String {
    let made = MADE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{made}", plain_name(name), std::process::id())
}

/// The lock file of the folder at `path`, beside it.
fn lock_path_of(path: &Path) -> PathBuf {
    let mut lock = path.as_os_str().to_owned();
    lock.push(".");
    lock.push(LOCK);
    PathBuf::from(lock)
}

/// `name` with only letters, digits, `-`, `_` and `.`, each other character an `_`, as a folder's
/// name on every system.
fn plain_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_made_empty_and_removed_with_what_it_holds_when_dropped() {
        let scratch = tempfile::tempdir().unwrap();
        let folders = TemporaryFolders::new(scratch.path().join("tmp")).unwrap();
        let folder = folders.make("a program").unwrap();
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 0);
        fs::create_dir(folder.path().join("inside")).unwrap();
        fs::write(folder.path().join("inside/file"), "kept until the end").unwrap();

        let path = folder.path().to_owned();
        drop(folder);
        assert!(!path.exists());
        assert_eq!(fs::read_dir(scratch.path().join("tmp")).unwrap().count(), 0);
    }

    #[test]
    fn an_engine_starting_removes_what_nobody_holds_and_leaves_what_another_holds() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().join("tmp");
        let first = TemporaryFolders::new(root.clone()).unwrap();
        let held = first.make("held").unwrap();
        // As a crash leaves a folder: it and its lock file, whose lock went with the process.
        let crashed = root.join("crashed-1-0");
        fs::create_dir(&crashed).unwrap();
        fs::write(crashed.join("file"), "").unwrap();
        File::create(lock_path_of(&crashed)).unwrap();

        let _second = TemporaryFolders::new(root.clone()).unwrap();
        assert!(held.path().exists(), "another engine's folder stays");
        assert!(!crashed.exists(), "a crash's folder goes");
        assert!(!lock_path_of(&crashed).exists());
    }

    #[test]
    fn engines_on_one_folder_make_their_programs_folders_apart() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().join("tmp");
        let first = TemporaryFolders::new(root.clone()).unwrap();
        let second = TemporaryFolders::new(root).unwrap();
        let one = first.make("notes").unwrap();
        let other = second.make("notes").unwrap();
        assert_ne!(one.path(), other.path());
    }

    #[test]
    fn a_name_is_plain_on_every_system() {
        assert_eq!(plain_name("Video Player: 2/3"), "Video_Player__2_3");
    }
}
