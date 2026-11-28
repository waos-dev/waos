//! Bounded queue of [`RunEvent`] frames from a running program to one attach's connection.
//!
//! The invariants: every frame leaves in the order pushed (`Started` before `Output` and `Ended`
//! last); queued `Output` bytes stay within [`OUTPUT_IN_FLIGHT`] via async backpressure so a slow
//! or unread client suspends writes with a [`Waker`] rather than blocking the program's executor
//! thread; and [`Frames::close`] wakes waiting writers and drops further output so stopping a
//! program, or detaching, never waits on output.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

use tokio::sync::Notify;
use waos_dev_protocol::{OutputStream as ProtocolStream, RunEvent};

/// How much of a program's output may wait to go over the connection: a program writing faster
/// than it carries pauses cleanly at the async write boundary.
pub(super) const OUTPUT_IN_FLIGHT: usize = 1 << 20;

/// The frames a run's connection sends, in the order queued, with async backpressure on output.
pub(crate) struct Frames {
    queued: Mutex<Queued>,
    /// Wakes the connection sender when a frame is queued or the run ends.
    wake: Notify,
}

#[derive(Default)]
struct Queued {
    frames: VecDeque<RunEvent>,
    /// The bytes of the queued `Output` events, which [`OUTPUT_IN_FLIGHT`] bounds.
    output: usize,
    closed: bool,
    /// Writers waiting for `output` to drop below [`OUTPUT_IN_FLIGHT`].
    writers: Vec<Waker>,
}

impl Frames {
    pub(in crate::logic) fn new() -> Arc<Frames> {
        Arc::new(Frames {
            queued: Mutex::default(),
            wake: Notify::new(),
        })
    }

    /// Queues `event`; a queue already closed drops it.
    pub(in crate::logic) fn push(&self, event: RunEvent) {
        let mut queued = self.lock();
        if queued.closed {
            return;
        }
        queued.frames.push_back(event);
        drop(queued);
        self.wake.notify_one();
    }

    /// Polls for room for output: how many bytes [`OUTPUT_IN_FLIGHT`] leaves room for now. Returns
    /// [`Poll::Pending`] and keeps `cx.waker()` when there is none yet, or any amount once closed,
    /// so a stopped run's or a detached attach's output drains instead of hanging.
    pub(super) fn poll_room(&self, cx: &mut Context<'_>) -> Poll<usize> {
        let mut queued = self.lock();
        if queued.closed {
            return Poll::Ready(usize::MAX);
        }
        let room = OUTPUT_IN_FLIGHT.saturating_sub(queued.output);
        if room == 0 {
            if !queued
                .writers
                .iter()
                .any(|waker| waker.will_wake(cx.waker()))
            {
                queued.writers.push(cx.waker().clone());
            }
            return Poll::Pending;
        }
        Poll::Ready(room)
    }

    /// Queues `bytes` the program wrote to `stream`, counted against [`OUTPUT_IN_FLIGHT`], which
    /// the caller found room for; a queue already closed drops them.
    pub(super) fn push_output(&self, stream: ProtocolStream, bytes: &[u8]) {
        let mut queued = self.lock();
        if queued.closed || bytes.is_empty() {
            return;
        }
        queued.output += bytes.len();
        queued.frames.push_back(RunEvent::Output {
            stream,
            bytes: bytes.to_vec(),
        });
        drop(queued);
        self.wake.notify_one();
    }

    /// Whether the queue is closed: its attach is gone, or the run ended.
    pub(super) fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// The next frame, once one is queued; none once closed and drained.
    pub(crate) async fn next(&self) -> Option<RunEvent> {
        loop {
            let woken = self.wake.notified();
            {
                let mut queued = self.lock();
                if let Some(event) = queued.frames.pop_front() {
                    let writers = if let RunEvent::Output { bytes, .. } = &event {
                        queued.output -= bytes.len();
                        std::mem::take(&mut queued.writers)
                    } else {
                        Vec::new()
                    };
                    drop(queued);
                    for waker in writers {
                        waker.wake();
                    }
                    return Some(event);
                }
                if queued.closed {
                    return None;
                }
            }
            woken.await;
        }
    }

    /// Closes the queue: waiting writers wake to drop further output, and the sender ends once
    /// buffered frames drain.
    pub(in crate::logic) fn close(&self) {
        let writers = {
            let mut queued = self.lock();
            queued.closed = true;
            std::mem::take(&mut queued.writers)
        };
        for waker in writers {
            waker.wake();
        }
        self.wake.notify_one();
    }

    fn lock(&self) -> MutexGuard<'_, Queued> {
        self.queued.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;
    use std::time::Duration;

    use super::*;

    /// Writes as much of `bytes` as there is room for, as a program's output does.
    async fn write(frames: &Frames, bytes: &[u8]) -> usize {
        let room = poll_fn(|cx| frames.poll_room(cx)).await;
        let take = bytes.len().min(room);
        frames.push_output(ProtocolStream::Stdout, &bytes[..take]);
        take
    }

    #[tokio::test]
    async fn output_waits_with_pending_until_room_opens() {
        let frames = Frames::new();
        assert_eq!(
            write(&frames, &vec![1; OUTPUT_IN_FLIGHT]).await,
            OUTPUT_IN_FLIGHT
        );

        let writer = tokio::spawn({
            let frames = Arc::clone(&frames);
            async move { write(&frames, b"more").await }
        });
        tokio::task::yield_now().await;
        assert!(!writer.is_finished(), "a full queue suspends with Pending");

        let first = frames.next().await.unwrap();
        assert!(matches!(first, RunEvent::Output { .. }));
        let accepted = tokio::time::timeout(Duration::from_secs(1), writer)
            .await
            .expect("room woke the writer")
            .unwrap();
        assert_eq!(accepted, 4);
        let second = frames.next().await.unwrap();
        assert!(
            matches!(&second, RunEvent::Output { bytes, .. } if bytes == b"more"),
            "{second:?}"
        );
    }

    #[tokio::test]
    async fn a_closed_queue_drops_what_a_program_writes_and_ends_the_sender() {
        let frames = Frames::new();
        write(&frames, b"kept").await;
        frames.close();
        assert!(frames.is_closed());
        assert_eq!(write(&frames, b"dropped").await, b"dropped".len());
        assert!(matches!(frames.next().await, Some(RunEvent::Output { .. })));
        assert!(
            frames.next().await.is_none(),
            "the queue is drained and closed"
        );
    }

    #[tokio::test]
    async fn a_queue_closed_with_output_waiting_lets_the_writer_go() {
        let frames = Frames::new();
        write(&frames, &vec![1; OUTPUT_IN_FLIGHT]).await;
        let writer = tokio::spawn({
            let frames = Arc::clone(&frames);
            async move { write(&frames, b"more").await }
        });
        tokio::task::yield_now().await;
        frames.close();
        let accepted = tokio::time::timeout(Duration::from_secs(1), writer)
            .await
            .expect("close woke the writer")
            .unwrap();
        assert_eq!(accepted, 4);
    }
}
