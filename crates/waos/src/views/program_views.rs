//! One program's views: the ones it has now, in the order they arrived, what changes among them,
//! as the shell adds and removes them and the program asks for more, what each request the
//! program made attached, kept for the view that arrives for it, and which of them a screen's
//! refresh serves as it comes.
//!
//! Ids are minted here, a view's and a request's, and never reused within a program; and here is
//! the program's one record of the modifier keys it was told of, which every view it has shares,
//! as a program has one keyboard for all its views. The invariant: a view is added to the list
//! and announced under one lock, and removed and announced under the same, so what `views()`
//! answers and what `changes()` delivers are one history; the program's lifecycle state, and
//! whether one of its views has the input focus, follow every view added, every change of a
//! view's presence, every tool's watch begun or ended, and every view removed, so no other code
//! sets them; and a refresh that comes hands one frame to each view it serves, the same frame,
//! naming exactly those views, as Android's `Choreographer` serves every window of its thread at
//! one vsync. A view that asks within a refresh it had no frame of is served by itself
//! (`program_view`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use futures::channel::mpsc;
use tokio::sync::watch;

use crate::bindings::wasdk::platform::screens::ScreenId;
use crate::bindings::wasdk::platform::views::{RequestFailure, RequestId, ViewError, ViewId};
use crate::channel::Channel;
use crate::clipboard::PlatformClipboard;
use crate::clock::Clock;
use crate::lifecycle::{FreezeInhibitorKind, Lifecycle};
use crate::news::MaybeInhibiting;
use crate::screenshots::Screenshots;
use crate::shell::PlacedView;
use crate::sync::locked;

use super::modifiers::ToldModifiers;
use super::program_view::{ProgramView, Shared, frame_of};
use super::screen_refreshes::{Refresh, ScreenRefreshes};
use super::view_layers::ViewLayers;

/// A program's views, shared by the shell, which adds and removes them, and the engine, which
/// reads them for the program. Every clone reaches the same ones.
#[derive(Clone)]
pub(crate) struct ProgramViews(Arc<ViewsState>);

pub(super) struct ViewsState {
    clock: Clock,
    /// The modifier keys the program has been told are held, through any of its views.
    told_modifiers: Arc<Mutex<ToldModifiers>>,
    /// How many tools watch the program now, which keeps its views in sight though occluded.
    watchers: Arc<AtomicUsize>,
    /// The screens' refreshes the engine waits for, which every view's frames wait on.
    refreshes: ScreenRefreshes,
    /// The platform's clipboard, which a view's paste is of.
    clipboard: PlatformClipboard,
    /// What draws a view's screenshot.
    screenshots: Arc<Screenshots>,
    lifecycle: Lifecycle,
    /// In the order they arrived.
    views: Mutex<Vec<ProgramView>>,
    next_view_id: AtomicU64,
    next_request_id: AtomicU64,
    /// What each request still unanswered attached, and its window's title, for the view that
    /// arrives for it.
    requests: Mutex<HashMap<RequestId, Requested>>,
    /// What changes among the views, a view's going holding the program unfrozen until it has
    /// handled it on each stream.
    changes: Channel<MaybeInhibiting<ViewChange>>,
    /// Whether one of the views has the input focus.
    focused: watch::Sender<bool>,
    /// The layers the views hold, which the program's store keeps; gone once the program has
    /// ended.
    layers: Weak<ViewLayers>,
}

/// What the program asked a view for with: what its request attached, and the title of the window
/// it asked for.
#[derive(Default)]
pub(super) struct Requested {
    pub(super) initial_data: Option<String>,
    pub(super) title: String,
}

/// A change among a program's views, in the order the changes happened: the WIT's `view-change`
/// with the view itself in place of its handle, which the code serving the interface makes.
#[derive(Clone)]
pub(crate) enum ViewChange {
    /// A view the program now has, arrived for a request.
    Added(ProgramView),
    /// A view the program no longer has. Its events have ended.
    Removed(ViewId),
    /// A request got no view.
    RequestFailed(RequestFailure),
}

/// The first id of a program's views, and of its requests: 0 names none.
const FIRST_ID: u64 = 1;

