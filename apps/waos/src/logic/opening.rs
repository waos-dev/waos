//! Handing a program what its user does to it from outside it: files opened with it, as `waos open
//! <file> --program <id>` asks until programs register what opens them, and a click on one of its
//! notifications. Each goes to the app the id names (`program_named`), its instance running,
//! whether `waos launch` started it or the library opened it, or else the installed app, opened by
//! app management (`app_management`); the program hears it once it listens
//! (`AppInstance::open_requested`, `AppInstance::notification_activated`), as a shell hands an
//! app it opened what it was opened for (`hand_over`).
//!
//! The invariant: a request or a click reaches a program, or the caller hears why not; nothing is
//! started for an id that names no installed program.

use std::sync::Arc;

use anyhow::{Context, Result};
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{AppInstance, OpenRequest};
#[cfg(desktop)]
use waos_dev_protocol::{OpenAnswer, Refusal, RefusalKind, ServerConnection};
use waos_package::ProgramId;
use waos_services::packages::PackageManager;

use super::app_instances::{OpenConfiguration, OpenedFor};
use super::app_management::AppManagement;

/// Hands `request` to the app `id`: its instance running, or else the installed app, opened
/// first; fails for an id neither installed nor running.
pub(crate) async fn open_with(
    instances: &Arc<dyn AppManagement>,
    packages: &PackageManager,
    id: &ProgramId,
    request: OpenRequest,
) -> Result<()> {
    let program = program_named(instances, packages, id)
        .await?
        .with_context(|| format!("{id} is neither installed nor running"))?;
    tracing::info!(%id, ?request, "files opened with the program");
    program.open_requested(request);
    Ok(())
}

/// Hands the click `activation` on a notification to the program `id`, as files are handed: the one
/// running under it, or else the installed one, opened first; fails, saying so, for an id neither
/// installed nor running, as a removed program leaves its notifications as they are.
pub(crate) async fn notification_activated(
    instances: &Arc<dyn AppManagement>,
    packages: &PackageManager,
    id: &ProgramId,
    activation: Activation,
) -> Result<()> {
    let program = program_named(instances, packages, id)
        .await?
        .with_context(|| {
            format!("{id} is no longer installed, so its notification has nothing to open")
        })?;
    tracing::info!(%id, ?activation, "a click on the program's notification");
    program.notification_activated(activation);
    Ok(())
}

/// The instance of the app `id` names: the installed app's, opened unless it runs already, or else
/// the one running under it, as `waos` runs one; none for an id neither installed nor running.
pub(crate) async fn program_named(
    instances: &Arc<dyn AppManagement>,
    packages: &PackageManager,
    id: &ProgramId,
) -> Result<Option<AppInstance>> {
    if packages.installed_program(id).is_none() {
        return Ok(instances.running().borrow().get(id).cloned());
    }
    let configuration = OpenConfiguration::new(OpenedFor::HandOver);
    let opened = Arc::clone(instances)
        .open(id.clone(), configuration)
        .await?;
    Ok(Some(opened.instance().clone()))
}

/// Hands `instance` what the user opened its app for: the files and links the system opened it
/// with, and the click on one of its notifications, each heard once the app listens.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "only macOS's shell opens an app with what the system handed it"
    )
)]
pub(crate) fn hand_over(
    instance: &AppInstance,
    files: OpenRequest,
    notification_click: Option<Activation>,
) {
    if !(files.files.is_empty() && files.links.is_empty()) {
        instance.open_requested(files);
    }
    if let Some(click) = notification_click {
        instance.notification_activated(click);
    }
}

/// Answers a connection's `Open`: the files handed to the program `id`, then `Opened`, or why
/// not.
#[cfg(desktop)]
pub(crate) async fn answer(
    mut connection: ServerConnection,
    instances: &Arc<dyn AppManagement>,
    packages: &PackageManager,
    id: &ProgramId,
    request: OpenRequest,
) {
    let answer = match open_with(instances, packages, id, request).await {
        Ok(()) => OpenAnswer::Opened,
        Err(why) => {
            tracing::info!("an opening refused: {why:#}");
            OpenAnswer::Refused(Refusal::new(RefusalKind::Failed, format!("{why:#}")))
        }
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}
