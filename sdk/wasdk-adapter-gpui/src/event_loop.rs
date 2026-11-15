//! The program's one loop: every event of every stream into GPUI, and GPUI's queued work between
//! them.
//!
//! Each turn runs the work that is due for a slice of time, then takes the next event, or wakes
//! for the earliest timer. While work is left after its slice, the loop yields instead of
//! waiting, so events keep arriving between pieces of long work.
//!
//! The invariant: a view is read from (its events stream added) before it is handed to its
//! window, and each event is delivered with no borrow of the platform held, since delivering
//! re-enters GPUI, which calls the platform.

use std::future::{self, Future};
use std::ops::ControlFlow;
use std::pin::{Pin, pin};
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::future::{Either, select};
use wasdk::lifecycle::{AppExitResponse, ExitRequest};
use wasdk::monotonic_clock;
use wasdk::system::SystemChange;
use wasdk::views::{ViewChange, ViewEvent};

use crate::events::{Event, Events, Notice};
use crate::platform::WasdkPlatform;

/// How long queued work runs before the loop looks at its events again.
const WORK_SLICE: Duration = Duration::from_millis(8);

pub(crate) struct EventLoop {
    platform: Rc<WasdkPlatform>,
    events: Events,
    alarm: Alarm,
}

enum Turn {
    Event(Event),
    /// The earliest timer is due, or work is left: run it.
    Work,
    Ended,
}

impl EventLoop {
    pub(crate) fn new(platform: Rc<WasdkPlatform>, events: Events) -> EventLoop {
        EventLoop {
            platform,
            events,
            alarm: Alarm::default(),
        }
    }

    /// Turns until the app quits.
    pub(crate) async fn run(&mut self) {
        loop {
            let work_left = self.run_due_work();
            self.alarm.set(self.platform.dispatcher.next_deadline());
            let carry_on = match self.next_turn(work_left).await {
                Turn::Event(event) => self.deliver(event),
                Turn::Work => ControlFlow::Continue(()),
                Turn::Ended => ControlFlow::Break(()),
            };
            if carry_on.is_break() {
                return;
            }
        }
    }

    /// Runs GPUI's due work for one slice; whether any is left.
    fn run_due_work(&self) -> bool {
        let started = Instant::now();
        while started.elapsed() < WORK_SLICE {
            match self.platform.dispatcher.next_work(Instant::now()) {
                Some(runnable) => {
                    runnable.run();
                }
                None => return false,
            }
        }
        self.platform.dispatcher.has_ready_work()
    }

    async fn next_turn(&mut self, work_left: bool) -> Turn {
        if work_left {
            wasdk::yield_async().await;
        }
        let events = pin!(self.events.next());
        let waiting = pin!(if work_left {
            Either::Left(future::ready(()))
        } else {
            Either::Right(self.alarm.ring())
        });
        match select(events, waiting).await {
            Either::Left((Some(event), _)) => Turn::Event(event),
            Either::Left((None, _)) => Turn::Ended,
            Either::Right(((), _)) => Turn::Work,
        }
    }

    fn deliver(&mut self, event: Event) -> ControlFlow<()> {
        match event {
            Event::View(id, event) => self.view_event(id, event),
            Event::ViewChange(change) => self.view_change(change),
            Event::ScreensChanged => self.platform.displays_changed(),
            Event::SystemChange(SystemChange::PlatformBrightness(brightness)) => {
                self.platform.set_brightness(brightness);
                for window in self.platform.shown_windows() {
                    window.appearance_changed();
                }
            }
            Event::SystemChange(_) => {}
            Event::ExitRequest(request) => return self.exit_requested(request),
            Event::Notice(Notice::Wake) => self.platform.dispatcher.take_wake(),
            Event::Notice(Notice::Realtime(function)) => function(),
            Event::Notice(Notice::Quit) => {
                self.platform.quitting();
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }

    fn view_event(&self, id: u64, event: ViewEvent) {
        let Some(window) = self.platform.shown_window(id) else {
            return;
        };
        match &event {
            ViewEvent::Pointer(_) => self.platform.pointer_moved(),
            ViewEvent::Focus(focus) if focus.state == wasdk::views::ViewFocusState::Focused => {
                self.platform.set_active_window(Some(window.handle))
            }
            _ => {}
        }
        window.view_event(event);
    }

    fn view_change(&mut self, change: ViewChange) {
        match change {
            ViewChange::Added(view) => {
                let Some(window) = view
                    .request_id()
                    .and_then(|request| self.platform.take_pending_window(request))
                else {
                    log::warn!("a view WaOS gave matches no window GPUI opened");
                    return;
                };
                let id = view.id();
                self.events.add_view(&view);
                match window.attach(view) {
                    Ok(()) => self.platform.window_shown(id, window),
                    Err(error) => log::error!("the window's view could not be drawn on: {error:#}"),
                }
            }
            ViewChange::Removed(id) => {
                if let Some(window) = self.platform.take_shown_window(id) {
                    window.removed();
                }
            }
            ViewChange::RequestFailed(failure) => {
                self.platform.take_pending_window(failure.request);
                log::error!("WaOS gave no window: {:?}", failure.error);
            }
        }
    }

    /// The platform asks whether the program may end: it may, once GPUI has shut down.
    fn exit_requested(&self, request: ExitRequest) -> ControlFlow<()> {
        self.platform.quitting();
        ExitRequest::respond(request, AppExitResponse::Exit);
        ControlFlow::Break(())
    }
}

/// The earliest timer as one wait on the program's monotonic clock, made again when it moves.
#[derive(Default)]
struct Alarm {
    deadline: Option<Instant>,
    waiting: Option<Pin<Box<dyn Future<Output = ()>>>>,
}

impl Alarm {
    fn set(&mut self, deadline: Option<Instant>) {
        if deadline == self.deadline {
            return;
        }
        self.deadline = deadline;
        self.waiting = deadline.map(|at| {
            let wait = at.saturating_duration_since(Instant::now());
            let nanoseconds = u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX);
            Box::pin(monotonic_clock::wait_for(nanoseconds)) as Pin<Box<_>>
        });
    }

    /// Resolves when the deadline comes, never while none is set; dropped before, it keeps its
    /// place.
    async fn ring(&mut self) {
        let Some(waiting) = self.waiting.as_mut() else {
            return future::pending().await;
        };
        waiting.await;
        self.deadline = None;
        self.waiting = None;
    }
}
