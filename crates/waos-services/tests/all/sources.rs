//! The sources the user added, reading a source's index, and installing what it lists, against
//! a local HTTP server.

use std::sync::Arc;

use crate::support::{
    Server, component_of, install_from, listing, packages, sources, write_package,
};
use waos_package::{Index, SigningKey};
use waos_services::packages::InstallError;
use waos_services::sources::{
    Source, SourceError, SourceListError, SourceProxy, Sources, SourcesFile,
};

const TRIANGLE: &str = "dev.example.triangle";

fn source(text: &str) -> Source {
    text.parse().unwrap()
}

/// The sources in WaOS's file in `directory`.
fn sources_in_file(directory: &std::path::Path) -> Sources {
    let file = Arc::new(SourcesFile::new(directory));
    Sources::open(file, &directory.join("pictures"), SourceProxy::None).unwrap()
}

#[test]
fn sources_are_kept_in_the_order_added() {
    let scratch = tempfile::tempdir().unwrap();
    let directory = scratch.path().join("store");
    let sources = sources_in_file(&directory);
    let github = source("github.com/example/triangle");
    let index = source("example.com/apps");
    sources.add(github.clone()).unwrap();
    sources.add(index.clone()).unwrap();
    sources.add(github.clone()).unwrap();
    assert_eq!(sources.list(), [github.clone(), index.clone()]);
    assert_eq!(
        sources_in_file(&directory).list(),
        [github.clone(), index.clone()]
    );
    sources.remove(&github).unwrap();
    assert_eq!(sources.list(), [index]);
    assert!(matches!(
        sources.remove(&github).unwrap_err(),
        SourceListError::NotAdded(_)
    ));
}

#[test]
fn the_first_sources_are_made_once_and_a_removed_one_never_comes_back() {
    let scratch = tempfile::tempdir().unwrap();
    let sources = sources(scratch.path());
    let first = [source("list.waos.app")];
    sources.start_with(&first).unwrap();
    assert_eq!(sources.list(), first);
    sources.remove(&first[0]).unwrap();
    sources.start_with(&first).unwrap();
    assert!(sources.list().is_empty());
}

#[test]
fn the_first_sources_leave_a_list_added_to_already_as_it_is() {
    let scratch = tempfile::tempdir().unwrap();
    let sources = sources(scratch.path());
    let github = source("github.com/example/triangle");
    sources.add(github.clone()).unwrap();
    sources.start_with(&[source("list.waos.app")]).unwrap();
    assert_eq!(sources.list(), [github]);
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
    let sources = sources(&scratch.path().join("store/pictures"));
    let read = sources
        .read_index(&source(&server.url("waos")))
        .await
        .unwrap();
    assert_eq!(read, index);
}

#[tokio::test]
async fn a_sources_picture_is_read_from_its_server_and_kept_while_a_source_lists_it() {
    let scratch = tempfile::tempdir().unwrap();
    let server = Server::serve(vec![("waos/icon.png".to_owned(), b"an icon".to_vec())]).await;
    let sources = sources(&scratch.path().join("store/pictures"));
    let from = source(&server.url("waos"));
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
    assert_eq!(
        sources.picture(&from, "icon.png").await.unwrap(),
        b"an icon"
    );
    assert_eq!(kept(), [b"an icon".to_vec()], "kept as read");

    sources.keep_pictures(&[from.picture_url("icon.png").unwrap()]);
    assert_eq!(kept().len(), 1, "a source lists it still");
    sources.keep_pictures(&[]);
    assert!(kept().is_empty(), "no source lists it any more");
}

#[tokio::test]
async fn a_picture_on_another_server_than_its_index_is_not_read() {
    let scratch = tempfile::tempdir().unwrap();
    let sources = sources(&scratch.path().join("store/pictures"));
    let from = source("apps.example/waos");
    let elsewhere = sources
        .picture(&from, "http://192.168.1.1/admin.png")
        .await
        .unwrap_err();
    assert!(
        matches!(elsewhere, SourceError::PictureElsewhere { .. }),
        "{elsewhere}"
    );
}

#[tokio::test]
async fn an_address_with_no_index_is_told_it_lists_no_apps() {
    let scratch = tempfile::tempdir().unwrap();
    let server = Server::serve(vec![("apps/index.json".to_owned(), b"<html>".to_vec())]).await;
    let sources = sources(&scratch.path().join("store/pictures"));
    let missing = sources
        .read_index(&source(&server.url("missing")))
        .await
        .unwrap_err();
    assert!(
        matches!(missing, SourceError::ListsNothing { .. }),
        "{missing}"
    );
    let site = sources
        .read_index(&source(&server.url("")))
        .await
        .unwrap_err();
    let looked_at = server.url(".well-known/waos/index.json");
    assert!(site.to_string().ends_with(&looked_at), "{site}");
    let not_an_index = sources
        .read_index(&source(&server.url("apps")))
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
    let directory = scratch.path().join("store");
    let sources = sources(&directory.join("pictures"));
    let download = sources
        .download(&source(&server.url("waos")), &listed)
        .await
        .unwrap();
    assert_eq!(
        download.size,
        Some(std::fs::metadata(&package).unwrap().len())
    );
    let packages = packages(&directory);
    let installed = install_from(&sources, &packages, &source(&server.url("waos")), &listed)
        .await
        .unwrap();
    assert_eq!(installed.publisher(), Some(key.publisher()));
    let lock = packages.lock(&TRIANGLE.parse().unwrap()).await;
    let component = lock.version().unwrap().component();
    assert_eq!(
        std::fs::read(component).unwrap(),
        component_of(TRIANGLE, "1.2.0")
    );
}

#[tokio::test]
async fn a_download_unlike_its_listing_ends_in_an_error_and_installs_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let package = write_package(scratch.path(), TRIANGLE, "1.2.0", &SigningKey::generate());
    let mut listed = listing(&package, TRIANGLE, "1.2.0");
    listed.sha256[0] ^= 1;
    let server = Server::serve(vec![(
        format!("apps/{}", listed.package),
        std::fs::read(&package).unwrap(),
    )])
    .await;
    let directory = scratch.path().join("store");
    let sources = sources(&directory.join("pictures"));
    let packages = packages(&directory);
    let error = install_from(&sources, &packages, &source(&server.url("apps")), &listed)
        .await
        .unwrap_err();
    assert!(matches!(error, InstallError::Receiving(_)), "{error}");
    assert!(error.to_string().contains("SHA-256"), "{error}");
    assert!(packages.installed().is_empty());
}

#[tokio::test]
async fn a_listing_of_a_development_id_is_refused_before_its_download() {
    let scratch = tempfile::tempdir().unwrap();
    let package = write_package(scratch.path(), TRIANGLE, "1.0.0", &SigningKey::generate());
    let posing = waos_package::ListedProgram {
        id: "dev.example.triangle.development".parse().unwrap(),
        ..listing(&package, TRIANGLE, "1.0.0")
    };
    let sources = sources(&scratch.path().join("store/pictures"));
    // Nothing listens there: a download would fail otherwise.
    let nowhere = source("http://127.0.0.1:9/apps");
    let refused = sources.download(&nowhere, &posing).await;
    assert!(
        matches!(refused, Err(SourceError::DevelopmentId(_))),
        "{:?}",
        refused.err()
    );
}
