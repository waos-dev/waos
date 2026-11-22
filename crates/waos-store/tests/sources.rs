//! The sources the user added, reading a source's index, and installing what it lists, against
//! a local HTTP server.

mod support;

use support::{Server, component_of, listing, write_package};
use waos_package::{Index, SigningKey};
use waos_store::{InstallError, Source, SourceError, SourceProxy, Store, StoreError};

const TRIANGLE: &str = "dev.example.triangle";

fn source(text: &str) -> Source {
    text.parse().unwrap()
}

#[test]
fn sources_are_kept_in_the_order_added() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let store = Store::open(&directory, SourceProxy::None).unwrap();
    let github = source("github.com/example/triangle");
    let index = source("https://example.com/waos/index.json");
    store.add_source(github.clone()).unwrap();
    store.add_source(index.clone()).unwrap();
    store.add_source(github.clone()).unwrap();
    assert_eq!(store.sources(), [github.clone(), index.clone()]);
    assert_eq!(
        Store::open(&directory, SourceProxy::None)
            .unwrap()
            .sources(),
        [github.clone(), index.clone()]
    );
    store.remove_source(&github).unwrap();
    assert_eq!(store.sources(), [index]);
    assert!(matches!(
        store.remove_source(&github).unwrap_err(),
        StoreError::NotAdded(_)
    ));
}

#[test]
fn the_first_sources_are_made_once_and_a_removed_one_never_comes_back() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let store = Store::open(&directory, SourceProxy::None).unwrap();
    let first = [source("https://list.waos.app/index.json")];
    store.start_sources_with(&first).unwrap();
    assert_eq!(store.sources(), first);
    store.remove_source(&first[0]).unwrap();
    store.start_sources_with(&first).unwrap();
    assert!(store.sources().is_empty());
}

#[test]
fn the_first_sources_leave_a_list_added_to_already_as_it_is() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let github = source("github.com/example/triangle");
    store.add_source(github.clone()).unwrap();
    store
        .start_sources_with(&[source("https://list.waos.app/index.json")])
        .unwrap();
    assert_eq!(store.sources(), [github]);
}

#[tokio::test]
async fn a_source_is_read_from_its_index() {
    let scratch = tempfile::tempdir().unwrap();
    let key = SigningKey::generate();
    let package = write_package(scratch.path(), TRIANGLE, "1.2.0", &key);
    let index = Index {
        programs: vec![listing(&package, TRIANGLE, "1.2.0")],
    };
    let server = Server::serve(vec![(
        "waos/index.json".to_owned(),
        index.to_json().into_bytes(),
    )])
    .await;
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let read = store
        .read_source(&source(&server.url("waos/index.json")))
        .await
        .unwrap();
    assert_eq!(read, index);
}

#[tokio::test]
async fn a_source_with_no_index_says_what_the_server_answered() {
    let scratch = tempfile::tempdir().unwrap();
    let server = Server::serve(vec![("index.json".to_owned(), b"<html>".to_vec())]).await;
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let missing = store
        .read_source(&source(&server.url("missing/index.json")))
        .await
        .unwrap_err();
    assert!(matches!(missing, SourceError::Status { .. }), "{missing}");
    assert!(missing.to_string().contains("404 Not Found"), "{missing}");
    let not_an_index = store
        .read_source(&source(&server.url("index.json")))
        .await
        .unwrap_err();
    assert!(
        matches!(not_an_index, SourceError::NotAnIndex { .. }),
        "{not_an_index}"
    );
}

#[tokio::test]
async fn a_listed_package_is_downloaded_and_installed() {
    let scratch = tempfile::tempdir().unwrap();
    let key = SigningKey::generate();
    let package = write_package(scratch.path(), TRIANGLE, "1.2.0", &key);
    let listed = listing(&package, TRIANGLE, "1.2.0");
    let server = Server::serve(vec![(
        format!("waos/{}", listed.package),
        std::fs::read(&package).unwrap(),
    )])
    .await;
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let source = source(&server.url("waos/index.json"));
    let installed = store.install_from(&source, &listed).await.unwrap();
    assert_eq!(installed.publisher, key.publisher());
    let component = store.program(&TRIANGLE.parse().unwrap()).unwrap();
    assert_eq!(
        std::fs::read(component).unwrap(),
        component_of(TRIANGLE, "1.2.0")
    );
}

