//! An inspection `waos inspect` asks for (the inspector proposal's protocol): a program's
//! audit summary, the days kept with today's as the engine counts it now; what it uses
//! now and each change to it; or its live trace, each event as it comes; the last two until the
//! connection closes or the program ends.
//! Each value goes as JSON of `wasdk:inspect`'s, one a line, so the command prints what it reads.
//!
//! The invariant: only a program a run of `waos run` on this machine runs is inspected live,
//! and only one it runs or has run has its summary read, as its settings are (`dev_settings`);
//! never a program the library opened, whose record is its user's to read.

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use waos::{RunningProgram, TraceKind, WaOS};
use waos_dev_protocol::{Connection, InspectAnswer, Inspection, ServerStream, TracedCapability};
use waos_package::ProgramId;

use super::dev_refusal;
use super::dev_server::BUILD;
use super::runs::Runs;
use super::sent_components::SentComponents;

/// What an inspection reads of the app.
pub(crate) struct InspectionContext<'a> {
    pub(crate) runs: &'a Runs,
    pub(crate) components: &'a SentComponents,
    pub(crate) engine: &'a WaOS,
}

/// Answers a connection's `Inspect`: what `asked` asks of the program `id`.
pub(crate) async fn answer(
    mut connection: Connection<ServerStream>,
    context: InspectionContext<'_>,
    id: &ProgramId,
    asked: Inspection,
) {
    let running = context.runs.dev_run(id).await.map(|run| run.program);
    let answered = match asked {
        Inspection::Summary { days } => {
            let was_run = running.is_some() || context.components.was_sent(id);
            if was_run {
                Ok(summary(context.engine, id, days))
            } else {
                Err(dev_refusal::never_dev_run(id))
            }
        }
        Inspection::ActiveUses => match running.as_ref().and_then(RunningProgram::active_uses) {
            Some((now, changes)) => {
                let changes = changes.map(|change| json(&change)).boxed();
                send_lines(connection, vec![json(&now)], changes).await;
                return;
            }
            None => Err(dev_refusal::no_dev_run_now(id)),
        },
        Inspection::Trace(traced) => {
            let Some(kinds) = traced
                .iter()
                .map(|each| kind_of(*each))
                .collect::<Option<Vec<_>>>()
            else {
                let refusal = dev_refusal::unsupported(BUILD, "inspection");
                let _ = connection.send(&InspectAnswer::Refused(refusal)).await;
                return;
            };
            match running {
                Some(program) => {
                    trace(connection, &program, &kinds).await;
                    return;
                }
                None => Err(dev_refusal::no_dev_run_now(id)),
            }
        }
        Inspection::Unknown => Err(dev_refusal::unsupported(BUILD, "inspection")),
    };
    let answer = match answered {
        Ok(lines) => InspectAnswer::Inspected(lines),
        Err(refusal) => InspectAnswer::Refused(refusal),
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// The program's last `days` days, newest first, today's as the engine counts it now.
fn summary(engine: &WaOS, id: &ProgramId, days: u32) -> Vec<String> {
    engine
        .days_of(id.as_str())
        .iter()
        .take(usize::try_from(days).unwrap_or(usize::MAX))
        .map(|day| json(&waos::inspection::day_of(day)))
        .collect()
}

/// Sends each event of `program`'s uses of `kinds` as it comes, until the connection closes or
/// the program ends.
async fn trace(
    connection: Connection<ServerStream>,
    program: &RunningProgram,
    kinds: &[TraceKind],
) {
    let streams: Vec<BoxStream<'static, waos::TraceEvent>> = kinds
        .iter()
        .filter_map(|kind| program.trace(*kind))
        .map(StreamExt::boxed)
        .collect();
    let events = stream::select_all(streams).map(|event| json(&event));
    send_lines(connection, Vec::new(), events.boxed()).await;
}

/// Sends `first`, where there is anything, then each of `lines` as it comes, a line an
/// `Inspected`, until the connection closes or the lines end.
async fn send_lines(
    connection: Connection<ServerStream>,
    first: Vec<String>,
    mut lines: BoxStream<'static, String>,
) {
    let (mut reading, mut writing) = connection.into_split();
    if !first.is_empty()
        && writing
            .send(&InspectAnswer::Inspected(first))
            .await
            .is_err()
    {
        return;
    }
    // The client says nothing more: anything it sends, or its closing, ends what is sent.
    let closed = reading.receive::<waos_dev_protocol::Request>();
    tokio::pin!(closed);
    loop {
        tokio::select! {
            line = lines.next() => {
                let Some(line) = line else { return };
                if writing.send(&InspectAnswer::Inspected(vec![line])).await.is_err() {
                    return;
                }
            }
            _ = &mut closed => return,
        }
    }
}

fn kind_of(traced: TracedCapability) -> Option<TraceKind> {
    Some(match traced {
        TracedCapability::Network => TraceKind::Network,
        TracedCapability::Files => TraceKind::Files,
        TracedCapability::Clipboard => TraceKind::Clipboard,
        TracedCapability::Camera => TraceKind::Camera,
        TracedCapability::Microphone => TraceKind::Microphone,
        TracedCapability::Machines => TraceKind::Machines,
        TracedCapability::Permissions => TraceKind::Permissions,
        TracedCapability::Links => TraceKind::Links,
        TracedCapability::Notifications => TraceKind::Notifications,
        TracedCapability::Output => TraceKind::Output,
        TracedCapability::Unknown => return None,
    })
}

/// `value` as one line of JSON.
fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| {
        tracing::warn!(%error, "a value of wasdk:inspect does not serialize");
        "null".to_owned()
    })
}
