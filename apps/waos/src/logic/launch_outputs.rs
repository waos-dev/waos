//! What each program `waos launch` started writes, and how it ended, for `waos attach` (the
//! design's decision 70): a launch's output goes to every attach's connection as the program
//! writes it, with backpressure, so a program writing faster than the slowest attach reads waits,
//! as on a pipe; while nobody is attached it is kept, its last [`KEPT_UNATTACHED`] bytes, and the
//! next attach hears it first, so what a program writes between its launch and `waos run`'s attach,
//! or a tool's whole output when it ends before anything attaches, is not lost. A launch that
//! ended with nobody attached is kept until one attach hears how, or the next launch of its id.
//!
//! The invariants: every attach hears `Started` first, then what was kept, then what the program
//! writes, in the order written, then `Ended` once; an attach that went away holds back no write;
//! and a program never waits on output while nobody is attached.

mod frames;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

pub(crate) use frames::Frames;
use waos::{OutputStream, ProgramOutput};
use waos_dev_protocol::{OutputStream as ProtocolStream, RunEnd, RunEvent};
use waos_package::ProgramId;

/// How much of a program's output is kept while nobody is attached: what the next attach hears
/// first. Older output goes as newer comes.
pub(crate) const KEPT_UNATTACHED: usize = 1 << 20;

/// What one launch writes and how it ends, for the attaches to it; the engine writes the
/// program's output here.
#[derive(Default)]
pub(crate) struct LaunchOutput {
    state: Mutex<OutputState>,
}

#[derive(Default)]
struct OutputState {
    /// What the program wrote while nobody was attached, oldest first.
    kept: VecDeque<(ProtocolStream, Vec<u8>)>,
    /// The bytes in `kept`, which [`KEPT_UNATTACHED`] bounds.
    kept_bytes: usize,
    /// Each attach's queue.
    attached: Vec<Arc<Frames>>,
    /// How the run ended, once it has.
    end: Option<RunEnd>,
}

impl LaunchOutput {
    pub(crate) fn new() -> Arc<LaunchOutput> {
        Arc::default()
    }

    /// Attaches a queue that hears `Started`, what was kept, then what the program writes and how
    /// it ends; one that ended already hears how, and is closed.
    pub(crate) fn attach(&self) -> Arc<Frames> {
        let frames = Frames::new();
        frames.push(RunEvent::Started);
        let mut state = self.lock();
        state.kept_bytes = 0;
        for (stream, bytes) in state.kept.drain(..) {
            frames.push_output(stream, &bytes);
        }
        match &state.end {
            Some(end) => {
                frames.push(RunEvent::Ended(end.clone()));
                frames.close();
            }
            None => state.attached.push(Arc::clone(&frames)),
        }
        frames
    }

    /// The run ended as `end`: every attach hears it and is closed. Answers whether any attach
    /// heard it.
    pub(crate) fn ended(&self, end: RunEnd) -> bool {
        let mut state = self.lock();
        let attached = std::mem::take(&mut state.attached);
        let mut heard = false;
        for frames in attached {
            heard |= !frames.is_closed();
            frames.push(RunEvent::Ended(end.clone()));
            frames.close();
        }
        state.end = Some(end);
        heard
    }

    /// Whether the run ended.
    pub(crate) fn has_ended(&self) -> bool {
        self.lock().end.is_some()
    }

    /// Keeps `bytes` for the next attach, as the last [`KEPT_UNATTACHED`] bytes written.
    fn keep(state: &mut OutputState, stream: ProtocolStream, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(KEPT_UNATTACHED)..];
        state.kept_bytes += bytes.len();
        state.kept.push_back((stream, bytes.to_vec()));
        while state.kept_bytes > KEPT_UNATTACHED {
            let Some((_, oldest)) = state.kept.pop_front() else {
                break;
            };
            state.kept_bytes -= oldest.len();
        }
    }

    fn lock(&self) -> MutexGuard<'_, OutputState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ProgramOutput for LaunchOutput {
    fn poll_write(&self, stream: OutputStream, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<usize> {
        if bytes.is_empty() {
            return Poll::Ready(0);
        }
        let stream = match stream {
            OutputStream::Stdout => ProtocolStream::Stdout,
            OutputStream::Stderr => ProtocolStream::Stderr,
        };
        let mut state = self.lock();
        state.attached.retain(|frames| !frames.is_closed());
        if state.attached.is_empty() {
            Self::keep(&mut state, stream, bytes);
            return Poll::Ready(bytes.len());
        }
        let mut room = usize::MAX;
        let mut full = false;
        for frames in &state.attached {
            match frames.poll_room(cx) {
                Poll::Ready(each) => room = room.min(each),
                Poll::Pending => full = true,
            }
        }
        if full {
            return Poll::Pending;
        }
        let written = &bytes[..bytes.len().min(room)];
        for frames in &state.attached {
            frames.push_output(stream, written);
        }
        Poll::Ready(written.len())
    }
}