impl ProgramViews {
    /// The views `state` is the state of.
    pub(super) fn of(state: Arc<ViewsState>) -> ProgramViews {
        ProgramViews(state)
    }

    /// No views yet, for a program whose events are stamped on `clock`, whose state `lifecycle`
    /// holds, whose frames wait on `refreshes`, and whose views' layers are `layers`.
    pub(crate) fn new(
        clock: Clock,
        lifecycle: Lifecycle,
        refreshes: ScreenRefreshes,
        clipboard: PlatformClipboard,
        screenshots: Arc<Screenshots>,
        layers: Weak<ViewLayers>,
    ) -> ProgramViews {
        ProgramViews(Arc::new(ViewsState {
            clock,
            told_modifiers: Arc::default(),
            watchers: Arc::default(),
            refreshes,
            clipboard,
            screenshots,
            lifecycle,
            views: Mutex::default(),
            next_view_id: AtomicU64::new(FIRST_ID),
            next_request_id: AtomicU64::new(FIRST_ID),
            requests: Mutex::default(),
            changes: Channel::default(),
            focused: watch::Sender::new(false),
            layers,
        }))
    }

    /// Whether one of the views has the input focus, now and as it changes: the presence the
    /// shell reports into each.
    pub(crate) fn focus(&self) -> watch::Receiver<bool> {
        self.0.focused.subscribe()
    }

    /// Now on the program's clock, which its views' events are stamped on.
    pub(crate) fn now(&self) -> u64 {
        self.0.clock.now()
    }

    /// The views the program has now, in the order they arrived.
    pub(crate) fn views(&self) -> Vec<ProgramView> {
        locked(&self.0.views).clone()
    }

    /// What changes among the views from now on, on a stream of its own, a view's going with what
    /// holds the program unfrozen until it has handled it, since it decides whether the program
    /// goes on.
    pub(crate) fn changes(&self) -> mpsc::UnboundedReceiver<MaybeInhibiting<ViewChange>> {
        self.0.changes.subscribe()
    }

    /// A view the shell adds for `request`, with a fresh id and what the request attached: the
    /// program has it from now on, hears of it, and its state follows the views it has now.
    pub(crate) fn add(&self, request: RequestId, placed: PlacedView) -> ProgramView {
        let requested = self.requested_with(request).unwrap_or_default();
        let id = self.0.next_view_id.fetch_add(1, Ordering::Relaxed);
        let shared = Shared {
            clock: self.0.clock,
            told_modifiers: Arc::clone(&self.0.told_modifiers),
            watchers: Arc::clone(&self.0.watchers),
            refreshes: self.0.refreshes.clone(),
            clipboard: self.0.clipboard.clone(),
            screenshots: Arc::clone(&self.0.screenshots),
            views: Arc::downgrade(&self.0),
        };
        let view = ProgramView::new(id, request, placed, requested, shared);
        let mut views = locked(&self.0.views);
        views.push(view.clone());
        self.0.changes.send((ViewChange::Added(view.clone()), None));
        self.0.state_follows(&views);
        view
    }

    /// What the request `request` was made with, taken out: the request is answered by the view
    /// that arrives for it. A request the program did not make, or that was answered already, was
    /// made with nothing, and is a shell answering twice.
    fn requested_with(&self, request: RequestId) -> Option<Requested> {
        let requested = locked(&self.0.requests).remove(&request);
        if requested.is_none() {
            tracing::warn!(request, "a view arrived for a request not waiting for one");
        }
        requested
    }

    /// The shell took the view away: its events end once read, the program hears of it, it lets
    /// go of its layers, and the program's state follows the views that remain.
    pub(crate) fn remove(&self, id: ViewId) {
        let Some(removed) = self.take_out(id) else {
            return;
        };
        removed.end(&self.0.lifecycle);
        if let Some(layers) = self.0.layers.upgrade() {
            layers.view_removed(id);
        }
        self.0.state_follows_views();
    }

