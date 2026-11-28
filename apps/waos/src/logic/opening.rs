//! Handing a program what its user does to it from outside it: files opened with it, as `waos open
//! <file> --program <id>` asks until programs register what opens them, and a click on one of its
//! notifications. Each goes to the app the id names (`program_named`), its instance running,
//! whether `waos launch` started it or the library opened it, or else the installed app, opened as
//! the library opens it; the program hears it once it listens
//! (`AppInstance::open_requested`, `AppInstance::notification_activated`). And opening each
//! app the user opened through the system (`open_each`), as the library opens one the user opens
//! there.
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

use super::app_instances::{AppInstances, OpenConfiguration, OpenPlace, OpenedFor};
use super::program_places::ProgramPlace;
use crate::system_shell::UserOpen;

/// Hands `request` to the app `id`: its instance running, or else the installed app, opened
/// first; fails for an id neither installed nor running.
pub(crate) async fn open_with(
    instances: &Arc<AppInstances>,
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
    instances: &Arc<AppInstances>,
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
    instances: &Arc<AppInstances>,
    packages: &PackageManager,
    id: &ProgramId,
) -> Result<Option<AppInstance>> {
    if packages.installed_program(id).is_none() {
        return Ok(instances.instance(id));
    }
    let configuration = OpenConfiguration::new(OpenedFor::HandOver);
    let opened = instances.open(id, configuration).await?;
    Ok(Some(opened.instance().clone()))
}

/// Opens each app the user opened through the system among `instances`, as the library opens one
/// the user opens there, in a window, the one running left as it is; answers each open with the
/// instance, or why there is none. Ends with `opens`.
pub(crate) async fn open_each(
    instances: Arc<AppInstances>,
    mut opens: futures::stream::BoxStream<'static, UserOpen>,
) {
    use futures::StreamExt;
    while let Some(UserOpen { app, opened }) = opens.next().await {
        let configuration = OpenConfiguration {
            place: OpenPlace::Named(ProgramPlace::Windows),
            ..OpenConfiguration::new(OpenedFor::User)
        };
        let answer = (instances.open(&app, configuration).await)
            .map(|opened| opened.instance().clone())
            .map_err(|error| format!("{error:#}"));
        let _ = opened.send(answer);
    }
}

/// Answers a connection's `Open`: the files handed to the program `id`, then `Opened`, or why
/// not.
#[cfg(desktop)]
pub(crate) async fn answer(
    mut connection: ServerConnection,
    instances: &Arc<AppInstances>,
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
