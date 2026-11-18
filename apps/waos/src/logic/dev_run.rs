//! One connection from `cargo waos`, which is one run (the design's decision 29), once its `Run`
//! and the component it announced have arrived (`dev_server`): the component kept in a file of
//! the app's own (`sent_components`) and loaded off this thread, which compiles it unless the code
//! cache holds its code, with `Compiling` on the connection first, before the run takes the runs'
//! lock, which every start, screenshot, input and quit takes, so a compile holds back no other
//! program; the program run as the one its id names, with that program's settings and folders,
//! installed or not (decision 11 of the permissions proposal), with this computer's
//! capabilities and its output as `Output` frames on the connection, `Started`, then `Ended`
//! with how it ended; a component that cannot load or run is `Refused` with why, and the
//! connection closing stops the program. The run is watched while its connection is open (the
//! design's decision 63): from its start, so its windows count as in sight though other windows
//! cover them, and a tool's screenshot and input meet windows that draw, until the connection
//! closes or the program ends. What WaOS asks the program's user meanwhile, a row of its
//! settings on Ask, the connection hears as `Asking` as it is asked (`asking_now`).
//!
//! The invariant: every frame to the connection goes through one queue, in the order it was
//! queued, so `Started` comes before the first `Output` and `Ended` after the last, and the
//! queue is closed only after the program has ended.

use std::sync::Arc;
use std::time::Instant;

use super::asking_now::AskingNow;
use super::dev_compiles::DevCompiles;
use super::environments::ProgramEnvironments;
use super::runs::{Runs, SentRun};
use super::sent_components::{SentComponent, SentComponents};
use anyhow::{Context, Result};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing::Instrument;
use waos::{OutputStream, Program, ProgramEnvironment, ProgramOutput, WaOS, Watching};
use waos_dev_protocol::{Connection, FromWaOS, ServerStream, ToWaOS};
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

/// Serves `connection` to its end: the run of `component` as the program `id` it asked for, with
/// `variables` added to its environment.
pub(crate) async fn serve(
    connection: Connection<ServerStream>,
    id: ProgramId,
    component: Vec<u8>,
    variables: Vec<(String, String)>,
    context: RunContext,
) {
    let (reading, writing) = connection.into_split();
    let (frames, queued) = mpsc::unbounded_channel();
    let sending = tokio::spawn(send_frames(writing, queued));
    run(&id, component, variables, reading, &context, &frames)
        .instrument(tracing::info_span!("run", name = id.as_str()))
        .await;
    drop(frames);
    // The sender ends with the queue; nothing to hear from it.
    let _ = sending.await;
}

/// Sends every frame queued, in order, until the queue is closed; a connection gone ends it.
async fn send_frames(
    mut writing: Connection<WriteHalf<ServerStream>>,
    mut queued: UnboundedReceiver<FromWaOS>,
) {
    while let Some(frame) = queued.recv().await {
        if writing.send(&frame).await.is_err() {
            return;
        }
    }
}

/// Runs the component sent as the program `id` until it ends or the connection closes,
/// queueing what the connection hears of it, with `variables` added to its environment.
async fn run(
    id: &ProgramId,
    component: Vec<u8>,
    variables: Vec<(String, String)>,
    reading: Connection<ReadHalf<ServerStream>>,
    context: &RunContext,
    frames: &UnboundedSender<FromWaOS>,
) {
    let told = context.asking.told();
    let (sent, watching) = match start(id, component, variables, context, frames).await {
        Ok(started) => started,
        Err(why) => {
            tracing::info!("refused: {why:#}");
            queue(frames, FromWaOS::Refused(format!("{why:#}")));
            return;
        }
    };
    tracing::info!("started");
    let started_at = Instant::now();
    let end = tokio::select! {
        end = sent.run.program.ended() => Some(end),
        () = tell_asking(told, id, frames) => None,
        () = closed(reading) => {
            tracing::info!("the connection closed: stopping the program");
            drop(watching);
            sent.run.program.stop();
            sent.run.program.ended().await;
            None
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
        queue(frames, FromWaOS::Ended(sent.end(&end)));
    }
    context.runs.ended(&sent.run).await;
}

/// Starts the component sent as the program `id`, watched: loaded first, which compiles it,
/// while no other program waits for it, then started among the runs in place of the program
/// running under the id, with `variables` added to its environment. The id's file is held until
/// the run is among the runs, so the runs of one id start in the order they came.
async fn start(
    id: &ProgramId,
    component: Vec<u8>,
    variables: Vec<(String, String)>,
    context: &RunContext,
    frames: &UnboundedSender<FromWaOS>,
) -> Result<(SentRun, Watching)> {
    let file = context.components.hold(id).await;
    let (program, _file) = load(id, component, file, context, frames).await?;
    let mut watching = None;
    let sent = context
        .runs
        .start(id, || {
            let environment = context.environments.of(id)?;
            // Queued before the program can write anything, so it comes first.
            queue(frames, FromWaOS::Started);
            let environment = ProgramEnvironment {
                output: Some(Arc::new(OutputToConnection {
                    frames: frames.clone(),
                })),
                variables,
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

/// The program `id`, named by it: `component` written to `file`, the id's, and loaded from
/// there, off this thread, since a load compiles and blocks while it does. The connection hears
/// `Compiling` before a load that compiles. Answers the file with the program, still held.
async fn load(
    id: &ProgramId,
    component: Vec<u8>,
    file: SentComponent,
    context: &RunContext,
    frames: &UnboundedSender<FromWaOS>,
) -> Result<(Program, SentComponent)> {
    let engine = Arc::clone(&context.engine);
    let frames = frames.clone();
    let dev_compiles = context.dev_compiles.clone();
    let compiling_id = id.clone();
    let (program, file) = tokio::task::spawn_blocking(move || {
        file.write(&component)?;
        let compiling = if engine.is_compiled(file.path())? {
            None
        } else {
            queue(&frames, FromWaOS::Compiling);
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
    frames: &UnboundedSender<FromWaOS>,
) {
    loop {
        match told.recv().await {
            Ok((name, asked)) if name == id.as_str() => queue(frames, FromWaOS::Asking(asked)),
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
        match reading.receive::<ToWaOS>().await {
            Ok(Some(message)) => tracing::warn!(?message, "a message after the run was asked"),
            Ok(None) | Err(_) => return,
        }
    }
}

/// Queues a frame; a connection already gone has nobody to send to.
fn queue(frames: &UnboundedSender<FromWaOS>, frame: FromWaOS) {
    let _ = frames.send(frame);
}

/// A program's output, as `Output` frames on its run's connection.
struct OutputToConnection {
    frames: UnboundedSender<FromWaOS>,
}

impl ProgramOutput for OutputToConnection {
    fn write(&self, stream: OutputStream, bytes: &[u8]) {
        let stream = match stream {
            OutputStream::Stdout => waos_dev_protocol::OutputStream::Stdout,
            OutputStream::Stderr => waos_dev_protocol::OutputStream::Stderr,
        };
        queue(
            &self.frames,
            FromWaOS::Output {
                stream,
                bytes: bytes.to_vec(),
            },
        );
    }
}
