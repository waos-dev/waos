//! Installing, updating and removing packages: a new id installs, the same id and key at a newer
//! version updates, and anything else leaves the program installed as it was.

mod support;

use std::fs;
use std::path::Path;

use support::{component_of, write_package};
use waos_package::{Package, ProgramId, SigningKey, Version};
use waos_store::{InstallError, SourceProxy, Store, StoreError};

const TRIANGLE: &str = "dev.example.triangle";

fn triangle() -> ProgramId {
    TRIANGLE.parse().unwrap()
}

fn package(folder: &Path, version: &str, key: &SigningKey) -> Package {
    Package::open(&write_package(folder, TRIANGLE, version, key)).unwrap()
}

/// The versions installed, by id.
fn installed(store: &Store) -> Vec<(String, Version)> {
    store
        .installed()
        .into_iter()
        .map(|program| {
            let manifest = program.manifest;
            (manifest.id().to_string(), manifest.version().clone())
        })
        .collect()
}

fn triangle_at(version: &str) -> Vec<(String, Version)> {
    vec![(TRIANGLE.to_owned(), version.parse().unwrap())]
}

/// What the store runs of the triangle.
fn component(store: &Store) -> Vec<u8> {
    fs::read(store.program(&triangle()).unwrap()).unwrap()
}

#[test]
fn a_new_program_installs_and_its_component_is_what_runs() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let key = SigningKey::generate();
    let installed_program = store
        .install(package(scratch.path(), "1.0.0", &key))
        .unwrap();
    assert_eq!(installed_program.publisher(), Some(key.publisher()));
    assert_eq!(installed(&store), triangle_at("1.0.0"));
    assert_eq!(store.installed()[0].publisher(), Some(key.publisher()));
    assert_eq!(component(&store), component_of(TRIANGLE, "1.0.0"));
}

#[test]
fn the_same_key_at_a_newer_version_updates_it() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let key = SigningKey::generate();
    store
        .install(package(scratch.path(), "1.0.0", &key))
        .unwrap();
    store
        .install(package(scratch.path(), "1.1.0", &key))
        .unwrap();
    assert_eq!(installed(&store), triangle_at("1.1.0"));
    assert_eq!(component(&store), component_of(TRIANGLE, "1.1.0"));
}

#[test]
fn another_key_is_refused_naming_both_keys() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let first = SigningKey::generate();
    let second = SigningKey::generate();
    store
        .install(package(scratch.path(), "1.0.0", &first))
        .unwrap();
    let error = store
        .install(package(scratch.path(), "2.0.0", &second))
        .unwrap_err();
    assert!(
        matches!(error, InstallError::DifferentPublisher { .. }),
        "{error}"
    );
    let message = error.to_string();
    assert!(
        message.contains(&first.publisher().fingerprint()),
        "{message}"
    );
    assert!(
        message.contains(&second.publisher().fingerprint()),
        "{message}"
    );
    assert_eq!(installed(&store), triangle_at("1.0.0"));
}

#[test]
fn a_version_not_newer_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let key = SigningKey::generate();
    store
        .install(package(scratch.path(), "1.1.0", &key))
        .unwrap();
    for version in ["1.1.0", "1.0.9", "1.1.0-beta.1"] {
        let error = store
            .install(package(scratch.path(), version, &key))
            .unwrap_err();
        assert!(matches!(error, InstallError::NotNewer { .. }), "{error}");
    }
    assert_eq!(installed(&store), triangle_at("1.1.0"));
}

#[test]
fn an_install_that_fails_while_unpacking_leaves_the_earlier_one() {
    let scratch = tempfile::tempdir().unwrap();
    let store_directory = scratch.path().join("store");
    let store = Store::open(&store_directory, SourceProxy::None).unwrap();
    let key = SigningKey::generate();
    store
        .install(package(scratch.path(), "1.0.0", &key))
        .unwrap();
    let path = write_package(scratch.path(), TRIANGLE, "1.1.0", &key);
    let opened = Package::open(&path).unwrap();
    fs::write(&path, b"changed after it was opened").unwrap();
    let error = store.install(opened).unwrap_err();
    assert!(matches!(error, InstallError::Package(_)), "{error}");
    assert_eq!(installed(&store), triangle_at("1.0.0"));
    assert_eq!(component(&store), component_of(TRIANGLE, "1.0.0"));
    let program_folder = store_directory.join("programs").join(TRIANGLE);
    assert_eq!(fs::read_dir(program_folder).unwrap().count(), 2);
}

#[test]
fn a_removed_program_is_gone_and_cannot_be_removed_again() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let key = SigningKey::generate();
    store
        .install(package(scratch.path(), "1.0.0", &key))
        .unwrap();
    store.remove(&triangle()).unwrap();
    assert!(store.installed().is_empty());
    assert_eq!(store.program(&triangle()), None);
    assert!(matches!(
        store.remove(&triangle()).unwrap_err(),
        StoreError::NotInstalled(_)
    ));
    let another_key = SigningKey::generate();
    store
        .install(package(scratch.path(), "0.1.0", &another_key))
        .unwrap();
    assert_eq!(installed(&store), triangle_at("0.1.0"));
}

#[test]
fn a_store_opened_again_has_what_was_installed() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let key = SigningKey::generate();
    Store::open(&directory, SourceProxy::None)
        .unwrap()
        .install(package(scratch.path(), "1.0.0", &key))
        .unwrap();
    let store = Store::open(&directory, SourceProxy::None).unwrap();
    assert_eq!(installed(&store), triangle_at("1.0.0"));
    assert_eq!(store.installed()[0].publisher(), Some(key.publisher()));
}
