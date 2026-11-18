use std::fs;
use std::path::{Path, PathBuf};

use super::{Drift, FolderCopy, system_path};

const COPY: FolderCopy = FolderCopy {
    source: "wit",
    destination: "crate/wit",
    left_out: &[],
};

/// A workspace's root holding `wit/` with a world and a dependency.
fn root_with_wit() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "wit/program.wit", "package a:program;");
    write(root.path(), "wit/deps/b/package.wit", "package a:b;");
    root
}

fn write(root: &Path, file: &str, contents: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

#[test]
fn a_copy_made_is_its_source() {
    let root = root_with_wit();
    COPY.sync(root.path()).unwrap();
    assert_eq!(COPY.drift(root.path()).unwrap(), []);
    assert_eq!(
        fs::read_to_string(root.path().join("crate/wit/deps/b/package.wit")).unwrap(),
        "package a:b;"
    );
}

#[test]
fn each_file_that_drifted_is_named() {
    let root = root_with_wit();
    COPY.sync(root.path()).unwrap();
    write(root.path(), "wit/program.wit", "package a:program2;");
    write(root.path(), "wit/deps/c/package.wit", "package a:c;");
    write(root.path(), "crate/wit/old.wit", "package a:old;");
    assert_eq!(
        COPY.drift(root.path()).unwrap(),
        [
            Drift::Missing {
                copy: PathBuf::from("crate/wit/deps/c/package.wit"),
                source: PathBuf::from("wit/deps/c/package.wit"),
            },
            Drift::Extra {
                copy: PathBuf::from("crate/wit/old.wit"),
            },
            Drift::Differs {
                copy: PathBuf::from("crate/wit/program.wit"),
                source: PathBuf::from("wit/program.wit"),
            },
        ]
    );
}

#[test]
fn a_copy_not_made_misses_every_file() {
    let root = root_with_wit();
    let drift = COPY.drift(root.path()).unwrap();
    let named: Vec<String> = drift.iter().map(ToString::to_string).collect();
    let missing = |copy: &str, source: &str| {
        format!(
            "{} is missing, a copy of {}",
            system_path(copy).display(),
            system_path(source).display()
        )
    };
    assert_eq!(
        named,
        [
            missing("crate/wit/deps/b/package.wit", "wit/deps/b/package.wit"),
            missing("crate/wit/program.wit", "wit/program.wit"),
        ]
    );
}

#[test]
fn syncing_again_leaves_nothing_of_the_copy_before() {
    let root = root_with_wit();
    COPY.sync(root.path()).unwrap();
    fs::rename(
        root.path().join("wit/program.wit"),
        root.path().join("wit/renamed.wit"),
    )
    .unwrap();
    COPY.sync(root.path()).unwrap();
    assert_eq!(COPY.drift(root.path()).unwrap(), []);
    assert!(!root.path().join("crate/wit/program.wit").exists());
}

#[test]
fn a_path_left_out_is_neither_copied_nor_drift() {
    let root = root_with_wit();
    let copy = FolderCopy {
        left_out: &["deps/b"],
        ..COPY
    };
    copy.sync(root.path()).unwrap();
    assert!(!root.path().join("crate/wit/deps/b/package.wit").exists());
    assert!(root.path().join("crate/wit/program.wit").exists());
    assert_eq!(copy.drift(root.path()).unwrap(), []);
}
