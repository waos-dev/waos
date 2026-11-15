//! A program's live trace: each use told, sent as an event of `wasdk:inspect`'s `trace` to each
//! listener of its capability ([`TraceKind`]), and to nobody otherwise ([`Taps`]).
//!
//! While nobody listens to a capability a use of it costs one relaxed load of the taps' word, a bit
//! per capability, and no event is made. A listener's events go into a bounded channel without
//! waiting: one that falls behind loses events, and the next one it is sent says how many, so a
//! program is never slowed by a slow listener, only seen less fully. Events flow from when a
//! listener subscribes; the last listener of a capability clears its bit as it drops its stream.
//!
//! The invariant: an event is made only inside [`Taps::send`]'s closure, after the bit is read set,
//! so nothing of the trace is paid for while nobody listens.

use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::Stream;
use futures::channel::mpsc;
use serde::Serialize;

use crate::inspection::bindings::wasdk::inspect::trace::{
    ClipboardEvent, DeviceEvent, FileEvent, LinkEvent, MachineEvent, NetworkEvent,
    NotificationEvent, OutputChunk, PermissionEvent,
};
use crate::inspection::bindings::wasi::clocks::system_clock::Instant;
use crate::sync::locked;

/// How many events wait for a listener before the next is dropped.
const WAITING: usize = 4096;

/// One capability's stream of events, as a listener subscribes to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceKind {
    Network,
    Files,
    Clipboard,
    Camera,
    Microphone,
    Machines,
    Permissions,
    Links,
    Notifications,
    Output,
}

/// One event of a program's trace, of the capability its case names.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum TraceEvent {
    Network(NetworkEvent),
    Files(FileEvent),
    Clipboard(ClipboardEvent),
    Camera(DeviceEvent),
    Microphone(DeviceEvent),
    Machines(MachineEvent),
    Permissions(PermissionEvent),
    Links(LinkEvent),
    Notifications(NotificationEvent),
    Output(OutputChunk),
}

/// Who listens to a program's trace, and to which capabilities.
#[derive(Default)]
pub(super) struct Taps {
    /// A bit per capability listened to.
    listening: AtomicU32,
    listeners: Mutex<Listeners>,
    /// The next request's id.
    requests: AtomicU64,
}

#[derive(Default)]
struct Listeners {
    next: u64,
    each: Vec<Listener>,
}

struct Listener {
    id: u64,
    kind: TraceKind,
    events: mpsc::Sender<TraceEvent>,
    /// The events dropped since the last one this listener was sent.
    dropped: u32,
}

/// One listener's events of one capability of a program, from when it subscribed, until the
/// program and every use it held have ended; dropped, it no longer listens.
pub struct TraceStream {
    events: mpsc::Receiver<TraceEvent>,
    /// The taps it listens at, held without keeping them: they go with the program's access,
    /// and the stream ends then.
    taps: Weak<Taps>,
    id: u64,
}

impl TraceKind {
    fn bit(self) -> u32 {
        1 << self as u32
    }
}

impl Taps {
    /// Whether anyone listens to `kind`.
    pub(super) fn listens(&self, kind: TraceKind) -> bool {
        self.listening.load(Ordering::Relaxed) & kind.bit() != 0
    }

    /// Sends what `make` makes to each listener of `kind`, made only where there is one.
    pub(super) fn send(&self, kind: TraceKind, make: impl FnOnce() -> TraceEvent) {
        if !self.listens(kind) {
            return;
        }
        let event = make();
        let mut listeners = locked(&self.listeners);
        let mut gone = false;
        for listener in listeners
            .each
            .iter_mut()
            .filter(|listener| listener.kind == kind)
        {
            let mut sent = event.clone();
            sent.set_dropped_before(listener.dropped);
            match listener.events.try_send(sent) {
                Ok(()) => listener.dropped = 0,
                Err(error) if error.is_full() => listener.dropped += 1,
                Err(_) => gone = true,
            }
        }
        if gone {
            listeners
                .each
                .retain(|listener| !listener.events.is_closed());
            self.listen_to(&listeners);
        }
    }

    /// A new listener of `kind`, its events flowing from now on.
    pub(super) fn subscribe(taps: &Arc<Taps>, kind: TraceKind) -> TraceStream {
        let (events, received) = mpsc::channel(WAITING);
        let mut listeners = locked(&taps.listeners);
        let id = listeners.next;
        listeners.next += 1;
        listeners.each.push(Listener {
            id,
            kind,
            events,
            dropped: 0,
        });
        taps.listen_to(&listeners);
        TraceStream {
            events: received,
            taps: Arc::downgrade(taps),
            id,
        }
    }

    /// The next of the program's requests' ids.
    pub(super) fn next_request(&self) -> u64 {
        self.requests.fetch_add(1, Ordering::Relaxed)
    }