    /// Takes the view `id` out of the list and announces it, under one lock; `None` for a view
    /// the program does not have.
    fn take_out(&self, id: ViewId) -> Option<ProgramView> {
        let mut views = locked(&self.0.views);
        let index = views.iter().position(|view| view.id() == id)?;
        let removed = views.remove(index);
        let unfreeze = self.0.changes.is_heard().then(|| {
            Arc::new(
                self.0
                    .lifecycle
                    .inhibit_freeze(FreezeInhibitorKind::PendingRequest),
            )
        });
        self.0.changes.send((ViewChange::Removed(id), unfreeze));
        Some(removed)
    }

    /// A tool watches the program until the answer is dropped: its views count as in sight
    /// though occluded, its state follows, and a frame one of them waits for is asked for.
    pub(crate) fn watch(&self) -> Watching {
        self.0.watchers.fetch_add(1, Ordering::SeqCst);
        self.watchers_changed();
        Watching(self.clone())
    }

    /// The program's state follows how its views count now, and each view that waits for a frame
    /// asks for it if it can be served.
    fn watchers_changed(&self) {
        self.0.state_follows_views();
        for view in self.views() {
            view.watchers_changed();
        }
    }

    /// The clock the program's time is on, which its views' frames are stamped on.
    pub(crate) fn clock(&self) -> Clock {
        self.0.clock
    }

    /// A fresh id for a request the program is making, which attaches `initial_data` and titles
    /// its window `title` for the view that arrives.
    pub(crate) fn new_request(&self, initial_data: Option<String>, title: String) -> RequestId {
        let request = self.0.next_request_id.fetch_add(1, Ordering::Relaxed);
        let requested = Requested {
            initial_data,
            title,
        };
        locked(&self.0.requests).insert(request, requested);
        request
    }

    /// `screen` refreshed, as `refresh` says: every view of the program that waits for a frame,
    /// is in sight on that screen and has not had this refresh's frame gets one on its own stream,
    /// stamped with the refresh's times on the program's clock, the frame naming them all. A view
    /// left out that still waits asks for the next refresh, and so does the engine where a frame
    /// was served (`screen_refreshes`).
    pub(crate) fn screen_refreshed(&self, screen: ScreenId, refresh: Refresh) {
        let (served, left_out): (Vec<ProgramView>, Vec<ProgramView>) = self
            .views()
            .into_iter()
            .partition(|view| view.takes_frame_of(screen, refresh));
        for view in &left_out {
            view.ask_refresh_if_waiting();
        }
        if served.is_empty() {
            return;
        }
        let views = served.iter().map(ProgramView::id).collect();
        let frame = frame_of(refresh, &self.0.clock, views);
        for view in &served {
            view.hand_over(frame.clone());
        }
        self.0.refreshes.frame_served(screen);
    }

    /// The shell did not take the request `request`: the program hears so from the call it
    /// made, and nothing more.
    pub(crate) fn request_refused(&self, request: RequestId) {
        locked(&self.0.requests).remove(&request);
    }

    /// The shell could not place the view asked for with `request`: the program hears why.
    pub(crate) fn request_failed(&self, request: RequestId, error: ViewError) {
        locked(&self.0.requests).remove(&request);
        self.0.changes.send((
            ViewChange::RequestFailed(RequestFailure { request, error }),
            None,
        ));
    }
}

impl ViewsState {
    /// The program's lifecycle, which a request to close a view unfreezes.
    pub(super) fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    /// The program's state follows how its views stand now.
    pub(super) fn state_follows_views(&self) {
        // Derived and applied under the views' lock, so reports from two threads reach the
        // lifecycle in the order their states were derived, and a stale one never lands last.
        let views = locked(&self.views);
        self.state_follows(&views);
    }

    /// The program's state, and whether it has the focus, follow how `views`, held under their
    /// lock, stand now.
    fn state_follows(&self, views: &[ProgramView]) {
        self.lifecycle
            .follow_views(views.iter().map(ProgramView::counted_presence));
        let focused = views.iter().any(|view| view.presence().focused);
        self.focused
            .send_if_modified(|was| std::mem::replace(was, focused) != focused);
    }
}

