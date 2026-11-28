//! The package manager's own rules hold over the installed apps kept in memory: it needs no disk.

use super::support::{Manager, TRIANGLE, content, sent, triangle, triangle_at};
use waos_package::SigningKey;
use waos_services::packages::{Caller, InstallError, PackageChange};

#[tokio::test]
async fn an_app_kept_in_memory_is_stopped_before_each_change() {
    let manager = Manager::in_memory();
    let key = SigningKey::generate();
    for version in ["1.0.0", "1.1.0"] {
        manager
            .install(manager.package(version, &key))
            .await
            .unwrap();
    }
    assert_eq!(manager.installed(), triangle_at("1.1.0"));
    manager.packages.remove(&triangle()).await.unwrap();
    assert!(manager.installed().is_empty());
    assert_eq!(
        *manager.stops.stops.lock().unwrap(),
        [
            (triangle(), PackageChange::Install),
            (triangle(), PackageChange::Install),
            (triangle(), PackageChange::Removal),
        ]
    );
}

#[tokio::test]
async fn an_update_kept_in_memory_is_refused_from_another_publisher() {
    let manager = Manager::in_memory();
    manager
        .install(manager.package("1.0.0", &SigningKey::generate()))
        .await
        .unwrap();
    let other = manager.package("1.1.0", &SigningKey::generate());
    let error = manager
        .packages
        .install(content(TRIANGLE, other), Caller::User)
        .finished()
        .await
        .unwrap_err();
    assert!(
        matches!(error, InstallError::DifferentPublisher { .. }),
        "{error}"
    );
    assert_eq!(manager.installed(), triangle_at("1.0.0"));
}

#[tokio::test]
async fn a_removal_of_an_app_kept_in_memory_cancels_its_update_under_way() {
    let manager = Manager::in_memory();
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
}
