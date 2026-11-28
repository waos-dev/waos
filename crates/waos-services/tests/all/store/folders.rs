//! Each app's own folders: made at its first run, kept across an install of its id, and going
//! with it when it is removed.

use std::fs;

use crate::support::{block_on, open_store, packages, remove, write_package};
use waos_package::{ProgramId, SigningKey};

const TRIANGLE: &str = "dev.example.triangle";

fn triangle() -> ProgramId {
    TRIANGLE.parse().unwrap()
}

#[test]
fn a_programs_own_folders_are_made_at_its_first_run_and_kept_until_it_is_removed() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let store = open_store(&directory);
    let folders = store.folders(&triangle()).unwrap();
    assert_eq!(
        folders.data,
        directory.join("programs").join(TRIANGLE).join("data")
    );
    assert_eq!(
        folders.cache,
        directory.join("programs").join(TRIANGLE).join("cache")
    );
    fs::write(folders.data.join("notes.txt"), "kept").unwrap();

    let reopened = open_store(&directory);
    let packages = packages(&directory);
    assert!(packages.installed().is_empty(), "run, not installed");
    assert_eq!(fs::read(folders.data.join("notes.txt")).unwrap(), b"kept");

    let package = write_package(scratch.path(), TRIANGLE, "1.0.0", &SigningKey::generate());
    block_on(crate::support::install(&packages, &package)).unwrap();
    assert_eq!(reopened.folders(&triangle()).unwrap(), folders);
    assert_eq!(
        fs::read(folders.data.join("notes.txt")).unwrap(),
        b"kept",
        "the installed program is the one its development runs were"
    );

    block_on(remove(&reopened, &packages, &triangle()));
    assert!(!folders.data.exists() && !folders.cache.exists());
}
