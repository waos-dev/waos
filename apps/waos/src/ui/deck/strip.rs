//! Swiping the bar sideways: the strip of pages follows the finger one to one, resists before the
//! first page and after the last as a scroll view does, and on release settles on a page with the
//! finger's velocity; a flick moves on to the next page however little the strip moved. Past the
//! last tab, while no home is open, is the home's page: settling on it opens the home there.
//!
//! The gesture's states are the strip's grip. Free: the strip settles on the current tab's page.
//! Held: a finger came down on the bar and the strip stands still where it was, moving or not; a
//! finger lifted without dragging, or whose drag went up into the overview, lets the strip settle
//! again. Dragged: the strip follows the finger; the release decides the page, tells the tabs,
//! and the strip settles on it from where it is at the finger's speed; a drag let go having hardly
//! moved is a held finger lifted. A grip dropped, because the
//! deck was sent elsewhere or its view changed size, leaves the finger still down with nothing.
//!
//! The invariant: the strip's position is only ever set by the finger that drags it or by the
//! spring that settles it, each from where the other left it, so the strip never jumps.

use std::rc::Rc;

use waft::{App, Handle};

use super::axis_drag::AxisDrag;
use super::grip::{Owner, Released};
use super::overscroll::{DragRange, scrolled_by_finger};
use super::{DeckViewState, StripFollows};
use crate::logic::tab_list::TabId;
use crate::state::deck::tabs::Tabs;

/// The finger speed, in points a second, above which a release moves on to the next page in its
/// direction however little the strip has moved.
const FLICK_VELOCITY: f64 = 300.0;

impl DeckViewState {
    /// The bar's sideways drag.
    pub(super) fn strip_drag(self: Handle<Self>, owner: Owner) -> AxisDrag {
        AxisDrag {
            on_down: Rc::new(move |app| self.hold_strip(app, owner)),
            on_start: Rc::new(move |app, _| self.start_strip_drag(app, owner)),
            on_update: Rc::new(move |app, moved| self.drag_strip(app, owner, moved.along)),
            on_end: Rc::new(move |app, velocity| self.release_strip(app, owner, velocity)),
            on_cancel: Rc::new(move |app| self.cancel_strip_hold(app, owner)),
        }
    }

    fn hold_strip(self: Handle<Self>, app: &mut App, owner: Owner) {
        if app.get_mut(self).strip_grip.hold(owner) {
            self.stop_strip(app);
        }
    }

    fn start_strip_drag(self: Handle<Self>, app: &mut App, owner: Owner) {
        if app.get_mut(self).strip_grip.drag(owner) {
            self.stop_strip(app);
            self.leave_search(app);
        }
    }

    fn drag_strip(self: Handle<Self>, app: &mut App, owner: Owner, finger_delta: f64) {
        if !app.get_mut(self).strip_grip.moved(owner, finger_delta) {
            return;
        }
        let spacing = self.layout(app).page_spacing();
        let range = DragRange {
            min: 0.0,
            max: (self.strip_page_count(app) - 1) as f64 * spacing,
            viewport: spacing,
        };
        let strip = self.strip(app);
        let pixels = scrolled_by_finger(strip.value(app) * spacing, finger_delta, range);
        strip.set_value(app, pixels / spacing);
    }

    fn release_strip(self: Handle<Self>, app: &mut App, owner: Owner, finger_velocity: f64) {
        match app.get_mut(self).strip_grip.release(owner) {
            None => {}
            Some(Released::HardlyMoved) => self.settle_strip(app, None),
            Some(Released::Dragged) => self.settle_strip_after_swipe(app, finger_velocity),
        }
    }

    /// The finger that dragged the strip lifted at `finger_velocity`: the deck heads to the page
    /// the swipe ends on, and the strip settles on it at the finger's speed.
    fn settle_strip_after_swipe(self: Handle<Self>, app: &mut App, finger_velocity: f64) {
        let position = self.strip(app).value(app);
        let page = page_after_swipe(position, finger_velocity, self.strip_page_count(app));
        let tab = app.get(self).followed.order.get(page).copied();
        self.change_tabs(app, StripFollows::Gliding, move |tabs, cx| {
            head_to_strip_page(tabs, tab, cx);
        });
        let spacing = self.layout(app).page_spacing();
        self.settle_strip(app, Some(-finger_velocity / spacing));
    }

    fn cancel_strip_hold(self: Handle<Self>, app: &mut App, owner: Owner) {
        if app.get_mut(self).strip_grip.cancel(owner) {
            self.settle_strip(app, None);
        }
    }

