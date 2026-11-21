//! One connection from `waos`, which is one run (the design's decision 29), once its `Run`
//! is read: the component's parts, which WaOS answers `Upload::Have` for when it holds that
//! component already and `Upload::Send` when it must receive them, then the component kept in a
//! file of the app's own (`sent_components`) and loaded off this thread, which compiles it unless
//! the code cache holds its code, with `Compiling` on the connection first, before the run takes
//! the runs' lock, which every start, screenshot, input and quit takes, so a compile holds back
//! no other program; the program run as the one its id names, with that program's settings and
//! folders, installed or not (decision 11 of the permissions proposal), its arguments and the
//! run's variables, with this computer's capabilities and its output as `Output` frames on the
//! connection, `Started`, then `Ended` with how it ended; a component that cannot load or run is
//! `Refused` with why, and the connection closing stops the program. The run is watched while
//! its connection is open (the design's decision 63): from its start, so its windows count as in
//! sight though other windows cover them, and a tool's screenshot and input meet windows that
//! draw, until the connection closes or the program ends. What WaOS asks the program's user
//! meanwhile, a row of its settings on Ask, the connection hears as `Asking` as it is asked
//! (`asking_now`).
//!
//! The invariants: every frame to the connection goes through one queue (`frames`), in the order
//! it was pushed, so `Started` comes before the first `Output` and `Ended` after the last, and
//! `frames` is closed when the run stops or ends.

mod frames;

use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::Instant;

use super::asking_now::AskingNow;
use super::dev_compiles::DevCompiles;
use super::dev_refusal;
use super::environments::ProgramEnvironments;
use super::runs::{Runs, SentRun};
use super::sent_components::{SentComponent, SentComponents};
use anyhow::{Context, Result};
use frames::Frames;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::sync::broadcast::{self, error::RecvError};
use tracing::Instrument;
use waos::{OutputStream, Program, ProgramEnvironment, ProgramOutput, WaOS, Watching};
use waos_dev_protocol::{
    Connection, OutputStream as ProtocolStream, Refusal, RefusalKind, Run, RunEvent, ServerStream,
    Upload,
};
use waos_package::ProgramId;

/// What a connection's run needs of the app.
pub(crate) struct RunContext {
    pub(crate) engine: Arc<WaOS>,
    /// What each program runs with: the one its id names.
    pub(crate) environments: Arc<ProgramEnvironments>,
    pub(crate) runs: Arc<Runs>,
    /// Where the components sent are kept.
    pub(crate) components: Arc<SentComponents>,
    /// Which programs' code is being compiled, for the activity that keeps macOS from throttling
    /// WaOS; none where nothing follows them.
    pub(crate) dev_compiles: Option<Arc<DevCompiles>>,
    /// What each program's user is asked now.
    pub(crate) asking: Arc<AskingNow>,
}

/// Serves a connection's `Run` to its end: the upload WaOS answers, the component's parts, then
/// the run of it as the program it names.
pub(crate) async fn serve(mut connection: Connection<ServerStream>, run: Run, context: RunContext) {
    let environment = match context.environments.of(&run.id) {
        Ok(environment) => environment,
        Err(why) => {
            let refusal = Refusal::new(
                RefusalKind::Failed,
                format!("{} could not be set up: {why:#}", run.id),
            );
            let _ = connection.send(&Upload::Refused(refusal)).await;
            return;
        }
    };
    // The id's file is held from before WaOS answers `Have`, so no later run of the id changes it
    // between the answer and the load.
    let file = context.components.hold(&run.id).await;
    let component = if file.holds_digest(run.length, &run.digest).await {
        if connection.send(&Upload::Have).await.is_err() {
            return;
        }
        None
    } else {
        if connection.send(&Upload::Send).await.is_err() {
            return;
        }
        match connection.receive_component(run.length, &run.digest).await {
            Ok(component) => Some(component),
            // A wrong upload was whole frames, so the connection still speaks: the run is
            // refused on it, as a component that cannot load is.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                tracing::warn!(%error, "a run's component did not arrive as announced");
                let refusal = Refusal::new(RefusalKind::CannotLoad, error.to_string());
                let _ = connection.send(&RunEvent::Refused(refusal)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "a run's component did not arrive");
                return;
            }
        }
    };
    let (reading, writing) = connection.into_split();
    let frames = Frames::new();
    let sending = tokio::spawn(send_frames(writing, Arc::clone(&frames)));
    let span = tracing::info_span!("run", name = run.id.as_str());
    let closed_by_peer = serve_run(
        run,
        component,
        environment,
        file,
        reading,
        &context,
        &frames,
    )
    .instrument(span)
    .await;
    frames.close();
    if closed_by_peer {
        sending.abort();
    } else {
        let _ = sending.await;
    }
}

/// Sends every frame queued, in order, until the queue is closed; a connection gone ends it.
async fn send_frames(mut writing: Connection<WriteHalf<ServerStream>>, frames: Arc<Frames>) {
    while let Some(frame) = frames.next().await {
        if writing.send(&frame).await.is_err() {
            break;
        }
    }
    frames.close();
}

