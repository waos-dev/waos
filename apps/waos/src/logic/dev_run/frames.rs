//! Bounded queue of [`RunEvent`] frames from a running program to its dev connection.
//!
//! The invariants: every frame leaves in the order pushed (`Started` before `Output` and `Ended`
//! last); queued `Output` bytes stay within [`OUTPUT_IN_FLIGHT`] via async backpressure so a slow
//! or unread client suspends writes with a [`Waker`] rather than blocking the program's executor
//! thread; and [`Frames::close`] wakes waiting writers and drops further output so stopping a
//! program never waits on output.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

use tokio::sync::Notify;
use waos_dev_protocol::{OutputStream as ProtocolStream, RunEvent};

/// How much of a program's output may wait to go over the connection: a program writing faster
/// than it carries pauses cleanly at the async write boundary.
pub(super) const OUTPUT_IN_FLIGHT: usize = 1 << 20;

/// The frames a run's connection sends, in the order queued, with async backpressure on output.
pub(super) struct Frames {
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
    pub(super) fn new() -> Arc<Frames> {
        Arc::new(Frames {
            queued: Mutex::default(),
            wake: Notify::new(),
        })
    }

    /// Queues `event`; a queue already closed drops it.
    pub(super) fn push(&self, event: RunEvent) {
        let mut queued = self.lock();
        if queued.closed {
            return;
        }
        queued.frames.push_back(event);
        drop(queued);
        self.wake.notify_one();
    }

    /// Polls writing as much of `bytes` as [`OUTPUT_IN_FLIGHT`] allows without exceeding it.
    /// Returns [`Poll::Pending`] and keeps `cx.waker()` when no room is open yet, or accepts all
    /// bytes once closed so a stopped run's output drains instead of hanging.
    pub(super) fn poll_write(
        &self,
        stream: ProtocolStream,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<usize> {
        if bytes.is_empty() {
            return Poll::Ready(0);
        }
        let mut queued = self.lock();
        if queued.closed {
            return Poll::Ready(bytes.len());
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
        let take = bytes.len().min(room);
        queued.output += take;
        queued.frames.push_back(RunEvent::Output {
            stream,
            bytes: bytes[..take].to_vec(),
        });
        drop(queued);
        self.wake.notify_one();
        Poll::Ready(take)
    }

    /// The next frame, once one is queued; none once closed and drained.
    pub(super) async fn next(&self) -> Option<RunEvent> {
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
    pub(super) fn close(&self) {
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

    #[tokio::test]
    async fn output_waits_with_pending_until_room_opens() {
        let frames = Frames::new();
        poll_fn(|cx| {
            assert_eq!(
                frames.poll_write(ProtocolStream::Stdout, cx, &vec![1; OUTPUT_IN_FLIGHT]),
                Poll::Ready(OUTPUT_IN_FLIGHT)
            );
            Poll::Ready(())
        })
        .await;

        let writer = tokio::spawn({
            let frames = Arc::clone(&frames);
            async move { poll_fn(|cx| frames.poll_write(ProtocolStream::Stdout, cx, b"more")).await }
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
        poll_fn(|cx| frames.poll_write(ProtocolStream::Stdout, cx, b"kept")).await;
        frames.close();
        let dropped = poll_fn(|cx| frames.poll_write(ProtocolStream::Stdout, cx, b"dropped")).await;
        assert_eq!(dropped, b"dropped".len());
        assert!(matches!(frames.next().await, Some(RunEvent::Output { .. })));
        assert!(
            frames.next().await.is_none(),
            "the queue is drained and closed"
        );
    }

    #[tokio::test]
    async fn a_queue_closed_with_output_waiting_lets_the_writer_go() {
        let frames = Frames::new();
        poll_fn(|cx| frames.poll_write(ProtocolStream::Stdout, cx, &vec![1; OUTPUT_IN_FLIGHT]))
            .await;
        let writer = tokio::spawn({
            let frames = Arc::clone(&frames);
            async move { poll_fn(|cx| frames.poll_write(ProtocolStream::Stdout, cx, b"more")).await }
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
