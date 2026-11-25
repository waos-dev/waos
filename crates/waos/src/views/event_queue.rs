//! A view's events on their way to the program, in the order they happened: each as the program
//! reads it where it holds no resource, and otherwise with the engine's own value for the
//! resource, which the code serving the view makes a handle of as the program reads the event.
//!
//! The shell pushes from its thread and the program reads from its own, through the one stream a
//! view hands out. The invariant: nothing is dropped. While the program is behind, a pointer's
//! consecutive moves are merged into the latest, and so are the view's consecutive changes of
//! geometry; everything else waits its turn. Events are counted as they are queued and as the
//! program reads them, a merged one counting as the one it merged into, so whoever knows how many
//! were queued at some moment knows when the program has read up to there.
//!
//! What decides whether the program goes on, a request to close the view and the view's going, holds
//! the program unfrozen until it has handled it: the request until the program reads again, the end
//! until it drops the stream; and only where the stream was taken, so a program that reads nothing
//! is not unfrozen for nothing.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use futures::Stream;

use crate::bindings::wasdk::platform::views::{
    KeyEvent, PointerChange, PointerEvent, PointerSignalKind, ViewEvent,
};
use crate::clipboard::{CopyRequest, Paste};
use crate::lifecycle::{FreezeInhibitor, FreezeInhibitorKind, Lifecycle};
use crate::news::MaybeInhibiting;
use crate::sync::locked;

use super::key_reply::KeyReply;

/// One event on its way to the program.
pub(crate) enum QueuedEvent {
    /// An event that holds no resource, as the program reads it.
    Plain(ViewEvent),
    /// A key, with the reply the shell waits on.
    Key(KeyEvent, KeyReply),
    /// The user's copy, and the request that writes it.
    Copy(CopyRequest),
    /// The user's cut, and the request that writes it.
    Cut(CopyRequest),
    /// The user's paste, and the clipboard as they pasted it.
    Paste(Paste),
}

/// The queue both sides hold.
#[derive(Clone, Default)]
pub(super) struct EventQueue(Arc<Mutex<Queued>>);

#[derive(Default)]
struct Queued {
    events: VecDeque<MaybeInhibiting<QueuedEvent>>,
    /// The program's task, waiting for the next event.
    reader: Option<Waker>,
    /// Whether the stream was handed out: a view has one reader.
    taken: bool,
    /// The view is gone: what is queued is read, and then the stream ends.
    ended: bool,
    /// What holds the program unfrozen from the view's going until it drops the stream.
    end_inhibitor: Option<Arc<FreezeInhibitor>>,
    /// How many events were queued, and how many of them the program read.
    queued: u64,
    read: u64,
}

/// A view's events, as the program reads them, each with what holds the program unfrozen while it
/// handles it; and once they end, what holds it unfrozen until it drops them.
pub(crate) struct ViewEvents {
    queue: EventQueue,
    end_inhibitor: Option<Arc<FreezeInhibitor>>,
}

impl EventQueue {
    /// Queues `event` after every event before it.
    pub(super) fn push(&self, event: QueuedEvent) {
        let mut queued = locked(&self.0);
        if queued.ended {
            return;
        }
        if !merged_into_last(&mut queued.events, &event) {
            queued.events.push_back((event, None));
            queued.queued += 1;
        }
        wake(&mut queued);
    }

    /// Queues `event`, which decides whether the program goes on, after every event before it,
    /// the program, whose lifecycle is `lifecycle`, unfrozen until it has handled it.
    pub(super) fn push_waking(&self, event: QueuedEvent, lifecycle: &Lifecycle) {
        let mut queued = locked(&self.0);
        if queued.ended {
            return;
        }
        let unfreeze = inhibitor_if_read(&queued, lifecycle);
        queued.events.push_back((event, unfreeze));
        queued.queued += 1;
        wake(&mut queued);
    }

    /// How many events were queued so far.
    pub(super) fn queued(&self) -> u64 {
        locked(&self.0).queued
    }

    /// How many of them the program has read.
    pub(super) fn read(&self) -> u64 {
        locked(&self.0).read
    }

