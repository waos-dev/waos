//! The storage tab's state: the mounts of the program chosen, read when it is chosen, and the
//! entries of each folder opened, read once as it is opened and kept until another program is
//! chosen. The tree the tab draws is this state, and no read is made twice.
//!
//! The invariant: every folder whose entries are shown is inside a mount, read through the
//! program's own mounts, so a path the program could not read is never read here.

use std::collections::{HashMap, HashSet};

use wasdk::inspect::storage::{Entry, Mount};

/// What has been read, or is being read, or why it could not be.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Load<T> {
    /// Being read now.
    #[default]
    Looking,
    /// Read.
    Ready(T),
    /// Not read, for this reason.
    Failed(String),
}

/// The storage of the program chosen.
#[derive(Default)]
pub(crate) struct Storage {
    /// Its mounts.
    mounts: Load<Vec<Mount>>,
    /// Each folder's entries by its path inside the program.
    folders: HashMap<String, Load<Vec<Entry>>>,
    /// The folders the user opened.
    expanded: HashSet<String>,
}

impl Storage {
    /// The storage of another program: nothing read, nothing open.
    pub(crate) fn reset(&mut self) {
        *self = Storage::default();
    }

    pub(crate) fn mounts(&self) -> &Load<Vec<Mount>> {
        &self.mounts
    }

    pub(crate) fn entries(&self, path: &str) -> Option<&Load<Vec<Entry>>> {
        self.folders.get(path)
    }

    pub(crate) fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Opens `path`, which the tree draws expanded from now on; answers whether its entries must
    /// be read.
    pub(crate) fn expand(&mut self, path: &str) -> bool {
        self.expanded.insert(path.to_owned());
        !self.folders.contains_key(path)
    }

    pub(crate) fn collapse(&mut self, path: &str) {
        self.expanded.remove(path);
    }

    /// The mounts arrived.
    pub(crate) fn mounts_arrived(&mut self, mounts: Vec<Mount>) {
        self.mounts = Load::Ready(mounts);
    }

    /// The entries of `path` are being read.
    pub(crate) fn folder_looking(&mut self, path: &str) {
        self.folders.insert(path.to_owned(), Load::Looking);
    }

    /// The entries of `path` arrived.
    pub(crate) fn folder_arrived(&mut self, path: &str, entries: Vec<Entry>) {
        self.folders.insert(path.to_owned(), Load::Ready(entries));
    }

    /// The entries of `path` could not be read, for `why`.
    pub(crate) fn folder_failed(&mut self, path: &str, why: String) {
        self.folders.insert(path.to_owned(), Load::Failed(why));
    }
}
