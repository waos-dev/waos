//! Packages the system hands WaOS without a path, as Android hands a document opened from its
//! Files app (a `content:` document, Waft's `PickedFile::Handle` or `Stream`): each is copied into
//! a file of WaOS's own, as Android's own package installer copies what it is handed, since a
//! package is opened from a file on disk and kept open until it is installed
//! (`waos_package::Package::open`). Each copy is in a folder of its own under the folder kept for
//! packages being asked about, named as the document is, which carries the `.wapk` extension a
//! package is known by (`logic::opened_package`); a name that is not a plain file name is refused.
//!
//! The invariant: a copy lives exactly as long as the question about it: it is removed once the
//! question is answered, or once it turns out not to be a package; copies an earlier run left are
//! removed as WaOS starts, before any is made.

use std::future::Future;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use waft::ui::{Files, PickedFile};

/// The folder the copies are made in, and the number of the next one.
#[derive(Clone)]
pub(crate) struct PackageCopies {
    folder: PathBuf,
    next: Arc<AtomicU64>,
}

impl PackageCopies {
    /// The copies made in `folder` from now on; what an earlier run left there is removed.
    pub(crate) fn clear(folder: PathBuf) -> io::Result<PackageCopies> {
        match std::fs::remove_dir_all(&folder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::create_dir_all(&folder)?;
        Ok(PackageCopies {
            folder,
            next: Arc::default(),
        })
    }

    /// Writes `bytes`, the whole document named `name`, as a copy of WaOS's own, and answers
    /// where; a name that is not a plain file name is refused.
    pub(crate) fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        if !is_plain_file_name(name) {
            bail!("{name:?} is not a file's name WaOS can keep a package under");
        }
        let number = self.next.fetch_add(1, Ordering::SeqCst);
        let folder = self.folder.join(number.to_string());
        std::fs::create_dir_all(&folder).with_context(|| format!("making {}", folder.display()))?;
        let copy = folder.join(name);
        std::fs::write(&copy, bytes).with_context(|| format!("writing {}", copy.display()))?;
        Ok(copy)
    }

    /// Removes `file` with its folder, when it is one of the copies; anything else is left.
    pub(crate) fn remove(&self, file: &Path) {
        let Some(folder) = file.parent() else {
            return;
        };
        if folder.parent() != Some(self.folder.as_path()) {
            return;
        }
        if let Err(error) = std::fs::remove_dir_all(folder) {
            tracing::warn!(copy = %file.display(), %error, "a package's copy could not be removed");
        }
    }
}

/// Whether `name` names a file in a folder, and nothing else: no folder, no parent, no root.
fn is_plain_file_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(only)), None) if only == name
    )
}

/// The name of a document handed without a path, as the system gives it; none for a path.
pub(crate) fn document_name(document: &PickedFile) -> Option<String> {
    match document {
        PickedFile::Path(_) => None,
        PickedFile::Handle(handle) => Some(handle.name().to_owned()),
        PickedFile::Stream(stream) => Some(stream.name().to_owned()),
    }
}

/// The whole of `document`, read from its start to its end, a path through the platform's
/// `files`: the read is started now, and the answer awaited wherever the caller awaits it.
pub(crate) fn read_document(
    document: PickedFile,
    files: Option<&dyn Files>,
) -> impl Future<Output = Result<Vec<u8>>> + use<> {
    let name = document_name(&document).unwrap_or_default();
    let reading = document.read_all(files);
    async move { reading.await.with_context(|| format!("{name} could not be read")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_made_under_the_documents_name_and_removed_with_its_folder() {
        let folder = tempfile::tempdir().unwrap();
        let copies = PackageCopies::clear(folder.path().join("packages")).unwrap();
        let first = copies.write("notes.wapk", b"one").unwrap();
        let second = copies.write("notes.wapk", b"two").unwrap();
        assert_eq!(first.file_name().unwrap(), "notes.wapk");
        assert_ne!(
            first, second,
            "two documents of one name, each its own copy"
        );
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        copies.remove(&first);
        assert!(!first.exists() && !first.parent().unwrap().exists());
        assert!(second.exists(), "the other copy stays");
    }

    #[test]
    fn a_name_that_is_not_a_plain_file_name_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let copies = PackageCopies::clear(folder.path().join("packages")).unwrap();
        for name in [
            "",
            ".",
            "..",
            "../notes.wapk",
            "a/notes.wapk",
            "/notes.wapk",
        ] {
            assert!(copies.write(name, b"x").is_err(), "{name:?}");
        }
        assert!(copies.write("Notes 1.0.wapk", b"x").is_ok());
    }

    #[test]
    fn copies_an_earlier_run_left_are_removed_at_the_start() {
        let folder = tempfile::tempdir().unwrap();
        let packages = folder.path().join("packages");
        let left = PackageCopies::clear(packages.clone())
            .unwrap()
            .write("notes.wapk", b"x")
            .unwrap();
        PackageCopies::clear(packages.clone()).unwrap();
        assert!(!left.exists());
        assert!(packages.is_dir());
    }

    #[test]
    fn only_a_copy_is_removed() {
        let folder = tempfile::tempdir().unwrap();
        let copies = PackageCopies::clear(folder.path().join("packages")).unwrap();
        let elsewhere = folder.path().join("mine.wapk");
        std::fs::write(&elsewhere, b"x").unwrap();
        copies.remove(&elsewhere);
        assert!(elsewhere.exists());
    }
}