    /// The view is gone, which decides whether the program goes on: the stream ends once what is
    /// queued has been read, the program, whose lifecycle is `lifecycle`, unfrozen until it has
    /// read the end and dropped the stream.
    pub(super) fn end(&self, lifecycle: &Lifecycle) {
        let mut queued = locked(&self.0);
        if queued.ended {
            return;
        }
        queued.end_inhibitor = inhibitor_if_read(&queued, lifecycle);
        queued.ended = true;
        wake(&mut queued);
    }

    /// The stream of the view's events, from its arrival on; `None` once it was taken.
    pub(super) fn take(&self) -> Option<ViewEvents> {
        let mut queued = locked(&self.0);
        if std::mem::replace(&mut queued.taken, true) {
            return None;
        }
        Some(ViewEvents {
            queue: self.clone(),
            end_inhibitor: None,
        })
    }
}

/// What holds the program, whose lifecycle is `lifecycle`, unfrozen for an item of `queued`'s, where
/// its stream was taken, so someone will read it.
fn inhibitor_if_read(queued: &Queued, lifecycle: &Lifecycle) -> Option<Arc<FreezeInhibitor>> {
    queued
        .taken
        .then(|| Arc::new(lifecycle.inhibit_freeze(FreezeInhibitorKind::PendingRequest)))
}

impl Stream for ViewEvents {
    type Item = MaybeInhibiting<QueuedEvent>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<MaybeInhibiting<QueuedEvent>>> {
        let this = self.get_mut();
        let mut queued = locked(&this.queue.0);
        if let Some(event) = queued.events.pop_front() {
            queued.read += 1;
            return Poll::Ready(Some(event));
        }
        if queued.ended {
            // Held until the stream is dropped: the program decides what its view's going means.
            if let Some(end_inhibitor) = queued.end_inhibitor.take() {
                this.end_inhibitor = Some(end_inhibitor);
            }
            return Poll::Ready(None);
        }
        queued.reader = Some(cx.waker().clone());
        Poll::Pending
    }
}

fn wake(queued: &mut Queued) {
    if let Some(reader) = queued.reader.take() {
        reader.wake();
    }
}

/// Merges `event` into the last event queued when both say where something is now, not what
/// happened: a change of geometry after a change of geometry, a pointer's move after its move.
/// Answers whether it did.
fn merged_into_last(
    events: &mut VecDeque<MaybeInhibiting<QueuedEvent>>,
    event: &QueuedEvent,
) -> bool {
    let (Some((QueuedEvent::Plain(last), None)), QueuedEvent::Plain(event)) =
        (events.back_mut(), event)
    else {
        return false;
    };
    match (last, event) {
        (ViewEvent::Metrics(last), ViewEvent::Metrics(now)) => {
            *last = *now;
            true
        }
        (ViewEvent::Pointer(last), ViewEvent::Pointer(now)) if continues_move(last, now) => {
            *last = *now;
            true
        }
        _ => false,
    }
}

/// Whether `now` is the same pointer going on moving the way `last` did: the same change,
/// with the same buttons, and no signal such as a scroll, whose deltas add up.
fn continues_move(last: &PointerEvent, now: &PointerEvent) -> bool {
    let moving = matches!(
        now.change,
        PointerChange::Move | PointerChange::Hover | PointerChange::PanZoomUpdate
    );
    moving
        && now.signal == PointerSignalKind::None
        && last.signal == PointerSignalKind::None
        && last.change == now.change
        && last.device == now.device
        && last.buttons == now.buttons
}

#[cfg(test)]
pub(super) mod tests {
    use futures::StreamExt;
    use futures::executor::block_on;

    use crate::bindings::wasdk::platform::views::{
        Frame, Insets, Metrics, PointerButtons, PointerDeviceKind,
    };

    use super::*;

    pub(in crate::views) fn metrics(width: u32) -> Metrics {
        let none = Insets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        Metrics {
            width,
            height: 1,
            device_pixel_ratio: 1.0,
            padding: none,
            view_padding: none,
            view_insets: none,
        }
    }

