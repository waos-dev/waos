//! The store: each app's activity, kept in a directory the app gives it.
//!
//! The invariant a later editor will break: every change to the apps' activity happens under one
//! lock, so two edits never interleave.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use crate::permissions::Permissions;
use waos_package::ProgramId;

use crate::store::StoreError;

/// The folder of installed programs, in the store's directory.
const PROGRAMS: &str = "programs";

/// Each app's activity. One store opens a
/// directory; clones share it.
#[derive(Clone)]
pub struct Store {
    directory: PathBuf,
    /// Held while the directory changes, so changes happen one at a time.
    changes: Arc<Mutex<()>>,
    /// Whether each app's activity is recorded, which its settings say.
    permissions: Permissions,
}

/// A store as a thread that should end with it holds it.
pub(crate) struct WeakStore {
    directory: PathBuf,
    changes: Weak<Mutex<()>>,
    permissions: Permissions,
}

impl Store {
    /// The store in `directory`, made when it is not there yet, recording each app's activity as `permissions` say: what was kept of an app whose record
    /// the user turns off goes as the setting changes, on a thread of the store's own.
    pub fn open(directory: &Path, permissions: Permissions) -> Result<Store, StoreError> {
        let store = Store {
            directory: directory.to_path_buf(),
            changes: Arc::default(),
            permissions,
        };
        let programs = store.programs_folder();
        fs::create_dir_all(&programs).map_err(StoreError::io_at(&programs))?;
        store.follow_settings()?;
        Ok(store)
    }

    /// The app `id` was removed, its folder with it: its activity goes.
    pub fn forget(&self, id: &ProgramId) {
        let _changing = self.lock_changes();
        self.forget_days(id.as_str());
    }

    pub(crate) fn downgrade(&self) -> WeakStore {
        WeakStore {
            directory: self.directory.clone(),
            changes: Arc::downgrade(&self.changes),
            permissions: self.permissions.clone(),
        }
    }

    /// The store `weak` holds, while a clone of it is held elsewhere.
    pub(crate) fn upgrade(weak: &WeakStore) -> Option<Store> {
        Some(Store {
            directory: weak.directory.clone(),
            changes: weak.changes.upgrade()?,
            permissions: weak.permissions.clone(),
        })
    }

    pub(crate) fn lock_changes(&self) -> MutexGuard<'_, ()> {
        // The lock guards no value, so a change that panicked leaves nothing half-changed in it.
        self.changes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn programs_folder(&self) -> PathBuf {
        self.directory.join(PROGRAMS)
    }

    /// What the user lets each app use, which says whether its activity is recorded.
    pub(crate) fn permissions(&self) -> &Permissions {
        &self.permissions
    }

    /// The store's directory.
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    /// Whether the store has a folder of the program named `program`: installed, or run in
    /// development.
    pub(crate) fn has_folder_of(&self, program: &str) -> bool {
        self.programs_folder().join(program).is_dir()
    }
}