/// A tool's watch of a program, from
/// [`AppInstance::watch`](crate::running::AppInstance::watch) until dropped: while one is
/// held, the program's views count as in sight though the platform says their windows are occluded.
/// Dropping the last puts each view back in sight as the shell reports it.
pub struct Watching(ProgramViews);

impl Drop for Watching {
    fn drop(&mut self) {
        self.0.0.watchers.fetch_sub(1, Ordering::SeqCst);
        self.0.watchers_changed();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use futures::StreamExt;
    use futures::executor::block_on;

    use crate::bindings::wasdk::platform::lifecycle::LifecycleState;
    use crate::bindings::wasdk::platform::views::{KeyKind, Placement, PointerChange, ViewEvent};
    use crate::lifecycle::Presence;
    use crate::screenshots::tests::screenshots;
    use crate::shell::tests::Headless;
    use crate::shell::{HardwareBuffer, ShellCompositionLayer};
    use crate::windows::tests::rect;

    use super::super::QueuedEvent;
    use super::super::event_queue::tests::{Heard, heard, pointer};
    use super::super::modifiers::tests::{SHIFT, key_event, keyboard_holding};
    use super::super::program_view::tests::{ON_SCREEN, placed};
    use super::super::screen_refreshes::tests::{
        refresh_at, refreshes_a_second_apart, refreshes_of,
    };
    use super::*;

    const FOCUSED: Presence = Presence {
        on_screen: true,
        occluded: false,
        focused: true,
    };
    const OFF_SCREEN: Presence = Presence {
        on_screen: false,
        occluded: false,
        focused: false,
    };

    fn program() -> (ProgramViews, Lifecycle) {
        program_holding(Weak::new())
    }

    /// A program whose views' layers are `layers`.
    fn program_holding(layers: Weak<ViewLayers>) -> (ProgramViews, Lifecycle) {
        let lifecycle = Lifecycle::default();
        let views = ProgramViews::new(
            Clock::start(),
            lifecycle.clone(),
            refreshes_of(&Arc::default()),
            PlatformClipboard::new(None),
            screenshots(),
            layers,
        );
        (views, lifecycle)
    }

    /// A program whose views' refreshes are asked of `shell`, and those refreshes.
    fn program_on(shell: &Arc<Headless>) -> (ProgramViews, ScreenRefreshes) {
        let refreshes = refreshes_of(shell);
        let views = ProgramViews::new(
            Clock::start(),
            Lifecycle::default(),
            refreshes.clone(),
            PlatformClipboard::new(None),
            screenshots(),
            Weak::new(),
        );
        (views, refreshes)
    }

    /// A refresh of `screen` comes, as the engine serves it: its request is forgotten, then the
    /// program's views on it are served.
    fn refresh(views: &ProgramViews, refreshes: &ScreenRefreshes, screen: ScreenId, at: Instant) {
        let refresh = Refresh {
            at,
            display_time: at,
        };
        refreshes.came(screen, refresh);
        views.screen_refreshed(screen, refresh);
    }

    /// What `view` was handed, once it is gone: these tests hand it only plain events.
    fn delivered(view: &ProgramView) -> Vec<ViewEvent> {
        let events = view.events().expect("the events are taken once");
        view.end(&Lifecycle::default());
        heard(events)
            .into_iter()
            .map(|event| match event {
                Heard::Plain(event) => event,
                heard => panic!("only plain events were handed, not {heard:?}"),
            })
            .collect()
    }

    /// The views the one frame `view` was handed names.
    fn the_frame_of(view: &ProgramView) -> Vec<ViewId> {
        match &delivered(view)[..] {
            [ViewEvent::Frame(frame)] => frame.views.clone(),
            events => panic!("one frame, not {events:?}"),
        }
    }

    /// A view as the shell places it on screen 1, standing as `presence` says.
    fn on_screen_1(presence: Presence) -> PlacedView {
        PlacedView {
            screen: Some(1),
            presence,
            ..placed(&Arc::default())
        }
    }

    /// `placed` added to `views` for a request the program made for it.
    fn arrived(views: &ProgramViews, placed: PlacedView) -> ProgramView {
        let request = views.new_request(None, String::new());
        views.add(request, placed)
    }

    #[test]
    fn a_view_added_is_listed_and_announced_with_its_id() {
        let (views, _) = program();
        let mut changes = views.changes();
        let view = arrived(&views, on_screen_1(ON_SCREEN));
        assert_eq!(view.id(), FIRST_ID);
        assert_eq!(view.screen(), Some(1));
        assert_eq!(view.request_id(), FIRST_ID);
        assert_eq!(view.initial_data(), None);
        let listed = views.views();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id(), view.id());
        let (ViewChange::Added(heard), _unfreeze) = changes.try_recv().unwrap() else {
            panic!("the view is announced");
        };
        assert_eq!(heard.id(), view.id());
    }