    pub(in crate::views) fn pointer(change: PointerChange, x: f64) -> PointerEvent {
        PointerEvent {
            time: 0,
            change,
            kind: PointerDeviceKind::Mouse,
            signal: PointerSignalKind::None,
            device: 1,
            x,
            y: 0.0,
            buttons: PointerButtons::empty(),
            pressure: 0.0,
            pressure_min: 0.0,
            pressure_max: 1.0,
            distance: 0.0,
            distance_max: 0.0,
            size: 0.0,
            radius_major: 0.0,
            radius_minor: 0.0,
            radius_min: 0.0,
            radius_max: 0.0,
            orientation: 0.0,
            tilt: 0.0,
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
            pan_x: 0.0,
            pan_y: 0.0,
            scale: 1.0,
            rotation: 0.0,
        }
    }

    /// What the program heard of an event: the event, a key without its reply.
    #[derive(Debug)]
    pub(in crate::views) enum Heard {
        Plain(ViewEvent),
        Key(KeyEvent),
        Copy,
        Cut,
        Paste,
    }

    /// Everything `events` will have, once its view is gone, as the program hears it.
    pub(in crate::views) fn heard(mut events: ViewEvents) -> Vec<Heard> {
        let events: Vec<MaybeInhibiting<QueuedEvent>> =
            block_on(async { events.by_ref().collect().await });
        events
            .into_iter()
            .map(|(event, _unfreeze)| match event {
                QueuedEvent::Plain(event) => Heard::Plain(event),
                QueuedEvent::Key(event, _reply) => Heard::Key(event),
                QueuedEvent::Copy(_) => Heard::Copy,
                QueuedEvent::Cut(_) => Heard::Cut,
                QueuedEvent::Paste(_) => Heard::Paste,
            })
            .collect()
    }

    /// What the program reads of `queue`, every event plain.
    fn drained(queue: &EventQueue) -> Vec<ViewEvent> {
        let events = queue.take().expect("the stream is taken once");
        queue.end(&Lifecycle::default());
        heard(events)
            .into_iter()
            .map(|event| match event {
                Heard::Plain(event) => event,
                event => panic!("these tests queue no {event:?}"),
            })
            .collect()
    }

    fn plain(event: ViewEvent) -> QueuedEvent {
        QueuedEvent::Plain(event)
    }

    #[test]
    fn events_of_every_kind_come_out_in_the_order_they_went_in() {
        let queue = EventQueue::default();
        queue.push(plain(ViewEvent::Pointer(pointer(PointerChange::Down, 1.0))));
        queue.push(plain(ViewEvent::Metrics(metrics(2))));
        queue.push(plain(ViewEvent::Frame(Frame {
            time: 3,
            display_time: 19,
            views: vec![1],
        })));
        queue.push(plain(ViewEvent::CloseRequested));
        let [
            ViewEvent::Pointer(first),
            ViewEvent::Metrics(second),
            ViewEvent::Frame(third),
            ViewEvent::CloseRequested,
        ] = &drained(&queue)[..]
        else {
            panic!("the events are out of order");
        };
        assert_eq!(first.x, 1.0);
        assert_eq!(second.width, 2);
        assert_eq!(third.time, 3);
    }

    #[test]
    fn consecutive_moves_and_resizes_merge_into_the_latest() {
        let queue = EventQueue::default();
        queue.push(plain(ViewEvent::Pointer(pointer(
            PointerChange::Hover,
            1.0,
        ))));
        queue.push(plain(ViewEvent::Pointer(pointer(
            PointerChange::Hover,
            2.0,
        ))));
        queue.push(plain(ViewEvent::Metrics(metrics(1))));
        queue.push(plain(ViewEvent::Metrics(metrics(2))));
        queue.push(plain(ViewEvent::Pointer(pointer(
            PointerChange::Hover,
            3.0,
        ))));
        let [
            ViewEvent::Pointer(first),
            ViewEvent::Metrics(second),
            ViewEvent::Pointer(third),
        ] = &drained(&queue)[..]
        else {
            panic!("three events remain of five");
        };
        assert_eq!(first.x, 2.0, "the latest move");
        assert_eq!(second.width, 2, "the latest geometry");
        assert_eq!(third.x, 3.0);
    }

