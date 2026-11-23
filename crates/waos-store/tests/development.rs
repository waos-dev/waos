//! Development installs: a program built from its source installed under its development id, a
//! program of its own beside the one published under the plain id, with its own folders and
//! settings, which an install again replaces the component of and keeps, and which no package
//! replaces, nor it a package.

mod support;

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use support::write_package;
use waos_package::{Manifest, Package, ProgramId, SigningKey};
use waos_store::{DevelopmentBuild, InstallError, Setting, SourceProxy, Store};

const TRIANGLE: &str = "dev.example.triangle";

fn triangle() -> ProgramId {
    TRIANGLE.parse().unwrap()
}

fn manifest(version: &str) -> Manifest {
    Manifest::new(triangle(), "Triangle", version.parse().unwrap()).unwrap()
}

/// The triangle's build at `version`, its component `component`, with no icon.
fn build<'a>(manifest: &'a Manifest, component: &'a [u8]) -> DevelopmentBuild<'a> {
    DevelopmentBuild {
        manifest,
        component,
        icon: None,
    }
}

fn store(folder: &Path) -> Store {
    Store::open(&folder.join("store"), SourceProxy::None).unwrap()
}

#[test]
fn a_development_install_is_a_program_of_its_own_beside_the_published_one() {
    let scratch = tempfile::tempdir().unwrap();
    let store = store(scratch.path());
    let key = SigningKey::generate();
    let package = write_package(scratch.path(), TRIANGLE, "1.0.0", &key);
    store.install(Package::open(&package).unwrap()).unwrap();
    let source = manifest("0.1.0");
    let installed = store
        .install_development(&build(&source, b"\0asm from source"))
        .unwrap();

    let development = triangle().development();
    assert_eq!(installed.manifest.id(), &development);
    assert_eq!(installed.name(), "Triangle (Development)");
    assert_eq!(installed.publisher(), None);
    let ids: Vec<String> = store
        .installed()
        .iter()
        .map(|program| program.manifest.id().to_string())
        .collect();
    assert_eq!(ids, [TRIANGLE, "dev.example.triangle.development"]);
    let published = store.installed_program(&triangle()).unwrap();
    assert_eq!(published.publisher(), Some(key.publisher()));
    assert_eq!(published.name(), "Triangle");

    let published_data = store.folders(&triangle()).unwrap().data;
    let development_data = store.folders(&development).unwrap().data;
    assert_ne!(published_data, development_data);
    fs::write(published_data.join("notes.txt"), "published").unwrap();
    fs::write(development_data.join("notes.txt"), "development").unwrap();
    assert_eq!(
        fs::read_to_string(published_data.join("notes.txt")).unwrap(),
        "published"
    );
    assert_eq!(
        fs::read(store.program(&development).unwrap()).unwrap(),
        b"\0asm from source"
    );
    assert_eq!(store.icon(&development), None, "the build had no icon");
}

#[test]
fn installing_again_replaces_the_component_and_keeps_its_data_and_settings() {
    let scratch = tempfile::tempdir().unwrap();
    let store = store(scratch.path());
    let development = triangle().development();
    let first = manifest("0.2.0");
    store
        .install_development(&build(&first, b"\0asm first"))
        .unwrap();
    let data = store.folders(&development).unwrap().data;
    fs::write(data.join("notes.txt"), "kept").unwrap();
    store
        .change_program_settings(&development, |settings| {
            settings.clipboard_read = Setting::Own(false);
        })
        .unwrap();
    let sha256 = |component: &[u8]| -> [u8; 32] { Sha256::digest(component).into() };
    assert!(store.holds_development(&first, &sha256(b"\0asm first"), None));
    assert!(!store.holds_development(&first, &sha256(b"\0asm second"), None));
    assert!(!store.holds_development(&first, &sha256(b"\0asm first"), Some(b"png")));

    let older = manifest("0.1.0");
    store
        .install_development(&build(&older, b"\0asm second"))
        .unwrap();
    assert_eq!(
        fs::read(store.program(&development).unwrap()).unwrap(),
        b"\0asm second",
        "whatever its version"
    );
    assert_eq!(fs::read_to_string(data.join("notes.txt")).unwrap(), "kept");
    let settings = store.program_settings(&development).unwrap();
    assert_eq!(settings.clipboard_read, Setting::Own(false));
}

#[test]
fn no_package_replaces_a_development_install_nor_a_development_build_a_package() {
    let scratch = tempfile::tempdir().unwrap();
    let store = store(scratch.path());
    let source = manifest("0.1.0");
    store
        .install_development(&build(&source, b"\0asm from source"))
        .unwrap();
    let development = "dev.example.triangle.development";
    let key = SigningKey::generate();
    let package = write_package(scratch.path(), development, "9.0.0", &key);
    let error = store.install(Package::open(&package).unwrap()).unwrap_err();
    assert!(matches!(error, InstallError::Development(_)), "{error}");
    assert!(store.installed()[0].is_development());

    let other = tempfile::tempdir().unwrap();
    let published = Store::open(&other.path().join("store"), SourceProxy::None).unwrap();
    published.install(Package::open(&package).unwrap()).unwrap();
    let error = published
        .install_development(&build(&source, b"\0asm from source"))
        .unwrap_err();
    assert!(matches!(error, InstallError::Published(_)), "{error}");
    assert_eq!(
        published.installed()[0].publisher(),
        Some(key.publisher()),
        "the package's install stays"
    );
}

#[tokio::test]
async fn what_is_installed_tells_each_install_and_removal() {
    let scratch = tempfile::tempdir().unwrap();
    let store = store(scratch.path());
    let mut changes = store.installed_changes();
    changes.borrow_and_update();
    let source = manifest("0.1.0");
    store
        .install_development(&build(&source, b"\0asm from source"))
        .unwrap();
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    store.remove(&triangle().development()).unwrap();
    assert!(changes.has_changed().unwrap());
}