    #[test]
    fn ids_are_never_reused() {
        let (views, _) = program();
        let first = arrived(&views, on_screen_1(ON_SCREEN));
        views.remove(first.id());
        let second = arrived(&views, on_screen_1(ON_SCREEN));
        assert_ne!(second.id(), first.id());
        assert_ne!(
            views.new_request(None, String::new()),
            views.new_request(None, String::new())
        );
    }

    #[test]
    fn a_view_removed_is_announced_and_its_events_end() {
        let (views, _) = program();
        let mut changes = views.changes();
        let view = arrived(&views, on_screen_1(ON_SCREEN));
        let mut events = view.events().unwrap();
        views.remove(view.id());
        assert!(views.views().is_empty());
        assert!(matches!(
            changes.try_recv().unwrap(),
            (ViewChange::Added(_), _)
        ));
        assert!(matches!(
            changes.try_recv().unwrap(),
            (ViewChange::Removed(id), _) if id == view.id()
        ));
        assert!(block_on(events.next()).is_none(), "the events have ended");
    }

    struct Layer;

    impl ShellCompositionLayer for Layer {
        fn present(&self, _buffer: HardwareBuffer, released: Box<dyn FnOnce() + Send + Sync>) {
            released();
        }

        fn current_picture(&self) -> Option<crate::shell::LayerPicture> {
            None
        }
    }

    /// As a program's layers go with its store once it has ended, a view's go with the view.
    #[test]
    fn a_view_taken_away_lets_go_of_its_layers_while_the_program_runs() {
        let store_layers = Arc::new(ViewLayers::default());
        let (views, _) = program_holding(Arc::downgrade(&store_layers));
        let first = arrived(&views, on_screen_1(ON_SCREEN));
        let second = arrived(&views, on_screen_1(ON_SCREEN));
        let layer: Arc<dyn ShellCompositionLayer> = Arc::new(Layer);
        for view in [&first, &second] {
            let placement = Placement {
                rect: rect(0.0),
                mutators: Vec::new(),
            };
            store_layers.set(view.id(), vec![(Arc::clone(&layer), placement)]);
        }
        views.remove(first.id());
        assert!(store_layers.of(first.id()).is_empty());
        assert_eq!(
            Arc::strong_count(&layer),
            2,
            "the view that remains holds the layer, and the view taken away does not"
        );
        views.remove(second.id());
        assert_eq!(Arc::strong_count(&layer), 1, "no view holds it");
    }

    #[test]
    fn a_view_the_program_does_not_have_is_not_removed_twice() {
        let (views, _) = program();
        let mut changes = views.changes();
        views.remove(FIRST_ID);
        assert!(changes.try_recv().is_err(), "nothing to announce");
    }

    #[test]
    fn a_failed_request_is_announced_with_its_id() {
        let (views, _) = program();
        let mut changes = views.changes();
        let request = views.new_request(None, String::new());
        views.request_failed(request, ViewError::Unsupported);
        assert!(matches!(
            changes.try_recv().unwrap(),
            (ViewChange::RequestFailed(RequestFailure { request: failed, error: ViewError::Unsupported }), _)
                if failed == request
        ));
    }

    #[test]
    fn a_view_that_arrives_for_a_request_carries_it_what_it_attached_and_its_windows_title() {
        let (views, _) = program();
        let request = views.new_request(Some("hello".into()), "Notes".into());
        let view = views.add(request, on_screen_1(ON_SCREEN));
        assert_eq!(view.request_id(), request);
        assert_eq!(view.initial_data(), Some("hello"));
        assert_eq!(view.window_title(), None, "a view in no window");
    }