    #[test]
    fn events_are_counted_as_queued_a_merged_one_as_the_one_it_merged_into_and_as_read() {
        let queue = EventQueue::default();
        let mut events = queue.take().expect("the stream is taken once");
        queue.push(plain(ViewEvent::Pointer(pointer(
            PointerChange::Hover,
            1.0,
        ))));
        queue.push(plain(ViewEvent::Pointer(pointer(
            PointerChange::Hover,
            2.0,
        ))));
        queue.push(plain(ViewEvent::CloseRequested));
        assert_eq!((queue.queued(), queue.read()), (2, 0));
        block_on(events.next()).expect("the merged move");
        assert_eq!(queue.read(), 1);
        block_on(events.next()).expect("the close");
        assert_eq!((queue.queued(), queue.read()), (2, 2));
    }

    #[test]
    fn a_press_between_moves_keeps_them_apart() {
        let queue = EventQueue::default();
        queue.push(plain(ViewEvent::Pointer(pointer(
            PointerChange::Hover,
            1.0,
        ))));
        queue.push(plain(ViewEvent::Pointer(pointer(PointerChange::Down, 2.0))));
        queue.push(plain(ViewEvent::Pointer(pointer(PointerChange::Move, 3.0))));
        assert_eq!(drained(&queue).len(), 3);
    }

    #[test]
    fn scrolls_add_up_and_never_merge() {
        let queue = EventQueue::default();
        let mut scroll = pointer(PointerChange::Hover, 1.0);
        scroll.signal = PointerSignalKind::Scroll;
        queue.push(plain(ViewEvent::Pointer(scroll)));
        queue.push(plain(ViewEvent::Pointer(scroll)));
        assert_eq!(drained(&queue).len(), 2);
    }

    #[test]
    fn a_view_has_one_reader() {
        let queue = EventQueue::default();
        assert!(queue.take().is_some());
        assert!(queue.take().is_none());
    }

    /// A program's lifecycle, frozen as the app suspends.
    fn frozen() -> Lifecycle {
        let lifecycle = Lifecycle::default();
        lifecycle.app_suspending();
        assert!(lifecycle.is_frozen());
        lifecycle
    }

    #[test]
    fn a_close_request_unfreezes_the_program_until_it_reads_again() {
        let lifecycle = frozen();
        let queue = EventQueue::default();
        let mut events = queue.take().expect("the stream is taken once");
        queue.push_waking(plain(ViewEvent::CloseRequested), &lifecycle);
        assert!(!lifecycle.is_frozen(), "unfrozen to hear it");
        let (event, unfreeze) = block_on(events.next()).expect("the request");
        assert!(matches!(
            event,
            QueuedEvent::Plain(ViewEvent::CloseRequested)
        ));
        assert!(!lifecycle.is_frozen(), "unfrozen while it handles it");
        drop(unfreeze);
        assert!(lifecycle.is_frozen(), "handled, the rule applies again");
    }

    #[test]
    fn the_views_going_unfreezes_the_program_until_it_drops_the_stream() {
        let lifecycle = frozen();
        let queue = EventQueue::default();
        let mut events = queue.take().expect("the stream is taken once");
        queue.end(&lifecycle);
        assert!(!lifecycle.is_frozen(), "unfrozen to hear it");
        assert!(block_on(events.next()).is_none(), "the end");
        assert!(
            !lifecycle.is_frozen(),
            "unfrozen while it decides what the end means"
        );
        drop(events);
        assert!(lifecycle.is_frozen());
    }

    #[test]
    fn what_nobody_reads_unfreezes_nothing() {
        let lifecycle = frozen();
        let queue = EventQueue::default();
        queue.push_waking(plain(ViewEvent::CloseRequested), &lifecycle);
        queue.end(&lifecycle);
        assert!(lifecycle.is_frozen(), "the stream was never taken");
    }

    #[test]
    fn nothing_is_queued_once_the_view_is_gone() {
        let queue = EventQueue::default();
        queue.end(&Lifecycle::default());
        queue.push(plain(ViewEvent::CloseRequested));
        assert!(drained(&queue).is_empty());
    }
}
