//! The sources the user added, reading a source's index, and installing what it lists, against
//! a local HTTP server.

mod support;

use support::{Server, component_of, listing, write_package};
use waos_package::{Index, Manifest, SigningKey};
use waos_store::{
    DevelopmentBuild, InstallError, Source, SourceError, SourceProxy, Store, StoreError,
};

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
        ..Index::default()
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
async fn a_sources_picture_is_read_from_its_server_and_kept_while_a_source_lists_it() {
    let scratch = tempfile::tempdir().unwrap();
    let server = Server::serve(vec![("waos/icon.png".to_owned(), b"an icon".to_vec())]).await;
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let from = source(&server.url("waos/index.json"));
    let kept = || -> Vec<Vec<u8>> {
        std::fs::read_dir(scratch.path().join("store/pictures"))
            .map(|files| {
                files
                    .map(|file| std::fs::read(file.unwrap().path()).unwrap())
                    .collect()
            })
            .unwrap_or_default()
    };
    assert!(kept().is_empty());
    assert_eq!(store.picture(&from, "icon.png").await.unwrap(), b"an icon");
    assert_eq!(kept(), [b"an icon".to_vec()], "kept as read");

    store.keep_pictures(&[from.picture_url("icon.png").unwrap()]);
    assert_eq!(kept().len(), 1, "a source lists it still");
    store.keep_pictures(&[]);
    assert!(kept().is_empty(), "no source lists it any more");
}

#[tokio::test]
async fn a_picture_on_another_server_than_its_index_is_not_read() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let from = source("https://apps.example/waos/index.json");
    let elsewhere = store
        .picture(&from, "http://192.168.1.1/admin.png")
        .await
        .unwrap_err();
    assert!(
        matches!(elsewhere, SourceError::PictureElsewhere { .. }),
        "{elsewhere}"
    );
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
    assert_eq!(installed.publisher(), Some(key.publisher()));
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
async fn an_update_from_a_source_never_touches_a_development_install() {
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open(&scratch.path().join("store"), SourceProxy::None).unwrap();
    let key = SigningKey::generate();
    let published = write_package(scratch.path(), TRIANGLE, "1.0.0", &key);
    store
        .install(waos_package::Package::open(&published).unwrap())
        .unwrap();
    let source_manifest = Manifest::new(
        TRIANGLE.parse().unwrap(),
        "Triangle",
        "0.1.0".parse().unwrap(),
    )
    .unwrap();
    store
        .install_development(&DevelopmentBuild {
            manifest: &source_manifest,
            component: b"\0asm from source",
            icon: None,
        })
        .unwrap();
    let development = "dev.example.triangle.development";
    let update = write_package(scratch.path(), TRIANGLE, "2.0.0", &key);
    let update_listed = listing(&update, TRIANGLE, "2.0.0");
    // A listing of the development id, as a source could list one: no package of it can be made,
    // so whatever it names is refused before it is downloaded.
    let posing_listed = waos_package::ListedProgram {
        id: development.parse().unwrap(),
        ..update_listed.clone()
    };
    let server = Server::serve(vec![(
        update_listed.package.clone(),
        std::fs::read(&update).unwrap(),
    )])
    .await;
    let source = source(&server.url("index.json"));

    store.install_from(&source, &update_listed).await.unwrap();
    let error = store
        .install_from(&source, &posing_listed)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            InstallError::Package(waos_package::PackageError::DevelopmentId(_))
        ),
        "{error}"
    );

    let installed: Vec<(String, String, bool)> = store
        .installed()
        .iter()
        .map(|program| {
            let manifest = &program.manifest;
            let id = manifest.id().to_string();
            (id, manifest.version().to_string(), program.is_development())
        })
        .collect();
    assert_eq!(
        installed,
        [
            (TRIANGLE.to_owned(), "2.0.0".to_owned(), false),
            (development.to_owned(), "0.1.0".to_owned(), true),
        ]
    );
    let component = store.program(&development.parse().unwrap()).unwrap();
    assert_eq!(std::fs::read(component).unwrap(), b"\0asm from source");
}