    #[test]
    fn a_request_is_answered_once() {
        let (views, _) = program();
        let request = views.new_request(Some("hello".into()), String::new());
        views.add(request, on_screen_1(ON_SCREEN));
        let again = views.add(request, on_screen_1(ON_SCREEN));
        assert_eq!(
            again.initial_data(),
            None,
            "what it attached was given once"
        );
        assert!(locked(&views.0.requests).is_empty());
    }

    #[test]
    fn a_request_refused_or_failed_attaches_nothing_to_a_later_view() {
        let (views, _) = program();
        let refused = views.new_request(Some("refused".into()), String::new());
        views.request_refused(refused);
        let failed = views.new_request(Some("failed".into()), String::new());
        views.request_failed(failed, ViewError::Unsupported);
        assert!(locked(&views.0.requests).is_empty());
    }

    #[test]
    fn the_state_follows_the_views_presence() {
        let (views, lifecycle) = program();
        let first = arrived(&views, on_screen_1(OFF_SCREEN));
        assert_eq!(
            lifecycle.state(),
            LifecycleState::Hidden,
            "a view arrives with how it stands"
        );
        let second = arrived(&views, on_screen_1(ON_SCREEN));
        assert_eq!(lifecycle.state(), LifecycleState::Inactive);
        first.presence_changed(FOCUSED);
        assert_eq!(lifecycle.state(), LifecycleState::Resumed);
        views.remove(first.id());
        assert_eq!(
            lifecycle.state(),
            LifecycleState::Inactive,
            "the view that remains decides"
        );
        views.remove(second.id());
        assert_eq!(
            lifecycle.state(),
            LifecycleState::Inactive,
            "a program with no view keeps its state"
        );
    }

    #[test]
    fn a_watched_programs_covered_views_count_in_sight_until_the_watch_ends() {
        let (views, lifecycle) = program();
        let covered = Presence {
            occluded: true,
            ..FOCUSED
        };
        let view = arrived(&views, on_screen_1(covered));
        assert_eq!(lifecycle.state(), LifecycleState::Hidden, "covered");

        let watching = views.watch();
        assert!(view.in_sight());
        assert_eq!(view.presence(), covered, "the report is kept as it was");
        assert_eq!(lifecycle.state(), LifecycleState::Resumed, "watched");
        let second = views.watch();
        drop(watching);
        assert_eq!(
            lifecycle.state(),
            LifecycleState::Resumed,
            "watched while any watch is held"
        );

        view.presence_changed(OFF_SCREEN);
        assert!(
            !view.in_sight(),
            "minimized, it is out of sight though watched"
        );
        assert_eq!(lifecycle.state(), LifecycleState::Hidden);
        view.presence_changed(covered);
        assert_eq!(lifecycle.state(), LifecycleState::Resumed);

        drop(second);
        assert!(!view.in_sight());
        assert_eq!(
            lifecycle.state(),
            LifecycleState::Hidden,
            "unwatched, it is covered again"
        );
    }

    #[test]
    fn a_watch_serves_a_covered_view_the_frame_it_waits_for_and_none_after_it_ends() {
        let shell = Arc::new(Headless::default());
        let (views, refreshes) = program_on(&shell);
        let covered = Presence {
            occluded: true,
            ..ON_SCREEN
        };
        let view = arrived(&views, on_screen_1(covered));
        view.request_frame();
        assert!(shell.refreshes_asked().is_empty(), "covered: no refresh");

        let watching = views.watch();
        assert_eq!(shell.refreshes_asked(), [1], "watched: the request asks");
        refresh(&views, &refreshes, 1, Instant::now());
        assert!(!view.wants_frame(), "served");

        drop(watching);
        view.request_frame();
        refresh(&views, &refreshes, 1, Instant::now());
        assert!(view.wants_frame(), "unwatched, covered, its frame waits");
        assert_eq!(shell.refreshes_asked(), [1], "and asks for no refresh");
        assert_eq!(
            the_frame_of(&view),
            [view.id()],
            "the one frame served watched"
        );
    }

