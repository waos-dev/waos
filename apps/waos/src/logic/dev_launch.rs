//! What `waos launch` and `waos stop` ask. A launch starts an installed program, a development
//! install or any other, in place of its run if it runs, which ends `Replaced`: its code waited for
//! while it is compiled, as at its install, or compiled where the cache holds none current, with
//! `Compiling` on the connection first, before the runs' lock is taken, which every start,
//! screenshot, input and quit takes, so a compile holds back no other program; then loaded off this
//! thread and run with its own settings and folders, this computer's capabilities, the launch's
//! arguments and the variables it alone adds, and its output going to the attaches
//! (`launch_outputs`). The launch is answered `Launched` once the program runs, and the program
//! runs on whoever attaches or not, until it ends or is stopped; a launch whose connection closes
//! while its code compiles, as Ctrl-C in `waos run` closes it, is not started. A stop ends the
//! program running, however it was started.
//!
//! The invariant: a launched run's end is told to its output, then the run is taken out of the
//! runs, by a task of the server's runtime (`InstalledRuns::take_out_once_ended`), so every attach
//! hears how it ended and the runs never hold an ended program.

use std::sync::Arc;

use anyhow::Context as _;
use tracing::Instrument;
use waos::ProgramEnvironment;
use waos_dev_protocol::{
    Launch, LaunchAnswer, Refusal, RefusalKind, ServerConnection, ServerReadHalf, ServerWriteHalf,
    StopAnswer,
};
use waos_package::ProgramId;
use waos_store::Store;

use super::dev_refusal;
use super::installed_runs::InstalledRuns;
use super::runs::Runs;

/// What launching and stopping need of the app.
#[derive(Clone)]
pub(crate) struct LaunchContext {
    pub(crate) installed: InstalledRuns,
    pub(crate) store: Store,
}

/// Serves a connection's `Launch`: the program started, answered `Launched`, or why not, after
/// `Compiling` where its code is compiled first.
pub(crate) async fn launch(connection: ServerConnection, launch: Launch, context: &LaunchContext) {
    let id = launch.id.clone();
    let (reading, mut writing) = connection.into_split();
    let span = tracing::info_span!("run", name = id.as_str());
    let launched = start(&mut writing, reading, launch, context)
        .instrument(span)
        .await;
    let answer = match launched {
        Ok(()) => LaunchAnswer::Launched,
        Err(refusal) => {
            tracing::info!(%id, ?refusal, "a launch refused");
            LaunchAnswer::Refused(refusal)
        }
    };
    // A client gone meanwhile has nobody to tell; the program runs on.
    let _ = writing.send(&answer).await;
}

/// Starts the installed program `launch` names, its code compiled first, telling `writing`
/// `Compiling` where it is; not started where `reading` closes while it compiles.
async fn start(
    writing: &mut ServerWriteHalf,
    reading: ServerReadHalf,
    launch: Launch,
    context: &LaunchContext,
) -> Result<(), Refusal> {
    let Launch {
        id,
        arguments,
        variables,
    } = launch;
    let component = context
        .store
        .program(&id)
        .ok_or_else(|| dev_refusal::not_installed(&id))?;
    let installed = &context.installed;
    let environment = installed.environments.of(&id).map_err(|error| {
        Refusal::new(
            RefusalKind::Failed,
            format!("{id} could not be set up: {error}"),
        )
    })?;
    tokio::select! {
        ready = code_ready(writing, installed, &id, &component) => {
            ready.map_err(|error| dev_refusal::cannot_load(&id, &error))?;
        }
        () = closed(reading) => {
            tracing::info!("the launch's connection closed while its code compiled: not started");
            return Err(Refusal::new(RefusalKind::Failed, format!("{id}'s launch was abandoned")));
        }
    }
    let engine = Arc::clone(&installed.engine);
    let loading = tokio::task::spawn_blocking(move || engine.load(&component));
    let program = loading
        .await
        .context("the load's thread")
        .and_then(|loaded| loaded)
        .map_err(|error| dev_refusal::cannot_load(&id, &error))?
        .named(id.as_str());
    let (environment, output) = installed.with_output(&id, environment);
    let environment = ProgramEnvironment {
        variables,
        arguments,
        ..environment
    };
    let run = installed
        .runs
        .start_in_place(&id, || installed.engine.run(&program, environment))
        .await
        .map_err(|error| dev_refusal::cannot_load(&id, &error))?;
    tracing::info!("launched");
    installed.take_out_once_ended(&id, run, output);
    Ok(())
}

/// Resolves once the code of the program `id`, whose component is at `component`, is in the
/// cache, telling `connection` `Compiling` first where it is not.
async fn code_ready(
    connection: &mut ServerWriteHalf,
    installed: &InstalledRuns,
    id: &ProgramId,
    component: &std::path::Path,
) -> anyhow::Result<()> {
    let engine = Arc::clone(&installed.engine);
    let checked = component.to_owned();
    let compiled = tokio::task::spawn_blocking(move || engine.is_compiled(&checked))
        .await
        .context("the check's thread")??;
    if !compiled {
        // A client gone meanwhile hears nothing; the launch goes on.
        let _ = connection.send(&LaunchAnswer::Compiling).await;
    }
    installed.code.compile(id, component).await
}

/// Resolves once the other side closed. What it sends after its `Launch` is not part of the
/// protocol and is dropped.
async fn closed(mut reading: ServerReadHalf) {
    loop {
        match reading.receive::<waos_dev_protocol::Request>().await {
            Ok(Some(message)) => tracing::warn!(?message, "a message after the launch was asked"),
            Ok(None) | Err(_) => return,
        }
    }
}

/// Serves a connection's `Stop`: the program `id` ended, however it was started, or why not.
pub(crate) async fn stop(mut connection: ServerConnection, id: &ProgramId, runs: &Runs) {
    let answer = if runs.stop(id).await {
        StopAnswer::Stopped
    } else {
        StopAnswer::Refused(dev_refusal::not_running(id))
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}
