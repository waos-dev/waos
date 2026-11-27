//! What `waos launch` and `waos stop` ask (the design's decision 70). A launch starts an
//! installed program, a development install or any other, in place of its run if it runs, which
//! ends `Replaced` (`InstalledRuns::launch`), with the launch's arguments and the variables it
//! alone adds (decision 68), and its output going to the attaches (`launch_outputs`). Where its
//! code is compiled first, after a compiler change, the package manager says the program is
//! `Optimizing`, and the connection hears `Compiling`. The launch is answered `Launched` once the
//! program runs, and the program runs on whoever attaches or not, until it ends or is stopped; a
//! launch whose connection closes while its code compiles, as Ctrl-C in `waos run` closes it, is
//! not started. A stop ends the program running, however it was started.
//!
//! The invariant: a launched run's end is told to its output, then the run is taken out of the
//! runs, by a task of the server's runtime (`InstalledRuns::take_out_once_ended`), so every attach
//! hears how it ended and the runs never hold an ended program.

use tracing::Instrument;
use waos_dev_protocol::{
    Launch, LaunchAnswer, Refusal, ServerConnection, ServerReadHalf, ServerWriteHalf, StopAnswer,
};
use waos_package::ProgramId;
use waos_store::Store;

use super::dev_refusal;
use super::installed_runs::InstalledRuns;
use super::package_manager::PackageOperation;
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

/// Starts the installed program `launch` names, telling `writing` `Compiling` while its code is
/// compiled first; not started where `reading` closes before it starts.
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
    if context.store.installed_program(&id).is_none() {
        return Err(dev_refusal::not_installed(&id));
    }
    let installed = &context.installed;
    let launching = installed.launch(&id, arguments, variables);
    let closing = closed(reading);
    let mut operations = installed.packages.operations();
    let mut told = false;
    tokio::pin!(launching, closing);
    loop {
        let optimizing =
            operations.borrow_and_update().get(&id) == Some(&PackageOperation::Optimizing);
        if optimizing && !told {
            told = true;
            // A client gone meanwhile hears nothing; the launch goes on.
            let _ = writing.send(&LaunchAnswer::Compiling).await;
        }
        tokio::select! {
            launched = &mut launching => {
                return launched.map_err(|error| dev_refusal::cannot_load(&id, &error));
            }
            () = &mut closing => {
                tracing::info!("the launch's connection closed before it started: not started");
                return Err(Refusal::new(
                    waos_dev_protocol::RefusalKind::Failed,
                    format!("{id}'s launch was abandoned"),
                ));
            }
            changed = operations.changed() => {
                if changed.is_err() {
                    return launching
                        .await
                        .map_err(|error| dev_refusal::cannot_load(&id, &error));
                }
            }
        }
    }
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
