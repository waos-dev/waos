//! What `waos attach` asks (the design's decision 70): the output of any program running, however
//! it was started, `Started` first, then what it wrote with nobody attached, then what it writes
//! as it writes it and how it ended (`launch_outputs`); or how its run ended, where it ended with
//! nobody attached. While attached, the program's windows count as in sight though other windows
//! cover them (the design's decision 63), so a tool's screenshot and input meet windows that draw,
//! and what WaOS asks the program's user, a row of its settings on Ask, is heard as `Asking`.
//! Closing the connection detaches: the program runs on.
//!
//! The invariant: every frame to the connection goes through the attach's queue, in the order it
//! was pushed, so `Started` comes before the first `Output` and `Ended` after the last; and the
//! queue is closed when the connection closes, so a detached attach holds back no write.

use std::sync::Arc;

use tokio::sync::broadcast::{self, error::RecvError};
use waos::Watching;
use waos_dev_protocol::{
    Asked, Refusal, RefusalKind, Request, RunEvent, ServerConnection, ServerReadHalf,
    ServerWriteHalf,
};
use waos_package::ProgramId;

use super::asking_now::AskingNow;
use super::dev_refusal;
use super::launch_outputs::{Frames, LaunchOutput, LaunchOutputs};
use super::runs::Runs;

/// What an attach needs of the app.
pub(crate) struct AttachContext<'a> {
    pub(crate) runs: &'a Runs,
    pub(crate) outputs: &'a LaunchOutputs,
    pub(crate) asking: &'a AskingNow,
}

/// Serves a connection's `Attach` to its end: the program `id`'s output until it ends or the
/// connection closes, or why there is none.
pub(crate) async fn serve(
    mut connection: ServerConnection,
    id: &ProgramId,
    context: AttachContext<'_>,
) {
    let (output, watching) = match attached_to(id, &context) {
        Ok(attached) => attached,
        Err(refusal) => {
            // A client gone meanwhile has nobody to tell.
            let _ = connection.send(&RunEvent::Refused(refusal)).await;
            return;
        }
    };
    let told = context.asking.told();
    let frames = output.attach();
    // What was asked before the attach is still asked of the user now.
    if let Some(asked) = context.asking.of(id.as_str()) {
        frames.push(RunEvent::Asking(asked));
    }
    tracing::info!(%id, "attached");
    let (reading, writing) = connection.into_split();
    let sending = send_frames(writing, Arc::clone(&frames));
    tokio::select! {
        () = sending => {}
        () = tell_asking(told, id, &frames) => {}
        () = closed(reading) => tracing::info!(%id, "detached"),
    }
    frames.close();
    if output.has_ended() {
        context.outputs.heard(id, &output);
    }
    drop(watching);
}

/// The output of the run of `id` to attach to, and the watch that keeps its windows in sight
/// while it runs; or why there is none: it does not run, nor ended unheard.
fn attached_to(
    id: &ProgramId,
    context: &AttachContext<'_>,
) -> Result<(Arc<LaunchOutput>, Option<Watching>), Refusal> {
    let kept = context.outputs.of(id);
    match (context.runs.running_now(id), kept) {
        (Some(program), Some(output)) if !output.has_ended() => {
            // Before the queue fills, so no window counts as covered while it is attached.
            Ok((output, Some(program.watch())))
        }
        (Some(_), _) => Err(Refusal::new(
            RefusalKind::Failed,
            format!("{id} runs, but WaOS keeps no output of its run"),
        )),
        (None, Some(output)) => Ok((output, None)),
        (None, None) => Err(dev_refusal::not_running(id)),
    }
}

/// Sends every frame queued, in order, until the queue is closed and drained; a connection gone
/// ends it.
async fn send_frames(mut writing: ServerWriteHalf, frames: Arc<Frames>) {
    while let Some(frame) = frames.next().await {
        if writing.send(&frame).await.is_err() {
            break;
        }
    }
}

/// Queues what WaOS asks the program `id`'s user as each is asked, from `told`; never resolves
/// while WaOS runs.
async fn tell_asking(
    mut told: broadcast::Receiver<(String, Asked)>,
    id: &ProgramId,
    frames: &Frames,
) {
    loop {
        match told.recv().await {
            Ok((name, asked)) if name == id.as_str() => frames.push(RunEvent::Asking(asked)),
            Ok(_) => {}
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(
                    missed,
                    "the attach missed what some programs' users were asked"
                );
            }
            Err(RecvError::Closed) => std::future::pending().await,
        }
    }
}

/// Resolves once the other side closed. What it sends after its `Attach` is not part of the
/// protocol and is dropped.
async fn closed(mut reading: ServerReadHalf) {
    loop {
        match reading.receive::<Request>().await {
            Ok(Some(message)) => tracing::warn!(?message, "a message after the attach was asked"),
            Ok(None) | Err(_) => return,
        }
    }
}
