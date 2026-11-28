//! Handing a program what its user does to it from outside it: files opened with it, as `waos open
//! <file> --program <id>` asks until programs register what opens them, and a click on one of its
//! notifications. Each goes to the program the id names (`program_named`), the one running under
//! it, whether `waos launch` started it or the library opened it, or else the installed program,
//! opened as the library opens it; the program hears it once it listens
//! (`RunningProgram::open_requested`, `RunningProgram::notification_activated`). And opening each
//! app the user opened through the system (`open_each`), as the library opens one the user opens
//! there.
//!
//! The invariant: a request or a click reaches a program, or the caller hears why not; nothing is
//! started for an id that names no installed program.

use std::sync::Arc;

use anyhow::{Context, Result};
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{OpenRequest, RunningProgram};
#[cfg(desktop)]
use waos_dev_protocol::{OpenAnswer, Refusal, RefusalKind, ServerConnection};
use waos_package::ProgramId;
use waos_services::packages::PackageManager;

#[cfg(target_os = "macos")]
use super::app_starter::StartPlace;
use super::app_starter::{AppStarter, Opening, StartRequest};
#[cfg(target_os = "macos")]
use super::program_places::ProgramPlace;
use super::running_apps::RunningApps;
#[cfg(target_os = "macos")]
use crate::system_shell::UserOpen;

/// Hands `request` to the program `id`: the one running under it, or else the installed one,
/// opened first; fails for an id neither installed nor running.
pub(crate) async fn open_with(
    starter: &Arc<AppStarter>,
    running: &RunningApps,
    packages: &PackageManager,
    id: &ProgramId,
    request: OpenRequest,
) -> Result<()> {
    let program = program_named(starter, running, packages, id)
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
    starter: &Arc<AppStarter>,
    running: &RunningApps,
    packages: &PackageManager,
    id: &ProgramId,
    activation: Activation,
) -> Result<()> {
    let program = program_named(starter, running, packages, id)
        .await?
        .with_context(|| {
            format!("{id} is no longer installed, so its notification has nothing to open")
        })?;
    tracing::info!(%id, ?activation, "a click on the program's notification");
    program.notification_activated(activation);
    Ok(())
}

/// The program `id` names: the installed one, opened unless it runs already, or else the one
/// running under it, as `waos` runs one; none for an id neither installed nor running.
pub(crate) async fn program_named(
    starter: &Arc<AppStarter>,
    running: &RunningApps,
    packages: &PackageManager,
    id: &ProgramId,
) -> Result<Option<RunningProgram>> {
    if packages.installed_program(id).is_none() {
        return Ok(running.running(id));
    }
    let opened = starter
        .start(StartRequest::open(id.clone(), Opening::ToHandOver))
        .await?;
    Ok(Some(opened.program().clone()))
}

/// Opens each app the user opened through the system with `starter`, as the library opens one the
/// user opens there, in a window, the one running left as it is; answers each open with the run,
/// or why there is none. Ends with `opens`.
#[cfg(target_os = "macos")]
pub(crate) async fn open_each(
    starter: Arc<AppStarter>,
    mut opens: futures::stream::BoxStream<'static, UserOpen>,
) {
    use futures::StreamExt;
    while let Some(UserOpen { app, opened }) = opens.next().await {
        let request = StartRequest {
            place: StartPlace::Named(ProgramPlace::Windows),
            ..StartRequest::open(app, Opening::ByTheUser)
        };
        let answer = (starter.start(request).await)
            .map(|answer| answer.program().clone())
            .map_err(|error| format!("{error:#}"));
        let _ = opened.send(answer);
    }
}

/// Answers a connection's `Open`: the files handed to the program `id`, then `Opened`, or why
/// not.
#[cfg(desktop)]
pub(crate) async fn answer(
    mut connection: ServerConnection,
    starter: &Arc<AppStarter>,
    running: &RunningApps,
    packages: &PackageManager,
    id: &ProgramId,
    request: OpenRequest,
) {
    let answer = match open_with(starter, running, packages, id, request).await {
        Ok(()) => OpenAnswer::Opened,
        Err(why) => {
            tracing::info!("an opening refused: {why:#}");
            OpenAnswer::Refused(Refusal::new(RefusalKind::Failed, format!("{why:#}")))
        }
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}
