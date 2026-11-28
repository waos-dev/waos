//! What a caller of the package manager relies on, one test where a change could break it, each
//! over the real manager in a throwaway folder.

use std::fs;
use std::time::Duration;

use super::support::{
    MARKER, Manager, TRIANGLE, UNCOMPILABLE, build, component_of, content, sent, triangle,
    triangle_at,
};
use futures::StreamExt as _;
use futures::stream;
use sha2::{Digest, Sha256};
use waos_package::SigningKey;
use waos_services::packages::{
    Caller, InstallError, InstallProgress, PackageChange, PackageContent, RemoveError,
};

/// How long a test waits for what must not happen before it says it did not.
const A_MOMENT: Duration = Duration::from_millis(200);

// 1. An app is always one whole version.

#[tokio::test]
async fn a_package_that_cannot_be_opened_leaves_the_version_installed() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    manager
        .install(manager.package("1.0.0", &key))
        .await
        .unwrap();
    let mut spoiled = manager.package("1.1.0", &key);
    let middle = spoiled.len() / 2;
    spoiled[middle] ^= 0xff;
    let error = manager.install(spoiled).await.unwrap_err();
    assert!(matches!(error, InstallError::Package(_)), "{error}");
    assert_eq!(manager.installed(), triangle_at("1.0.0"));
    let entries = fs::read_dir(manager.program_folder(&triangle())).unwrap();
    assert_eq!(
        entries.count(),
        2,
        "current and its version, nothing the install left"
    );
}

// 2. What is checked is what is installed.

#[tokio::test]
async fn the_bytes_installed_are_the_ones_received_whatever_their_source_does_after() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    let path = super::support::write_package(manager.scratch.path(), TRIANGLE, "1.0.0", &key);
    let bytes = fs::read(&path).unwrap();
    let source = path.clone();
    // The last thing the source does, once its bytes are sent, is change its file.
    let changed_after = stream::once(async move {
        fs::write(&source, b"changed once sent").unwrap();
        Ok(Vec::new())
    });
    let content = PackageContent::Package {
        app: triangle(),
        bytes: stream::iter([Ok(bytes)]).chain(changed_after).boxed(),
        size: None,
    };
    manager
        .packages
        .install(content, Caller::User)
        .finished()
        .await
        .unwrap();
    let lock = manager.packages.lock(&triangle()).await;
    let component = fs::read(lock.version().unwrap().component()).unwrap();
    assert_eq!(component, component_of(TRIANGLE, "1.0.0"));
}

#[tokio::test]
async fn a_package_of_another_app_than_asked_is_refused() {
    let manager = Manager::new();
    let bytes = manager.package("1.0.0", &SigningKey::generate());
    let asked = manager
        .packages
        .install(content("dev.example.other", bytes), Caller::User)
        .finished()
        .await;
    assert!(
        matches!(asked, Err(InstallError::OtherApp { .. })),
        "{asked:?}"
    );
    assert!(manager.installed().is_empty());
}

// 3. An update comes from the same publisher, at a newer version.

#[tokio::test]
async fn an_update_signed_with_another_key_is_refused_naming_both() {
    let manager = Manager::new();
    let (first, second) = (SigningKey::generate(), SigningKey::generate());
    manager
        .install(manager.package("1.0.0", &first))
        .await
        .unwrap();
    let error = manager
        .install(manager.package("2.0.0", &second))
        .await
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
    assert_eq!(manager.installed(), triangle_at("1.0.0"));
}

#[tokio::test]
async fn the_same_key_updates_only_to_a_newer_version() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    manager
        .install(manager.package("1.1.0", &key))
        .await
        .unwrap();
    for version in ["1.1.0", "1.0.9", "1.1.0-beta.1"] {
        let error = manager
            .install(manager.package(version, &key))
            .await
            .unwrap_err();
        assert!(matches!(error, InstallError::NotNewer { .. }), "{error}");
    }
    manager
        .install(manager.package("1.2.0", &key))
        .await
        .unwrap();
    assert_eq!(manager.installed(), triangle_at("1.2.0"));
}

// 4. Installed means ready to open.

#[tokio::test]
async fn an_app_is_installed_with_its_code_compiled_and_one_that_does_not_compile_is_refused() {
    let manager = Manager::new();
    let installed = manager
        .install(manager.package("1.0.0", &SigningKey::generate()))
        .await
        .unwrap();
    assert_eq!(installed.manifest.version().to_string(), "1.0.0");
    let version = manager.packages.lock(&triangle()).await.version().unwrap();
    assert!(version.code().join(MARKER).is_file());

    let refused = manager
        .packages
        .install(
            PackageContent::Development(build("0.1.0", UNCOMPILABLE)),
            Caller::DeveloperTools,
        )
        .finished()
        .await;
    assert!(
        matches!(refused, Err(InstallError::Compile(_))),
        "{refused:?}"
    );
    assert_eq!(manager.installed(), triangle_at("1.0.0"));
}

// 5. Compiled code is a cache.