#[tokio::test]
async fn a_download_unlike_its_listing_is_refused_before_it_is_opened() {
    let scratch = tempfile::tempdir().unwrap();
    let package = write_package(scratch.path(), TRIANGLE, "1.2.0", &SigningKey::generate());
    let mut listed = listing(&package, TRIANGLE, "1.2.0");
    listed.sha256[0] ^= 1;
    let server = Server::serve(vec![(
        listed.package.clone(),
        std::fs::read(&package).unwrap(),
    )])
    .await;
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let error = store
        .install_from(&source(&server.url("index.json")), &listed)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            InstallError::Download(SourceError::Sha256Mismatch { .. })
        ),
        "{error}"
    );
    assert!(store.installed().is_empty());
}

#[tokio::test]
async fn a_package_that_is_not_the_program_listed_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let square = write_package(
        scratch.path(),
        "dev.example.square",
        "1.2.0",
        &SigningKey::generate(),
    );
    let listed = waos_package::ListedProgram {
        id: TRIANGLE.parse().unwrap(),
        ..listing(&square, "dev.example.square", "1.2.0")
    };
    let server = Server::serve(vec![(
        listed.package.clone(),
        std::fs::read(&square).unwrap(),
    )])
    .await;
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let error = store
        .install_from(&source(&server.url("index.json")), &listed)
        .await
        .unwrap_err();
    assert!(matches!(error, InstallError::NotAsListed { .. }), "{error}");
    assert!(store.installed().is_empty());
}

#[tokio::test]
async fn an_update_from_a_source_must_be_signed_with_the_installed_key() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let first = write_package(scratch.path(), TRIANGLE, "1.0.0", &SigningKey::generate());
    store
        .install(waos_package::Package::open(&first).unwrap())
        .unwrap();
    let other = write_package(scratch.path(), TRIANGLE, "2.0.0", &SigningKey::generate());
    let listed = listing(&other, TRIANGLE, "2.0.0");
    let server = Server::serve(vec![(
        listed.package.clone(),
        std::fs::read(&other).unwrap(),
    )])
    .await;
    let error = store
        .install_from(&source(&server.url("index.json")), &listed)
        .await
        .unwrap_err();
    assert!(
        matches!(error, InstallError::DifferentPublisher { .. }),
        "{error}"
    );
    let installed = store.installed();
    assert_eq!(installed[0].manifest.version().to_string(), "1.0.0");
}

#[tokio::test]
async fn downloads_stage_inside_the_store_and_interrupted_ones_are_cleared_on_open() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let key = SigningKey::generate();
    let package = write_package(scratch.path(), TRIANGLE, "1.2.0", &key);
    let listed = listing(&package, TRIANGLE, "1.2.0");
    let server = Server::serve(vec![(
        format!("waos/{}", listed.package),
        std::fs::read(&package).unwrap(),
    )])
    .await;
    let store = Store::open(&directory, SourceProxy::None).unwrap();
    let download = store
        .download(&source(&server.url("waos/index.json")), &listed)
        .await
        .unwrap();
    let downloads = directory.join("downloads");
    assert_eq!(
        std::fs::read_dir(&downloads).unwrap().count(),
        1,
        "downloaded into the store's downloads folder"
    );
    drop(download);
    assert_eq!(
        std::fs::read_dir(&downloads).unwrap().count(),
        0,
        "temporary file removed when dropped"
    );

    std::fs::write(downloads.join("interrupted.tmp"), b"partial").unwrap();
    let _reopened = Store::open(&directory, SourceProxy::None).unwrap();
    assert!(
        !downloads.exists(),
        "interrupted downloads cleared on Store::open"
    );
}