    /// Sets the bits of the capabilities `listeners` listen to, and clears the others'.
    fn listen_to(&self, listeners: &Listeners) {
        let bits = listeners
            .each
            .iter()
            .fold(0, |bits, listener| bits | listener.kind.bit());
        self.listening.store(bits, Ordering::Relaxed);
    }
}

impl Stream for TraceStream {
    type Item = TraceEvent;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<TraceEvent>> {
        Pin::new(&mut self.events).poll_next(context)
    }
}

impl Drop for TraceStream {
    /// No longer listens: the last listener of its capability clears the capability's bit.
    fn drop(&mut self) {
        let Some(taps) = self.taps.upgrade() else {
            return;
        };
        let mut listeners = locked(&taps.listeners);
        listeners.each.retain(|listener| listener.id != self.id);
        taps.listen_to(&listeners);
    }
}

impl TraceEvent {
    fn set_dropped_before(&mut self, dropped: u32) {
        match self {
            TraceEvent::Network(event) => event.dropped_before = dropped,
            TraceEvent::Files(event) => event.dropped_before = dropped,
            TraceEvent::Clipboard(event) => event.dropped_before = dropped,
            TraceEvent::Camera(event) | TraceEvent::Microphone(event) => {
                event.dropped_before = dropped;
            }
            TraceEvent::Machines(event) => event.dropped_before = dropped,
            TraceEvent::Permissions(event) => event.dropped_before = dropped,
            TraceEvent::Links(event) => event.dropped_before = dropped,
            TraceEvent::Notifications(event) => event.dropped_before = dropped,
            TraceEvent::Output(event) => event.dropped_before = dropped,
        }
    }
}

/// The time of day `at` is, as `wasi:clocks` gives it.
pub(crate) fn instant_of(at: SystemTime) -> Instant {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    Instant {
        seconds: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        nanoseconds: since.subsec_nanos(),
    }
}

/// `length` as `wasi:clocks` gives a duration: in nanoseconds.
pub(crate) fn duration_of(length: Duration) -> u64 {
    u64::try_from(length.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use futures::{FutureExt, StreamExt};

    use super::*;
    use crate::inspection::bindings::wasdk::inspect::trace::{NotificationEvent, Verdict};

    fn posted(instance: u32) -> TraceEvent {
        TraceEvent::Notifications(NotificationEvent {
            at: instant_of(SystemTime::now()),
            instance,
            dropped_before: 0,
            verdict: Verdict::Allowed,
        })
    }

    fn dropped_before(event: &TraceEvent) -> u32 {
        match event {
            TraceEvent::Notifications(event) => event.dropped_before,
            _ => panic!("a notification's event"),
        }
    }

    #[test]
    fn nothing_is_made_while_nobody_listens() {
        let taps = Arc::new(Taps::default());
        taps.send(TraceKind::Notifications, || {
            panic!("made with nobody listening")
        });
        let stream = Taps::subscribe(&taps, TraceKind::Files);
        taps.send(TraceKind::Notifications, || {
            panic!("made for another capability's listener")
        });
        drop(stream);
        assert!(
            !taps.listens(TraceKind::Files),
            "the last listener clears its bit"
        );
    }

    #[test]
    fn a_listener_hears_from_when_it_subscribed_until_it_drops_its_stream() {
        let taps = Arc::new(Taps::default());
        taps.send(TraceKind::Notifications, || posted(0));
        let mut stream = Taps::subscribe(&taps, TraceKind::Notifications);
        taps.send(TraceKind::Notifications, || posted(2));
        let heard = futures::executor::block_on(stream.next()).unwrap();
        assert!(matches!(
            heard,
            TraceEvent::Notifications(NotificationEvent { instance: 2, .. })
        ));
        assert!(
            stream.next().now_or_never().is_none(),
            "nothing from before"
        );
    }

    #[test]
    fn a_listener_that_falls_behind_loses_events_and_the_next_says_how_many() {
        let taps = Arc::new(Taps::default());
        let mut stream = Taps::subscribe(&taps, TraceKind::Notifications);
        for _ in 0..WAITING + 10 {
            taps.send(TraceKind::Notifications, || posted(0));
        }
        let mut read = 0;
        while let Some(Some(event)) = stream.next().now_or_never() {
            assert_eq!(dropped_before(&event), 0);
            read += 1;
        }
        assert!(
            read <= WAITING + 1,
            "the channel holds what waits, and no more: {read}"
        );
        taps.send(TraceKind::Notifications, || posted(0));
        let next = stream.next().now_or_never().flatten().unwrap();
        assert_eq!(dropped_before(&next) as usize, WAITING + 10 - read);
    }

    #[test]
    fn a_stream_ends_as_the_programs_taps_go() {
        let taps = Arc::new(Taps::default());
        let mut stream = Taps::subscribe(&taps, TraceKind::Notifications);
        drop(taps);
        assert!(futures::executor::block_on(stream.next()).is_none());
    }
}