#[tokio::test]
async fn code_that_cannot_be_used_or_does_not_open_is_compiled_again_at_an_open() {
    let manager = Manager::new();
    manager
        .install(manager.package("1.0.0", &SigningKey::generate()))
        .await
        .unwrap();
    let lock = manager.packages.lock(&triangle()).await;
    let code = lock.version().unwrap().code();
    fs::remove_file(code.join(MARKER)).unwrap();
    let (told, compiling) = std::sync::mpsc::channel();
    let opened = lock
        .open(
            move || told.send(()).unwrap(),
            |version| Ok(version.clone()),
        )
        .await
        .unwrap();
    compiling
        .try_recv()
        .expect("the caller is told as it compiles");
    assert!(opened.code().join(MARKER).is_file());

    // Code there that does not open is compiled again, once.
    let compiles = *manager.compiler.compiles.lock().unwrap();
    let opens = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&opens);
    lock.open(
        || {},
        move |_| match counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
            0 => anyhow::bail!("stale code"),
            _ => Ok(()),
        },
    )
    .await
    .unwrap();
    assert_eq!(*manager.compiler.compiles.lock().unwrap(), compiles + 1);
}

// 6. An app is stopped before it changes, and does not start while it changes.

#[tokio::test]
async fn an_app_is_stopped_before_it_changes_and_an_install_waits_for_its_lock() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    manager
        .install(manager.package("1.0.0", &key))
        .await
        .unwrap();
    let lock = manager.packages.lock(&triangle()).await;
    let update = manager.packages.install(
        content(TRIANGLE, manager.package("1.1.0", &key)),
        Caller::User,
    );
    let mut finished = Box::pin(update.finished());
    assert!(
        tokio::time::timeout(A_MOMENT, &mut finished).await.is_err(),
        "no change while a start holds the app's lock"
    );
    assert_eq!(manager.installed(), triangle_at("1.0.0"));
    drop(lock);
    finished.await.unwrap();
    manager.packages.remove(&triangle()).await.unwrap();
    assert_eq!(
        *manager.stops.stops.lock().unwrap(),
        [
            (triangle(), PackageChange::Install),
            (triangle(), PackageChange::Install),
            (triangle(), PackageChange::Removal),
        ]
    );
}

// 7. Removing takes everything that is the app's.

#[tokio::test]
async fn a_removal_takes_the_apps_folder_whole_and_says_so() {
    let manager = Manager::new();
    manager
        .install(manager.package("1.0.0", &SigningKey::generate()))
        .await
        .unwrap();
    let data = manager.program_folder(&triangle()).join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("notes.txt"), "the app's").unwrap();
    let mut removals = manager.packages.removals();
    let removal = manager.packages.remove(&triangle());
    assert!(
        manager.packages.removing().borrow().contains(&triangle()),
        "being removed from the call"
    );
    removal.await.unwrap();
    assert!(manager.packages.removing().borrow().is_empty());
    assert!(!manager.program_folder(&triangle()).exists());
    assert_eq!(removals.recv().await, Some(triangle()));
    let again = manager.packages.remove(&triangle()).await;
    assert!(
        matches!(again, Err(RemoveError::NotInstalled(_))),
        "{again:?}"
    );
    // Another publisher may take the id once it is gone.
    manager
        .install(manager.package("0.1.0", &SigningKey::generate()))
        .await
        .unwrap();
    assert_eq!(manager.installed(), triangle_at("0.1.0"));
}

#[tokio::test]
async fn an_update_keeps_the_apps_own_data() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    manager
        .install(manager.package("1.0.0", &key))
        .await
        .unwrap();
    let data = manager.program_folder(&triangle()).join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("notes.txt"), "the app's").unwrap();

    manager
        .install(manager.package("1.1.0", &key))
        .await
        .unwrap();

    assert_eq!(manager.installed(), triangle_at("1.1.0"));
    assert_eq!(
        fs::read_to_string(data.join("notes.txt")).unwrap(),
        "the app's"
    );
}

// 8. Every change is announced; 9. who asked is recorded.

#[tokio::test]
async fn a_session_is_told_from_its_first_byte_to_its_end_with_who_asked() {
    let manager = Manager::new();
    let bytes = manager.package("1.0.0", &SigningKey::generate());
    let mut changes = manager.packages.installed_changes();
    changes.borrow_and_update();
    let (content, sender) = sent(Some(bytes.len() as u64));
    let session = manager.packages.install(content, Caller::User);
    let listed = manager.packages.sessions().borrow().clone();
    assert_eq!(listed.len(), 1, "listed as it is asked for");
    assert_eq!(
        (listed[0].app.clone(), listed[0].caller),
        (triangle(), Caller::User)
    );
    let mut progress = session.progress();
    sender.unbounded_send(Ok(bytes.clone())).unwrap();
    drop(sender);
    let total = Some(bytes.len() as u64);
    let received = InstallProgress::Receiving {
        received: bytes.len() as u64,
        total,
    };
    progress
        .wait_for(|now| *now == received || *now == InstallProgress::Installing)
        .await
        .unwrap();
    session.finished().await.unwrap();
    assert!(manager.packages.sessions().borrow().is_empty());
    assert!(changes.has_changed().unwrap(), "the install is announced");
}