    #[test]
    fn whether_a_view_has_the_focus_follows_the_views_presence() {
        let (views, _lifecycle) = program();
        let focus = views.focus();
        let view = arrived(&views, on_screen_1(ON_SCREEN));
        assert!(!*focus.borrow());
        view.presence_changed(FOCUSED);
        assert!(*focus.borrow());
        views.remove(view.id());
        assert!(
            !*focus.borrow(),
            "a focused view that goes takes the focus with it"
        );
    }

    #[test]
    fn a_presence_reported_after_the_program_ended_goes_nowhere() {
        let (views, lifecycle) = program();
        let view = arrived(&views, on_screen_1(ON_SCREEN));
        drop(views);
        view.presence_changed(OFF_SCREEN);
        assert_eq!(lifecycle.state(), LifecycleState::Inactive);
    }

    #[test]
    fn a_modifier_heard_through_one_view_counts_for_every_other() {
        let (views, _) = program();
        let (typed_in, clicked) = (
            arrived(&views, on_screen_1(ON_SCREEN)),
            arrived(&views, on_screen_1(ON_SCREEN)),
        );
        let mut heard = clicked.events().unwrap();
        let _handled = typed_in.key(key_event(KeyKind::Down, SHIFT));
        clicked.pointer(
            pointer(PointerChange::Down, 0.0),
            &keyboard_holding(&[SHIFT]),
        );
        assert!(
            matches!(
                block_on(heard.next()),
                Some((QueuedEvent::Plain(ViewEvent::Pointer(_)), _))
            ),
            "nothing missed"
        );
        clicked.pointer(pointer(PointerChange::Up, 0.0), &keyboard_holding(&[]));
        let Some((QueuedEvent::Key(released, _reply), _)) = block_on(heard.next()) else {
            panic!("the shift, released unheard, comes up first");
        };
        assert_eq!((released.kind, released.synthesized), (KeyKind::Up, true));
    }

    #[test]
    fn a_refresh_hands_each_view_on_its_screen_one_frame_naming_them_all() {
        let shell = Arc::new(Headless::default());
        let (views, refreshes) = program_on(&shell);
        let (first, second) = (
            arrived(&views, on_screen_1(ON_SCREEN)),
            arrived(&views, on_screen_1(ON_SCREEN)),
        );
        let elsewhere = arrived(
            &views,
            PlacedView {
                screen: Some(2),
                ..on_screen_1(ON_SCREEN)
            },
        );
        for view in [&first, &second, &elsewhere] {
            view.request_frame();
        }
        assert_eq!(
            shell.refreshes_asked(),
            [1, 2],
            "one refresh asked per screen"
        );

        let at = Instant::now();
        refresh(&views, &refreshes, 1, at);
        assert!(
            elsewhere.wants_frame(),
            "the view on the other screen waits for that screen's refresh"
        );
        let both = vec![first.id(), second.id()];
        for view in [&first, &second] {
            let events = delivered(view);
            let [ViewEvent::Frame(frame)] = &events[..] else {
                panic!("one frame each, not {events:?}");
            };
            assert_eq!(frame.views, both);
            assert_eq!(frame.time, views.0.clock.at(at));
        }
    }

    #[test]
    fn a_view_that_asked_before_a_refresh_is_served_at_it_and_one_that_asks_in_it_at_once() {
        let shell = Arc::new(Headless::default());
        let refreshes = refreshes_a_second_apart(&shell);
        let views = ProgramViews::new(
            Clock::start(),
            Lifecycle::default(),
            refreshes.clone(),
            PlatformClipboard::new(None),
            screenshots(),
            Weak::new(),
        );
        let (early, late) = (
            arrived(&views, on_screen_1(ON_SCREEN)),
            arrived(&views, on_screen_1(ON_SCREEN)),
        );
        early.request_frame();
        assert!(early.wants_frame(), "no refresh told yet: it waits for one");

        let came = refresh_at(Instant::now());
        refreshes.came(1, came);
        views.screen_refreshed(1, came);
        assert!(!early.wants_frame(), "served at the refresh");
        assert_eq!(
            shell.refreshes_asked(),
            [1, 1],
            "a frame served: the refresh after is asked for"
        );

        late.request_frame();
        assert!(
            !late.wants_frame(),
            "it had no frame of this refresh: at once"
        );
        early.request_frame();
        assert!(early.wants_frame(), "it had this refresh's: the next one's");

        for view in [&early, &late] {
            let events = delivered(view);
            let [ViewEvent::Frame(frame)] = &events[..] else {
                panic!("one frame each, not {events:?}");
            };
            assert_eq!(frame.views, [view.id()], "each its own");
            assert_eq!(frame.time, views.0.clock.at(came.at), "of the one refresh");
        }
    }