/// Runs the component `component` — or the one the held file has, when WaOS answered `Have` — as
/// the program `run` names until it ends or the connection closes, queueing what the connection
/// hears of it, with `run`'s arguments and variables. Returns `true` when the connection closed.
async fn serve_run(
    run: Run,
    component: Option<Vec<u8>>,
    environment: ProgramEnvironment,
    file: SentComponent,
    reading: Connection<ReadHalf<ServerStream>>,
    context: &RunContext,
    frames: &Arc<Frames>,
) -> bool {
    let Run {
        id,
        args,
        variables,
        ..
    } = run;
    let told = context.asking.told();
    let (sent, watching) = match start(
        &id,
        component,
        environment,
        file,
        args,
        variables,
        context,
        frames,
    )
    .await
    {
        Ok(started) => started,
        Err(why) => {
            tracing::info!("refused: {why:#}");
            frames.push(RunEvent::Refused(dev_refusal::cannot_load(&id, &why)));
            return false;
        }
    };
    tracing::info!("started");
    let started_at = Instant::now();
    let (end, closed_by_peer) = tokio::select! {
        end = sent.run.program.ended() => (Some(end), false),
        () = tell_asking(told, &id, frames) => (None, false),
        () = closed(reading) => {
            tracing::info!("the connection closed: stopping the program");
            frames.close();
            drop(watching);
            sent.run.program.stop();
            sent.run.program.ended().await;
            (None, true)
        }
    };
    let stats = sent.run.program.stats();
    tracing::info!(
        ?end,
        seconds = started_at.elapsed().as_secs_f64(),
        frames_presented = stats.frames_presented(),
        "ended"
    );
    if let Some(end) = end {
        frames.push(RunEvent::Ended(sent.end(&end)));
    }
    context.runs.ended(&sent.run).await;
    closed_by_peer
}

/// Starts the component sent as the program `id`, watched: loaded first, which compiles it,
/// while no other program waits for it, then started among the runs in place of the program
/// running under the id, with its `args` and the run's `variables`. The id's file is held by the
/// caller until the run is among the runs, so the runs of one id start in the order they came.
#[expect(clippy::too_many_arguments)]
async fn start(
    id: &ProgramId,
    component: Option<Vec<u8>>,
    environment: ProgramEnvironment,
    file: SentComponent,
    args: Vec<String>,
    variables: Vec<(String, String)>,
    context: &RunContext,
    frames: &Arc<Frames>,
) -> Result<(SentRun, Watching)> {
    let (program, _file) = load(id, component, file, context, frames).await?;
    let mut watching = None;
    let sent = context
        .runs
        .start(id, || {
            // Queued before the program can write anything, so it comes first.
            frames.push(RunEvent::Started);
            let environment = ProgramEnvironment {
                output: Some(Arc::new(OutputToConnection {
                    frames: Arc::clone(frames),
                })),
                variables,
                arguments: args,
                ..environment
            };
            let (running, _views) = context.engine.run(&program, environment, Vec::new())?;
            // Before the shell can place a view, so none counts as covered while the run is open.
            watching = Some(running.watch());
            Ok(running)
        })
        .await?;
    let watching = watching.context("a run that started is watched")?;
    Ok((sent, watching))
}

/// The program `id`, named by it: `component` written to `file`, the id's, where one was sent, and
/// loaded from there, off this thread, since a load compiles and blocks while it does. The
/// connection hears `Compiling` before a load that compiles. Answers the file with the program,
/// still held.
async fn load(
    id: &ProgramId,
    component: Option<Vec<u8>>,
    file: SentComponent,
    context: &RunContext,
    frames: &Arc<Frames>,
) -> Result<(Program, SentComponent)> {
    let engine = Arc::clone(&context.engine);
    let frames = Arc::clone(frames);
    let dev_compiles = context.dev_compiles.clone();
    let compiling_id = id.clone();
    let (program, file) = tokio::task::spawn_blocking(move || {
        // The file holds the component already when the run answered `Have`.
        if let Some(component) = component {
            file.write(&component)?;
        }
        let compiling = if engine.is_compiled(file.path())? {
            None
        } else {
            frames.push(RunEvent::Compiling);
            dev_compiles
                .as_ref()
                .map(|compiles| compiles.compiling(&compiling_id))
        };
        let program = engine.load(file.path())?;
        drop(compiling);
        Ok::<_, anyhow::Error>((program, file))
    })
    .await
    .context("the load's thread")??;
    Ok((program.named(id.as_str()), file))
}

/// Queues what WaOS asks the program `id`'s user as each is asked, from `told`; never resolves
/// while WaOS runs.
async fn tell_asking(
    mut told: broadcast::Receiver<(String, waos_dev_protocol::Asked)>,
    id: &ProgramId,
    frames: &Arc<Frames>,
) {
    loop {
        match told.recv().await {
            Ok((name, asked)) if name == id.as_str() => frames.push(RunEvent::Asking(asked)),
            Ok(_) => {}
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(
                    missed,
                    "the run missed what some programs' users were asked"
                );
            }
            Err(RecvError::Closed) => std::future::pending().await,
        }
    }
}

/// Resolves once the other side closed. What it sends after its `Run` is not part of the
/// protocol and is dropped.
async fn closed(mut reading: Connection<ReadHalf<ServerStream>>) {
    loop {
        match reading.receive::<waos_dev_protocol::Request>().await {
            Ok(Some(message)) => tracing::warn!(?message, "a message after the run was asked"),
            Ok(None) | Err(_) => return,
        }
    }
}

/// A program's output, as `Output` events on its run's connection.
struct OutputToConnection {
    frames: Arc<Frames>,
}

impl ProgramOutput for OutputToConnection {
    fn poll_write(
        &self,
        stream: OutputStream,
        cx: &mut TaskContext<'_>,
        bytes: &[u8],
    ) -> Poll<usize> {
        let stream = match stream {
            OutputStream::Stdout => ProtocolStream::Stdout,
            OutputStream::Stderr => ProtocolStream::Stderr,
        };
        self.frames.poll_write(stream, cx, bytes)
    }
}