#[tokio::test]
async fn a_session_asked_before_its_bytes_hears_its_size_later_and_ends_with_their_error() {
    let manager = Manager::new();
    let (content, sender) = sent(None);
    let session = manager.packages.install(content, Caller::User);
    let mut progress = session.progress();
    let nothing_yet = InstallProgress::Receiving {
        received: 0,
        total: None,
    };
    assert_eq!(*progress.borrow(), nothing_yet);
    session.set_size(1000);
    let sized = InstallProgress::Receiving {
        received: 0,
        total: Some(1000),
    };
    progress.wait_for(|now| *now == sized).await.unwrap();
    let listed = manager.packages.sessions().borrow()[0].progress;
    assert_eq!(listed, sized, "the list hears it too");
    sender
        .unbounded_send(Err(std::io::Error::other("the server is gone")))
        .unwrap();
    let error = session.finished().await.unwrap_err();
    assert!(matches!(error, InstallError::Receiving(_)), "{error}");
    assert!(error.to_string().contains("the server is gone"), "{error}");
    assert!(manager.packages.sessions().borrow().is_empty());
}

// The overlaps.

#[tokio::test]
async fn sessions_of_different_callers_go_on_and_the_update_check_decides_at_commit() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    let bytes = manager.package("1.0.0", &key);
    let by_the_user = manager
        .packages
        .install(content(TRIANGLE, bytes.clone()), Caller::User);
    let by_the_tools = manager
        .packages
        .install(content(TRIANGLE, bytes), Caller::DeveloperTools);
    let (first, second) = tokio::join!(by_the_user.finished(), by_the_tools.finished());
    let refused = [&first, &second]
        .iter()
        .filter(|answer| matches!(answer, Err(InstallError::NotNewer { .. })))
        .count();
    assert_eq!(refused, 1, "{first:?} {second:?}");
    assert_eq!(manager.installed(), triangle_at("1.0.0"));
}

#[tokio::test]
async fn a_callers_newer_session_for_an_app_cancels_its_older_one() {
    let manager = Manager::new();
    let (older, _sending) = sent(None);
    let older = manager.packages.install(older, Caller::User);
    let newer = manager.packages.install(
        content(TRIANGLE, manager.package("1.0.0", &SigningKey::generate())),
        Caller::User,
    );
    assert!(matches!(
        older.finished().await,
        Err(InstallError::Cancelled)
    ));
    newer.finished().await.unwrap();
    assert_eq!(manager.installed(), triangle_at("1.0.0"));
}

#[tokio::test]
async fn a_removal_cancels_the_apps_sessions_so_it_stays_removed() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    manager
        .install(manager.package("1.0.0", &key))
        .await
        .unwrap();
    let (update, sending) = sent(None);
    let update = manager.packages.install(update, Caller::User);
    sending
        .unbounded_send(Ok(manager.package("1.1.0", &key)))
        .unwrap();
    manager.packages.remove(&triangle()).await.unwrap();
    drop(sending);
    assert!(matches!(
        update.finished().await,
        Err(InstallError::Cancelled)
    ));
    assert!(manager.installed().is_empty());
    assert!(!manager.program_folder(&triangle()).exists());
}

#[tokio::test]
async fn what_a_session_left_at_a_restart_is_cleared_when_the_manager_opens() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    manager
        .install(manager.package("1.0.0", &key))
        .await
        .unwrap();
    let folder = manager.program_folder(&triangle());
    fs::write(folder.join(".receiving-left"), b"half a package").unwrap();
    fs::create_dir(folder.join("1.1.0-left")).unwrap();
    let reopened = manager.reopened();
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
    assert_eq!(reopened.installed().len(), 1);
}

// Development installs.

#[tokio::test]
async fn a_development_install_is_an_app_of_its_own_replaced_by_each_build() {
    let manager = Manager::new();
    let key = SigningKey::generate();
    manager
        .install(manager.package("1.0.0", &key))
        .await
        .unwrap();
    let install = |component: &'static [u8]| {
        let session = manager.packages.install(
            PackageContent::Development(build("0.2.0", component)),
            Caller::DeveloperTools,
        );
        async move { session.finished().await }
    };
    let installed = install(b"\0asm first").await.unwrap();
    let development = triangle().development();
    assert_eq!(installed.manifest.id(), &development);
    assert_eq!(installed.name(), "Triangle (Development)");
    assert_eq!(installed.publisher(), None);
    assert_eq!(manager.packages.installed().len(), 2);
    assert_eq!(
        manager.packages.icon(&development),
        None,
        "the build had no icon"
    );
    let sha256 = |component: &[u8]| -> [u8; 32] { Sha256::digest(component).into() };
    let manifest = build("0.2.0", b"").manifest;
    assert!(
        manager
            .packages
            .holds_development(&manifest, &sha256(b"\0asm first"), None)
    );
    assert!(
        !manager
            .packages
            .holds_development(&manifest, &sha256(b"\0asm second"), None)
    );

    install(b"\0asm second").await.unwrap();
    let lock = manager.packages.lock(&development).await;
    let component = fs::read(lock.version().unwrap().component()).unwrap();
    assert_eq!(component, b"\0asm second", "whatever its version");
}
