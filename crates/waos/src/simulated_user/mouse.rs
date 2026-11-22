//! The simulated user's mouse, which its taps, presses, drags and scrolls move over a program's
//! views as a user's mouse moves: in one of the program's views at a time,
//! where its last input left it, the program told of it as it comes into a view and as it leaves
//! one. It is a pointer of its own, [`DEVICE`], apart from the mouse the shell reports, so the two
//! never interleave in what the program hears; its events are a mouse's as the shell reports them,
//! as Waft's host reports Flutter's macOS embedder's: of the kind `mouse`, a pressure of 1 with 1
//! as both bounds, no contact, stamped on the program's clock, with no modifier key held. A press
//! and a drag take the time they say: the program hears the button go down, then nothing for the
//! hold, then each of a drag's moves as the time between them comes, then the button come up.
//!
//! The invariant a later editor will break: the program hears of the mouse in a view only between
//! an `add` there and a `remove`, and it is removed from one view before it is added to another;
//! and while its button is down, it moves only with a `move`, never a `hover`, until the `up`.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::bindings::wasdk::platform::views::{
    PointerButtons, PointerChange, PointerDeviceKind, PointerEvent, PointerSignalKind, ViewId,
};
use crate::views::{HeldModifiers, ProgramView, ProgramViews};

/// The mouse's device: a number no host gives a pointer, the mouse a window reports being 0, as
/// Flutter's `kMousePointerDeviceId` is.
pub(super) const DEVICE: i64 = 1 << 40;

/// Where the mouse is: in no view yet, or in one, at a point.
#[derive(Default)]
pub(super) struct Mouse(Mutex<Option<MouseAt>>);

/// A point in a view, in its physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MouseAt {
    view: ViewId,
    x: f64,
    y: f64,
}

/// One pointer event of the mouse's, and the view it goes to.
#[derive(Debug, PartialEq)]
struct Sent {
    view: ViewId,
    change: PointerChange,
    buttons: PointerButtons,
    at: (f64, f64),
    signal: PointerSignalKind,
    scroll: (f64, f64),
}

impl Mouse {
    /// A press and release of the primary button at `(x, y)` in `view`, one of `views`
    /// windows, the mouse coming there first.
    pub(super) fn tap(&self, views: &ProgramViews, view: &ProgramView, (x, y): (f64, f64)) {
        let to = MouseAt {
            view: view.id(),
            x,
            y,
        };
        let mut sent = self.come_to(to);
        sent.extend(press_and_release(to));
        deliver(views, view, &sent);
    }

    /// A wheel's scroll by `delta` at `at` in `view`, one of `views` windows, the mouse coming
    /// there first.
    pub(super) fn scroll(
        &self,
        views: &ProgramViews,
        view: &ProgramView,
        (x, y): (f64, f64),
        delta: (f64, f64),
    ) {
        let to = MouseAt {
            view: view.id(),
            x,
            y,
        };
        let mut sent = self.come_to(to);
        sent.push(scrolled(to, delta));
        deliver(views, view, &sent);
    }

    /// A press of the primary button at `(x, y)` in `view`, one of `views` windows, the mouse
    /// coming there first, held for `duration`, then released.
    pub(super) async fn press(
        &self,
        views: &ProgramViews,
        view: &ProgramView,
        (x, y): (f64, f64),
        duration: Duration,
    ) {
        let at = MouseAt {
            view: view.id(),
            x,
            y,
        };
        let mut sent = self.come_to(at);
        sent.push(pressed(at));
        deliver(views, view, &sent);
        tokio::time::sleep(duration).await;
        deliver(views, view, &[released(at)]);
    }

    /// A drag of the primary button along `path` in `view`, one of `views` windows: the mouse
    /// coming to its start, pressed there, held, moved along it as its time comes, and released
    /// at its end.
    pub(super) async fn drag(&self, views: &ProgramViews, view: &ProgramView, path: &DragPath) {
        let start = path.start(view.id());
        let mut sent = self.come_to(start);
        sent.push(pressed(start));
        deliver(views, view, &sent);
        tokio::time::sleep(path.hold).await;
        for point in path.moves(view.id()) {
            tokio::time::sleep(path.interval()).await;
            self.held_at(point);
            deliver(views, view, &[dragged(point)]);
        }
        deliver(views, view, &[released(path.end(view.id()))]);
    }

    /// Notes that the mouse, its button held, is now at `at`.
    fn held_at(&self, at: MouseAt) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(at);
    }

    /// The events of the mouse coming to `to`, from wherever it is, where it is from then on.
    fn come_to(&self, to: MouseAt) -> Vec<Sent> {
        let mut at = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let from = at.replace(to);
        moves(from, to)
    }
}