    /// How many pages the strip has: every tab's, and the home's past the last while no home is
    /// open.
    fn strip_page_count(self: Handle<Self>, app: &App) -> usize {
        let followed = &app.get(self).followed;
        followed.order.len() + usize::from(followed.home.is_none())
    }
}

/// The deck heads to the page of `tab`, or, for the page past the last tab, to the home, which
/// opens there.
fn head_to_strip_page(tabs: &mut Tabs, tab: Option<TabId>, cx: &mut waft::Context<Tabs>) {
    match tab {
        Some(tab) => tabs.show_page(tab, cx),
        None => {
            tabs.go_home(cx);
        }
    }
}

/// The page a swipe released at `position` and `finger_velocity` settles on, as Flutter's
/// `PageScrollPhysics` chooses it: a flick goes on to the next page in its direction, a slower
/// release to the nearest page.
fn page_after_swipe(position: f64, finger_velocity: f64, page_count: usize) -> usize {
    let heading = if finger_velocity <= -FLICK_VELOCITY {
        0.5
    } else if finger_velocity >= FLICK_VELOCITY {
        -0.5
    } else {
        0.0
    };
    (position + heading)
        .round()
        .clamp(0.0, (page_count - 1) as f64) as usize
}

/// Where the strip is once the tabs in `before` are those in `now`, so the pages it shows stay
/// where they are: it moves with the tab it is nearest to, which keeps its page in view whatever
/// came or went before it. Where that tab is gone, or the strip is past the last tab, it stays,
/// and what takes that place shows there.
pub(super) fn strip_among(strip: f64, before: &[TabId], now: &[TabId]) -> f64 {
    let nearest = strip.round();
    if nearest < 0.0 {
        return strip;
    }
    let moved_to = before
        .get(nearest as usize)
        .and_then(|tab| now.iter().position(|other| other == tab));
    match moved_to {
        Some(index) => strip + (index as f64 - nearest),
        None => strip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::tab_list::TabList;

    #[test]
    fn a_slow_release_settles_on_the_nearest_page() {
        assert_eq!(page_after_swipe(1.4, 0.0, 4), 1);
        assert_eq!(page_after_swipe(1.6, 0.0, 4), 2);
        assert_eq!(page_after_swipe(1.6, -299.0, 4), 2);
    }

    #[test]
    fn a_flick_goes_on_to_the_next_page_however_little_the_strip_moved() {
        assert_eq!(page_after_swipe(1.02, -800.0, 4), 2);
        assert_eq!(page_after_swipe(1.98, 800.0, 4), 1);
    }

    #[test]
    fn a_flick_that_turned_back_returns_to_the_page_it_left() {
        assert_eq!(page_after_swipe(1.3, 800.0, 4), 1);
    }

    #[test]
    fn a_flick_past_either_end_stays_on_the_end() {
        assert_eq!(page_after_swipe(-0.1, 900.0, 3), 0);
        assert_eq!(page_after_swipe(2.05, -900.0, 3), 2);
    }

    /// The ids of three programs' tabs and of the home opened after them, as a list hands them
    /// out.
    fn three_tabs_and_the_home() -> Vec<TabId> {
        let mut list = TabList::new();
        for name in ["a", "b", "c"] {
            list.go_home();
            list.open_program(&format!("dev.example.{name}").parse().unwrap());
        }
        list.go_home();
        list.tabs().iter().map(|tab| tab.id).collect()
    }

    fn three_tabs() -> Vec<TabId> {
        three_tabs_and_the_home()[..3].to_vec()
    }

    #[test]
    fn the_strip_moves_with_its_page_when_a_tab_before_it_goes() {
        let tabs = three_tabs();
        let without_first = &tabs[1..];
        assert_eq!(strip_among(2.0, &tabs, without_first), 1.0);
        assert_eq!(strip_among(1.8, &tabs, without_first), 0.8);
    }

    #[test]
    fn the_strip_stays_when_a_tab_after_it_goes_or_its_own_does() {
        let tabs = three_tabs();
        assert_eq!(strip_among(0.2, &tabs, &tabs[..2]), 0.2);
        let without_middle = [tabs[0], tabs[2]];
        assert_eq!(strip_among(1.0, &tabs, &without_middle), 1.0);
    }

    #[test]
    fn the_strip_past_the_last_tab_stays_as_the_home_opens_there() {
        let (tabs, with_home) = (three_tabs(), three_tabs_and_the_home());
        assert_eq!(strip_among(2.7, &tabs, &with_home), 2.7);
        assert_eq!(strip_among(-0.3, &tabs, &with_home), -0.3);
    }
}