    #[test]
    fn a_view_out_of_sight_is_left_out_and_served_once_it_is_seen() {
        let shell = Arc::new(Headless::default());
        let (views, refreshes) = program_on(&shell);
        let (shown, hidden) = (
            arrived(&views, on_screen_1(ON_SCREEN)),
            arrived(&views, on_screen_1(OFF_SCREEN)),
        );
        shown.request_frame();
        hidden.request_frame();
        refresh(&views, &refreshes, 1, Instant::now());
        assert!(hidden.wants_frame(), "out of sight, its frame waits");

        hidden.presence_changed(ON_SCREEN);
        assert_eq!(
            shell.refreshes_asked(),
            [1, 1],
            "seen, it asks for the next refresh"
        );
        refresh(&views, &refreshes, 1, Instant::now());
        assert_eq!(the_frame_of(&shown), [shown.id()]);
        assert_eq!(the_frame_of(&hidden), [hidden.id()]);
    }

    #[test]
    fn a_view_on_no_screen_waits_until_it_is_on_one() {
        let shell = Arc::new(Headless::default());
        let (views, refreshes) = program_on(&shell);
        let view = arrived(
            &views,
            PlacedView {
                screen: None,
                ..on_screen_1(ON_SCREEN)
            },
        );
        view.request_frame();
        refresh(&views, &refreshes, 1, Instant::now());
        assert!(view.wants_frame(), "no screen's refresh serves it");
        assert!(shell.refreshes_asked().is_empty());

        view.screen_changed(Some(1));
        assert_eq!(shell.refreshes_asked(), [1]);
        refresh(&views, &refreshes, 1, Instant::now());
        assert_eq!(the_frame_of(&view), [view.id()]);
    }

    #[test]
    fn a_view_with_two_frames_in_flight_is_left_out_until_one_lands_while_another_is_served() {
        let shell = Arc::new(Headless::default());
        let (views, refreshes) = program_on(&shell);
        let (slow, other) = (
            arrived(&views, on_screen_1(ON_SCREEN)),
            arrived(&views, on_screen_1(ON_SCREEN)),
        );
        let first = slow.frame_in_flight();
        let _second = slow.frame_in_flight();
        slow.request_frame();
        other.request_frame();
        refresh(&views, &refreshes, 1, Instant::now());
        assert!(slow.wants_frame(), "two in flight: it is left out");
        assert_eq!(
            shell.refreshes_asked(),
            [1, 1],
            "and asks for the next refresh"
        );
        other.request_frame();
        refresh(&views, &refreshes, 1, Instant::now());
        assert!(slow.wants_frame(), "still two in flight");

        drop(first);
        refresh(&views, &refreshes, 1, Instant::now());
        assert!(!slow.wants_frame(), "one landed: it is served");
        let served: Vec<Vec<ViewId>> = [&slow, &other]
            .map(|view| {
                delivered(view)
                    .into_iter()
                    .map(|event| match event {
                        ViewEvent::Frame(frame) => frame.views,
                        event => panic!("only frames, not {event:?}"),
                    })
                    .collect::<Vec<_>>()
            })
            .concat();
        assert_eq!(
            served,
            [vec![slow.id()], vec![other.id()], vec![other.id()]],
            "the slow view's one frame, once one landed; the other's at each refresh it asked"
        );
    }

    /// The shell adds from its threads and the program's thread reads.
    #[test]
    fn a_programs_views_cross_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<ProgramViews>();
        assert_shared::<ViewChange>();
    }
}