/// A drag's path in a view: from where the button goes down to where it comes up, in `steps`
/// equal moves over `duration`, after the button was held for `hold` at the start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragPath {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub hold: Duration,
    pub steps: u32,
    pub duration: Duration,
}

impl DragPath {
    fn start(&self, view: ViewId) -> MouseAt {
        MouseAt {
            view,
            x: self.from.0,
            y: self.from.1,
        }
    }

    fn end(&self, view: ViewId) -> MouseAt {
        MouseAt {
            view,
            x: self.to.0,
            y: self.to.1,
        }
    }

    /// Where each move goes, the last at the end: a step's share of the way further each, as
    /// Playwright's `mouse.move` with `steps` goes. No steps is one move, straight to the end.
    fn moves(&self, view: ViewId) -> impl Iterator<Item = MouseAt> + use<> {
        let DragPath { from, to, .. } = *self;
        let steps = self.steps.max(1);
        (1..=steps).map(move |step| {
            let share = f64::from(step) / f64::from(steps);
            MouseAt {
                view,
                x: from.0 + (to.0 - from.0) * share,
                y: from.1 + (to.1 - from.1) * share,
            }
        })
    }

    /// The time before each move.
    fn interval(&self) -> Duration {
        self.duration / self.steps.max(1)
    }
}

/// The events of the mouse moving from `from` to `to`: a hover in the same view, unless it is
/// there already; or else out of the view it was in, and into the other.
fn moves(from: Option<MouseAt>, to: MouseAt) -> Vec<Sent> {
    match from {
        Some(from) if from == to => Vec::new(),
        Some(from) if from.view == to.view => vec![sent(to, PointerChange::Hover)],
        Some(from) => vec![
            sent(from, PointerChange::Remove),
            sent(to, PointerChange::Add),
            sent(to, PointerChange::Hover),
        ],
        None => vec![sent(to, PointerChange::Add), sent(to, PointerChange::Hover)],
    }
}

fn press_and_release(at: MouseAt) -> [Sent; 2] {
    [pressed(at), released(at)]
}

/// The primary button going down.
fn pressed(at: MouseAt) -> Sent {
    Sent {
        buttons: PointerButtons::PRIMARY,
        ..sent(at, PointerChange::Down)
    }
}

/// The last button coming up.
fn released(at: MouseAt) -> Sent {
    sent(at, PointerChange::Up)
}

/// A move with the primary button held.
fn dragged(at: MouseAt) -> Sent {
    Sent {
        buttons: PointerButtons::PRIMARY,
        ..sent(at, PointerChange::Move)
    }
}

/// A scroll is a signal at the pointer, reported as the hover it interrupts.
fn scrolled(at: MouseAt, delta: (f64, f64)) -> Sent {
    Sent {
        signal: PointerSignalKind::Scroll,
        scroll: delta,
        ..sent(at, PointerChange::Hover)
    }
}

fn sent(at: MouseAt, change: PointerChange) -> Sent {
    Sent {
        view: at.view,
        change,
        buttons: PointerButtons::empty(),
        at: (at.x, at.y),
        signal: PointerSignalKind::None,
        scroll: (0.0, 0.0),
    }
}

/// Hands each of `sent` to its view: `view`, or the one of `views` it left, which nothing
/// reaches once it is gone.
fn deliver(views: &ProgramViews, view: &ProgramView, sent: &[Sent]) {
    let held = HeldModifiers::default();
    for sent in sent {
        let target = if sent.view == view.id() {
            Some(view.clone())
        } else {
            views
                .views()
                .into_iter()
                .find(|left| left.id() == sent.view)
        };
        if let Some(target) = target {
            target.pointer(pointer_event(sent, target.now()), &held);
        }
    }
}