/// The output of each id's last launch, while it runs, and after it ended until one attach heard
/// how.
#[derive(Default)]
pub(crate) struct LaunchOutputs {
    launches: Mutex<HashMap<ProgramId, Arc<LaunchOutput>>>,
}

impl LaunchOutputs {
    /// The output of a new launch of `id`, in place of the one before.
    pub(crate) fn launched(&self, id: &ProgramId) -> Arc<LaunchOutput> {
        let output = LaunchOutput::new();
        self.lock().insert(id.clone(), Arc::clone(&output));
        output
    }

    /// The output of the last launch of `id`, if one is kept.
    pub(crate) fn of(&self, id: &ProgramId) -> Option<Arc<LaunchOutput>> {
        self.lock().get(id).cloned()
    }

    /// Lets go of `output`, the launch of `id` whose end an attach heard, unless a later launch
    /// took its place already.
    pub(crate) fn heard(&self, id: &ProgramId, output: &Arc<LaunchOutput>) {
        let mut launches = self.lock();
        if launches
            .get(id)
            .is_some_and(|kept| Arc::ptr_eq(kept, output))
        {
            launches.remove(id);
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ProgramId, Arc<LaunchOutput>>> {
        self.launches.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;
    use std::time::Duration;

    use super::*;

    async fn write(output: &LaunchOutput, bytes: &[u8]) -> usize {
        poll_fn(|cx| output.poll_write(OutputStream::Stdout, cx, bytes)).await
    }

    /// Every frame `frames` holds until it is closed.
    async fn drained(frames: &Frames) -> Vec<RunEvent> {
        let mut events = Vec::new();
        while let Some(event) = frames.next().await {
            events.push(event);
        }
        events
    }

    fn output(bytes: &[u8]) -> RunEvent {
        RunEvent::Output {
            stream: ProtocolStream::Stdout,
            bytes: bytes.to_vec(),
        }
    }

    #[tokio::test]
    async fn an_attach_hears_what_was_written_before_it_then_what_follows_then_the_end() {
        let launch = LaunchOutput::new();
        assert_eq!(write(&launch, b"before\n").await, 7);
        let frames = launch.attach();
        write(&launch, b"after\n").await;
        assert!(launch.ended(RunEnd::Returned { success: true }));
        assert_eq!(
            drained(&frames).await,
            [
                RunEvent::Started,
                output(b"before\n"),
                output(b"after\n"),
                RunEvent::Ended(RunEnd::Returned { success: true }),
            ]
        );
    }

    #[tokio::test]
    async fn a_launch_that_ended_unheard_tells_the_next_attach_what_it_wrote_and_how_it_ended() {
        let launch = LaunchOutput::new();
        write(&launch, b"all of it\n").await;
        assert!(
            !launch.ended(RunEnd::Returned { success: false }),
            "unheard"
        );
        assert!(launch.has_ended());
        assert_eq!(
            drained(&launch.attach()).await,
            [
                RunEvent::Started,
                output(b"all of it\n"),
                RunEvent::Ended(RunEnd::Returned { success: false }),
            ]
        );
    }

    #[tokio::test]
    async fn while_nobody_is_attached_only_the_last_of_the_output_is_kept_and_nothing_waits() {
        let launch = LaunchOutput::new();
        for _ in 0..3 {
            assert_eq!(
                write(&launch, &vec![1; KEPT_UNATTACHED]).await,
                KEPT_UNATTACHED
            );
        }
        write(&launch, b"last").await;
        launch.ended(RunEnd::Stopped);
        let kept: usize = drained(&launch.attach())
            .await
            .iter()
            .map(|event| match event {
                RunEvent::Output { bytes, .. } => bytes.len(),
                _ => 0,
            })
            .sum();
        assert!(kept <= KEPT_UNATTACHED, "{kept}");
    }

    #[tokio::test]
    async fn an_attach_that_reads_nothing_holds_the_program_back_until_it_goes() {
        let launch = LaunchOutput::new();
        let frames = launch.attach();
        assert_eq!(frames.next().await, Some(RunEvent::Started));
        write(&launch, &vec![1; frames::OUTPUT_IN_FLIGHT]).await;
        let writer = tokio::spawn({
            let launch = Arc::clone(&launch);
            async move { write(&launch, b"more").await }
        });
        tokio::task::yield_now().await;
        assert!(!writer.is_finished(), "waits as on a pipe");
        frames.close();
        let accepted = tokio::time::timeout(Duration::from_secs(1), writer)
            .await
            .expect("the attach going lets the program go on")
            .unwrap();
        assert_eq!(accepted, 4);
    }

    #[test]
    fn a_later_launch_keeps_its_own_output_when_the_one_before_is_heard() {
        let launches = LaunchOutputs::default();
        let id: ProgramId = "dev.example.notes.development".parse().unwrap();
        let first = launches.launched(&id);
        let second = launches.launched(&id);
        launches.heard(&id, &first);
        assert!(Arc::ptr_eq(&launches.of(&id).unwrap(), &second));
        launches.heard(&id, &second);
        assert!(launches.of(&id).is_none());
    }
}