/// `sent` as the program reads it, stamped `now`.
fn pointer_event(sent: &Sent, now: u64) -> PointerEvent {
    PointerEvent {
        time: now,
        change: sent.change,
        kind: PointerDeviceKind::Mouse,
        signal: sent.signal,
        device: DEVICE,
        x: sent.at.0,
        y: sent.at.1,
        buttons: sent.buttons,
        pressure: 1.0,
        pressure_min: 1.0,
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
        scroll_delta_x: sent.scroll.0,
        scroll_delta_y: sent.scroll.1,
        pan_x: 0.0,
        pan_y: 0.0,
        scale: 1.0,
        rotation: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(view: ViewId, x: f64, y: f64) -> MouseAt {
        MouseAt { view, x, y }
    }

    fn changes(sent: &[Sent]) -> Vec<(ViewId, PointerChange)> {
        sent.iter().map(|sent| (sent.view, sent.change)).collect()
    }

    #[test]
    fn the_mouse_comes_into_its_first_view_before_it_moves_there() {
        let mouse = Mouse::default();
        let sent = mouse.come_to(at(1, 10.0, 20.0));
        assert_eq!(
            changes(&sent),
            [(1, PointerChange::Add), (1, PointerChange::Hover)]
        );
        assert_eq!(sent[1].at, (10.0, 20.0));
    }

    #[test]
    fn the_mouse_hovers_to_another_point_of_its_view_and_stays_where_it_is() {
        let mouse = Mouse::default();
        mouse.come_to(at(1, 10.0, 20.0));
        let sent = mouse.come_to(at(1, 30.0, 40.0));
        assert_eq!(changes(&sent), [(1, PointerChange::Hover)]);
        assert_eq!(sent[0].at, (30.0, 40.0));
        assert!(mouse.come_to(at(1, 30.0, 40.0)).is_empty(), "there already");
    }

    #[test]
    fn the_mouse_leaves_one_view_where_it_was_before_it_comes_into_another() {
        let mouse = Mouse::default();
        mouse.come_to(at(1, 10.0, 20.0));
        let sent = mouse.come_to(at(2, 5.0, 6.0));
        assert_eq!(
            changes(&sent),
            [
                (1, PointerChange::Remove),
                (2, PointerChange::Add),
                (2, PointerChange::Hover)
            ]
        );
        assert_eq!(sent[0].at, (10.0, 20.0), "where it left");
    }

    #[test]
    fn a_tap_is_the_primary_button_down_and_up() {
        let [down, up] = press_and_release(at(1, 3.0, 4.0));
        assert_eq!(
            (down.change, down.buttons),
            (PointerChange::Down, PointerButtons::PRIMARY)
        );
        assert_eq!(
            (up.change, up.buttons),
            (PointerChange::Up, PointerButtons::empty())
        );
    }

    fn path(steps: u32, duration_ms: u64) -> DragPath {
        DragPath {
            from: (10.0, 20.0),
            to: (110.0, -20.0),
            hold: Duration::ZERO,
            steps,
            duration: Duration::from_millis(duration_ms),
        }
    }

    #[test]
    fn a_drag_moves_in_equal_steps_to_its_end() {
        let moves: Vec<_> = path(4, 300).moves(1).map(|at| (at.x, at.y)).collect();
        assert_eq!(
            moves,
            [(35.0, 10.0), (60.0, 0.0), (85.0, -10.0), (110.0, -20.0)]
        );
        assert_eq!(path(4, 300).interval(), Duration::from_millis(75));
    }

    #[test]
    fn a_drag_of_no_steps_moves_once_straight_to_its_end() {
        let moves: Vec<_> = path(0, 300).moves(1).map(|at| (at.x, at.y)).collect();
        assert_eq!(moves, [(110.0, -20.0)]);
        assert_eq!(path(0, 300).interval(), Duration::from_millis(300));
    }

    #[test]
    fn a_drags_move_holds_the_primary_button() {
        let moved = dragged(at(1, 3.0, 4.0));
        assert_eq!(
            (moved.change, moved.buttons),
            (PointerChange::Move, PointerButtons::PRIMARY)
        );
    }

    #[test]
    fn a_scroll_is_a_signal_on_a_hover_with_its_delta() {
        let scroll = scrolled(at(1, 3.0, 4.0), (0.0, -120.0));
        let event = pointer_event(&scroll, 9);
        assert_eq!(event.change, PointerChange::Hover);
        assert_eq!(event.signal, PointerSignalKind::Scroll);
        assert_eq!((event.scroll_delta_x, event.scroll_delta_y), (0.0, -120.0));
        assert_eq!(event.time, 9);
    }

    #[test]
    fn its_events_are_a_mouses_of_its_own() {
        let event = pointer_event(&sent(at(1, 3.0, 4.0), PointerChange::Hover), 0);
        assert_eq!(event.kind, PointerDeviceKind::Mouse);
        assert_eq!(event.device, DEVICE);
        assert_ne!(event.device, 0, "not the window's mouse");
        assert_eq!(
            (event.pressure, event.pressure_min, event.pressure_max),
            (1.0, 1.0, 1.0)
        );
    }
}
