//! A page's flight between the strip and its card in the overview's grid: shrinking the current
//! page into its card, and growing a card back into its page.
//!
//! The grid stands still. From the moment a flight to the overview begins until the deck rests on
//! a page again, every tab but the one in flight is its card, in its own slot at its own size,
//! behind the page in flight: nothing fades in, the page covers the grid as it leaves and uncovers
//! it as it shrinks. Each tab has a flight of its own (`logic::deck_geometry`), so more than one
//! page can be in flight at once, each on its own spring: the page a card tapped during another's
//! landing sends off, or a page caught on the strip as a flight begins. A finger has one at most,
//! the current tab's.
//!
//! An upward drag on the bar takes the current tab's page into flight under the finger. The page's
//! size follows the finger's upward travel, from the whole page to its card's size at half the
//! page's height, and a little past it against a scroll view's resistance; and the page is carried
//! to stay under the finger both ways, its bottom edge as far above the finger as it was taken
//! hold of and the point of it the finger came down under still above the finger. The release
//! says open or closed, by how far the page would have flown once the finger's speed as it lifted
//! carried it on (`projection`), and the page springs from where it is, at the finger's speed, to
//! its slot or back to fill its box; the tabs button, Done and a tapped card are the same flight
//! with no finger.
//!
//! While the current tab's page flies to its card, a finger that comes down on it takes it over
//! from where it is, as the bar's does. Anywhere else on the overview there is no page under the
//! finger, and it does nothing until the overview is open enough to be used
//! (`overview_takes_touches`); from then on a vertical drag on the overview scrolls its grid
//! (`grid_scroll`). Which of the two a finger does is decided as it comes down.
//!
//! While the current tab's page flies to its page with no finger on it, after a tapped card, Done
//! or a release that sends it back, touches follow where the page is going, not where it is
//! drawn, as iOS hit-tests a view's final frame: the page takes them as it does at rest, and its
//! program hears them where the page will be once it lands (`frames`), so the program's own
//! gestures work from its first touch, and the flight goes on. What the card's clip still cuts
//! away of the page, along its top and bottom, takes no touch until it shows, shrinking to nothing
//! as the page lands: the clip is inside the transform that draws the flight, and a clip limits
//! where touches land in Waft as in Flutter, which has no clip that cuts only the drawing; no
//! other page and nothing of the deck's takes touches meanwhile, so such a touch goes to nobody.
//! The bar keeps its gestures throughout, while it shows, in the last quarter of a flight from a
//! card: a finger on it holds the page where it is drawn, its upward drag takes the page over from
//! there and its sideways drag swipes the strip, as from rest.
//!
//! The gesture's states are the overview's grip, which is on the current tab's flight. Free: every
//! flight settles where the tabs say. Held: a finger came down on the page and it stands still
//! where it was; a finger lifted without dragging, or whose drag went sideways, lets it settle
//! again. Dragged: the page follows the finger; a drag let go having hardly moved is a held finger
//! lifted. A grip dropped leaves the finger still down with nothing.
//!
//! The invariant: a tab's flight and its carry are only ever set by the finger that drags them or
//! by the springs that settle them, each from where the other left them, or at once where the
//! change cannot be seen: a tab off the screen on the strip takes its place in the grid as the
//! grid is stood up, and the cards go back to the strip once the deck rests on a page that covers
//! them.

use std::rc::Rc;

use waft::ui::PointerDeviceKind;
use waft::{AnimationController, App, Handle, Listener, Offset};
use waft_gestures::VelocityTracker;

use super::axis_drag::{AxisDrag, FingerMove};
use super::grip::{Owner, Released};
use super::overscroll::{DragRange, scrolled_by_finger};
use super::projection::projected;
use super::{DeckViewState, StripFollows, springs};
use crate::logic::deck_geometry::{CardPosition, PageHold};
use crate::logic::tab_list::TabId;

/// How far a page let go must be headed to fly, by where its release would carry it
/// (`projection`), for the overview to open.
const OPEN_THRESHOLD: f64 = 0.35;
/// How far the current tab's page has flown before the overview's cards and its toolbar take
/// touches. Before that a touch on the overview catches the flight instead.
const OVERVIEW_TOUCHABLE: f64 = 0.9;

/// The hold a dragging finger has of the page it took into flight.
pub(super) struct HeldPage {
    tab: TabId,
    /// How far upward the finger has taken the page, in points: the page's flight is this much of
    /// a whole flight's travel.
    travel: f64,
    /// Where on the page the finger is.
    hold: PageHold,
    /// How fast the finger has been carrying the page off its flight's line, for the springs that
    /// take it back.
    carrying_speed: VelocityTracker,
}

impl DeckViewState {
    /// A vertical drag by `owner`'s finger: on the page in flight it takes the page with it; on
    /// the overview, once that is open enough to be used, it scrolls the grid.
    pub(super) fn vertical_drag(self: Handle<Self>, owner: Owner) -> AxisDrag {
        AxisDrag {
            on_down: Rc::new(move |app| self.hold_vertical(app, owner)),
            on_start: Rc::new(move |app, at| self.start_vertical_drag(app, owner, at)),
            on_update: Rc::new(move |app, moved| self.drag_vertical(app, owner, moved)),
            on_end: Rc::new(move |app, velocity| self.release_vertical(app, owner, velocity)),
            on_cancel: Rc::new(move |app| self.cancel_vertical_hold(app, owner)),
        }
    }

    /// How far open the overview is, from 0, pages, to 1, cards: how far the current tab's page
    /// has flown. The bar fades out and the toolbar in by it.
    pub(super) fn openness(self: Handle<Self>, app: &App) -> f64 {
        let state = app.get(self);
        let flight = state
            .followed
            .current
            .and_then(|current| state.cards.get(&current))
            .map_or(0.0, |card| card.flight.value(app));
        flight.clamp(0.0, 1.0)
    }

    /// Whether the overview is open enough to be used: its cards and toolbar take touches, and a
    /// vertical drag on it scrolls its grid.
    pub(super) fn overview_takes_touches(self: Handle<Self>, app: &App) -> bool {
        app.get(self).followed.overview_open && self.openness(app) >= OVERVIEW_TOUCHABLE
    }

    /// Whether the current tab's page heads to its page with no finger on it: the deck rests on
    /// it, or it flies there as a tapped card, Done or a release sent it. Its program takes the
    /// touches on it then.
    pub(super) fn current_page_lands_untouched(self: Handle<Self>, app: &App) -> bool {
        let Some(current) = app.get(self).followed.current else {
            return false;
        };
        !self.is_in_hand(app, current) && self.flight_target(app, current) == 0.0
    }

    fn hold_vertical(self: Handle<Self>, app: &mut App, owner: Owner) {
        if owner != Owner::Bar && self.overview_takes_touches(app) {
            self.hold_grid(app, owner);
        } else if self.is_on_the_page(app, owner) && app.get_mut(self).overview_grip.hold(owner) {
            self.stop_current_page(app);
            self.redraw(app);
        }
    }

    /// Whether a finger of `owner`'s has the current tab's page under it: one on the bar beneath
    /// the page, or on the page's own card; not one anywhere else on the overview.
    fn is_on_the_page(self: Handle<Self>, app: &App, owner: Owner) -> bool {
        let current = app.get(self).followed.current;
        owner == Owner::Bar || current.is_some_and(|current| owner == Owner::Card(current))
    }

    /// Stops the current tab's page where it is, in its flight and where it was carried.
    fn stop_current_page(self: Handle<Self>, app: &mut App) {
        if let Some(current) = app.get(self).followed.current {
            self.stop_flight(app, current);
            self.stop_carry(app, current);
        }
    }

    fn start_vertical_drag(self: Handle<Self>, app: &mut App, owner: Owner, finger: Offset) {
        if app.get(self).grid_grip.is_held_by(owner) {
            self.start_grid_drag(app, owner);
        } else if app.get(self).overview_grip.is_held_by(owner) {
            self.start_overview_drag(app, owner, finger);
        }
    }

    fn drag_vertical(self: Handle<Self>, app: &mut App, owner: Owner, moved: FingerMove) {
        self.drag_grid(app, owner, moved.along);
        self.drag_overview(app, owner, moved);
    }

    fn release_vertical(self: Handle<Self>, app: &mut App, owner: Owner, finger_velocity: f64) {
        self.release_grid(app, owner, finger_velocity);
        self.release_overview(app, owner, finger_velocity);
    }

    fn cancel_vertical_hold(self: Handle<Self>, app: &mut App, owner: Owner) {
        self.cancel_grid_hold(app, owner);
        if app.get_mut(self).overview_grip.cancel(owner) {
            self.settle_flights(app);
        }
    }

    fn start_overview_drag(self: Handle<Self>, app: &mut App, owner: Owner, finger: Offset) {
        if !app.get_mut(self).overview_grip.drag(owner) {
            return;
        }
        self.leave_search(app);
        self.stand_grid(app);
        self.stop_current_page(app);
        let held = self.hold_of_current_page(app, finger);
        app.get_mut(self).held_page = held;
        self.settle_flights(app);
    }

    /// The hold a finger at `finger` takes of the current tab's page, where it is now.
    fn hold_of_current_page(self: Handle<Self>, app: &App, finger: Offset) -> Option<HeldPage> {
        let tab = app.get(self).followed.current?;
        let (index, card) = self.placed_card(app, tab)?;
        let layout = self.layout(app);
        Some(HeldPage {
            tab,
            travel: card.flight.max(0.0) * layout.full_flight_travel(),
            hold: layout.hold_of(index, finger, &card, &self.position(app)),
            carrying_speed: VelocityTracker::with_kind(PointerDeviceKind::Touch),
        })
    }

    /// Where `tab` is among the tabs, and its card's values now.
    fn placed_card(self: Handle<Self>, app: &App, tab: TabId) -> Option<(usize, CardPosition)> {
        let order = &app.get(self).followed.order;
        let index = order.iter().position(|other| *other == tab)?;
        let card = self.card_positions(app).get(index).copied()?;
        Some((index, card))
    }

    fn drag_overview(self: Handle<Self>, app: &mut App, owner: Owner, moved: FingerMove) {
        if !app.get_mut(self).overview_grip.moved(owner, moved.along) {
            return;
        }
        let held = app.get(self).held_page.as_ref();
        let Some((tab, travel)) = held.map(|held| (held.tab, held.travel)) else {
            return;
        };
        let Some(flight) = self.flight_of(app, tab) else {
            return;
        };
        let travel = self.travel_after(app, travel, moved.along);
        let whole_flight = self.layout(app).full_flight_travel();
        flight.set_value(app, travel / whole_flight);
        if let Some(held) = app.get_mut(self).held_page.as_mut() {
            held.travel = travel;
        }
        self.carry_page_with_finger(app, tab, moved);
    }

    /// How far upward a finger has taken a page that it had taken `travel`, once it moved by
    /// `finger_delta`: one to one up to a whole flight's travel, against a scroll view's
    /// resistance past that, and never less than nothing, the whole page.
    fn travel_after(self: Handle<Self>, app: &App, travel: f64, finger_delta: f64) -> f64 {
        let layout = self.layout(app);
        let range = DragRange {
            min: 0.0,
            max: layout.full_flight_travel(),
            viewport: layout.page_box().height(),
        };
        scrolled_by_finger(travel, finger_delta, range).max(0.0)
    }

    /// Carries the page of `tab` to where the finger that holds it is now.
    fn carry_page_with_finger(self: Handle<Self>, app: &mut App, tab: TabId, moved: FingerMove) {
        let Some(hold) = app.get(self).held_page.as_ref().map(|held| held.hold) else {
            return;
        };
        let (Some((index, position)), Some([across, down])) =
            (self.placed_card(app, tab), self.carry_of(app, tab))
        else {
            return;
        };
        let deck = self.position(app);
        let carry = self
            .layout(app)
            .carried_by(index, hold, moved.at, &position, &deck);
        across.set_value(app, carry.dx());
        down.set_value(app, carry.dy());
        let now = app.clock();
        if let (Some(held), Some(time)) = (app.get_mut(self).held_page.as_mut(), moved.time) {
            held.carrying_speed.add_position(now, time, carry);
        }
    }

    fn release_overview(self: Handle<Self>, app: &mut App, owner: Owner, finger_velocity: f64) {
        let Some(released) = app.get_mut(self).overview_grip.release(owner) else {
            return;
        };
        let Some(held) = app.get_mut(self).held_page.take() else {
            self.settle_flights(app);
            return;
        };
        if released == Released::HardlyMoved {
            self.settle_flights(app);
            return;
        }
        let whole_flight = self.layout(app).full_flight_travel();
        let flight_velocity = -finger_velocity / whole_flight;
        let open = projected(held.travel / whole_flight, flight_velocity) >= OPEN_THRESHOLD;
        self.change_tabs(app, StripFollows::Unseen, move |tabs, cx| {
            tabs.set_overview_open(open, cx);
        });
        self.settle_flight(app, held.tab, Some(flight_velocity));
        let carrying = held
            .carrying_speed
            .get_velocity(app.clock())
            .pixels_per_second;
        self.settle_carry(app, held.tab, Some(carrying));
    }

    /// The tabs button: the current page flies to its card, from wherever it is.
    pub(super) fn open_overview(self: Handle<Self>, app: &mut App) {
        self.change_tabs(app, StripFollows::Unseen, |tabs, cx| {
            tabs.set_overview_open(true, cx);
        });
    }

    /// Done: the current tab's card flies back to its page, from wherever it is.
    pub(super) fn close_overview(self: Handle<Self>, app: &mut App) {
        self.change_tabs(app, StripFollows::Unseen, |tabs, cx| {
            tabs.set_overview_open(false, cx);
        });
    }

    /// Stands the grid up behind the current page, as a flight to the overview begins: scrolled to
    /// show the current tab's card, with every other tab its card in its slot. A tab whose page
    /// is off the screen on the strip is in its slot at once, unseen behind the current page; one
    /// caught on the screen, the strip still settling, flies there.
    pub(super) fn stand_grid(self: Handle<Self>, app: &mut App) {
        if std::mem::replace(&mut app.get_mut(self).grid_stands, true) {
            return;
        }
        let followed = app.get(self).followed.clone();
        let layout = self.layout(app);
        let showing = layout.grid_scroll_showing(followed.current_index(), followed.order.len());
        self.grid_scroll(app).set_value(app, showing);
        let (deck, cards) = (self.position(app), self.card_positions(app));
        for (index, tab) in followed.order.iter().enumerate() {
            let on_screen = layout
                .place_tab(index, &cards[index], &deck)
                .is_visible(&layout);
            if Some(*tab) != followed.current && !on_screen {
                self.put_flight(app, *tab, 1.0);
            }
        }
    }

    /// Lays the grid down once the deck rests on a page: every tab is its page on the strip
    /// again, the cards unseen behind the page the deck rests on.
    pub(super) fn lay_grid_down(self: Handle<Self>, app: &mut App) {
        if !std::mem::replace(&mut app.get_mut(self).grid_stands, false) {
            return;
        }
        let tabs: Vec<TabId> = app.get(self).cards.keys().copied().collect();
        for tab in tabs {
            self.put_flight(app, tab, 0.0);
        }
    }

    /// Where the flight of `tab` heads: to its page for the current tab while the overview is
    /// closed, and for every tab while the grid is laid down; to its card otherwise.
    fn flight_target(self: Handle<Self>, app: &App, tab: TabId) -> f64 {
        let state = app.get(self);
        let flies_to_its_page =
            !state.followed.overview_open && state.followed.current == Some(tab);
        if !state.grid_stands || flies_to_its_page {
            0.0
        } else {
            1.0
        }
    }

    /// Puts the flight of `tab` at `flight` at once, where that cannot be seen.
    fn put_flight(self: Handle<Self>, app: &mut App, tab: TabId, flight: f64) {
        if let Some(motion) = self.flight_of(app, tab) {
            self.stop_flight(app, tab);
            motion.set_value(app, flight);
        }
    }

    fn stop_flight(self: Handle<Self>, app: &mut App, tab: TabId) {
        if let Some(flight) = self.flight_of(app, tab) {
            flight.stop(app, true);
            self.with_card(app, tab, |card| card.flight_heading = None);
        }
    }

    /// Stops the page of `tab` where a finger carried it.
    fn stop_carry(self: Handle<Self>, app: &mut App, tab: TabId) {
        if let Some(carry) = self.carry_of(app, tab) {
            for axis in carry {
                axis.stop(app, true);
            }
            self.with_card(app, tab, |card| card.carry_settling = [false; 2]);
        }
    }

    /// Lets every tab's flight settle where the tabs say it heads, and every page carried off its
    /// flight's line settle back onto it, each from where it is at the speed it has; but for the
    /// current tab's while a finger has it.
    pub(super) fn settle_flights(self: Handle<Self>, app: &mut App) {
        let tabs: Vec<TabId> = app.get(self).cards.keys().copied().collect();
        for tab in tabs {
            self.settle_flight(app, tab, None);
            self.settle_carry(app, tab, None);
        }
        self.check_arrival(app);
    }

    /// Whether a finger has the flight of `tab`: the current tab's, while the overview's grip is
    /// not free.
    fn is_in_hand(self: Handle<Self>, app: &App, tab: TabId) -> bool {
        let state = app.get(self);
        !state.overview_grip.is_free() && state.followed.current == Some(tab)
    }

    /// Lets the flight of `tab` settle where it heads, from where it is, at `velocity`, in
    /// flights a second, or at the speed it has; nothing while a finger has it.
    fn settle_flight(self: Handle<Self>, app: &mut App, tab: TabId, velocity: Option<f64>) {
        let Some((flight, heading)) = self
            .card(app, tab)
            .map(|card| (card.flight.handle(), card.flight_heading))
        else {
            return;
        };
        if self.is_in_hand(app, tab) {
            return;
        }
        let target = self.flight_target(app, tab);
        let heading_there = heading == Some(target);
        let there = flight.value(app) == target && heading.is_none();
        if there || (heading_there && velocity.is_none()) {
            return;
        }
        let velocity = velocity.unwrap_or_else(|| flight.velocity(app));
        self.with_card(app, tab, |card| card.flight_heading = Some(target));
        let spring = if target == 1.0 {
            springs::overview_opening()
        } else {
            springs::overview_closing()
        };
        let whole_flight = self.layout(app).full_flight_travel();
        springs::settle(app, flight, target, velocity, spring, whole_flight)
            .when_complete(app, Listener::new(move |app| self.flight_settled(app, tab)));
    }

    fn flight_settled(self: Handle<Self>, app: &mut App, tab: TabId) {
        self.with_card(app, tab, |card| card.flight_heading = None);
        self.check_arrival(app);
    }

    /// Lets the page of `tab` settle back onto its flight's line from where it was carried, at
    /// `velocity`, in points a second, or at the speed it has; nothing while a finger has it.
    fn settle_carry(self: Handle<Self>, app: &mut App, tab: TabId, velocity: Option<Offset>) {
        let Some(carry) = self.carry_of(app, tab) else {
            return;
        };
        if self.is_in_hand(app, tab) {
            return;
        }
        let velocity = velocity.map(|velocity| [velocity.dx(), velocity.dy()]);
        for axis in 0..2 {
            let velocity = velocity.map(|velocity| velocity[axis]);
            self.settle_carry_along(app, tab, axis, carry[axis], velocity);
        }
    }

    /// Lets one axis of the carry of `tab`'s page, `carry`, settle on nothing.
    fn settle_carry_along(
        self: Handle<Self>,
        app: &mut App,
        tab: TabId,
        axis: usize,
        carry: Handle<AnimationController>,
        velocity: Option<f64>,
    ) {
        let settling = self
            .card(app, tab)
            .is_some_and(|card| card.carry_settling[axis]);
        let on_its_line = carry.value(app) == 0.0 && !settling;
        if on_its_line || (settling && velocity.is_none()) {
            return;
        }
        let velocity = velocity.unwrap_or_else(|| carry.velocity(app));
        self.with_card(app, tab, |card| card.carry_settling[axis] = true);
        springs::settle(app, carry, 0.0, velocity, springs::overview_closing(), 1.0).when_complete(
            app,
            Listener::new(move |app| self.carry_settled(app, tab, axis)),
        );
    }

    fn carry_settled(self: Handle<Self>, app: &mut App, tab: TabId, axis: usize) {
        self.with_card(app, tab, |card| card.carry_settling[axis] = false);
        self.check_arrival(app);
    }

    /// Whether every tab's flight has landed where it heads, with no page carried off it or on its
    /// way back.
    pub(super) fn every_flight_has_landed(self: Handle<Self>, app: &App) -> bool {
        app.get(self).cards.iter().all(|(tab, card)| {
            card.flight.value(app) == self.flight_target(app, *tab)
                && card.flight_heading.is_none()
                && card.carry.iter().all(|axis| axis.value(app) == 0.0)
                && card.carry_settling == [false; 2]
        })
    }

    /// Does `change`, which moves the strip at once, with every page in flight kept where it is:
    /// each is carried by as much as the strip's move took it, and settles back from there.
    pub(super) fn keeping_pages_in_flight_in_place(
        self: Handle<Self>,
        app: &mut App,
        change: impl FnOnce(Handle<Self>, &mut App),
    ) {
        let before = self.clips_of_pages_in_flight(app);
        change(self, app);
        let after = self.clips_of_pages_in_flight(app);
        for (tab, was) in before {
            let (Some(now), Some([across, down])) = (after.get(&tab), self.carry_of(app, tab))
            else {
                continue;
            };
            let moved = *now - was;
            if moved == Offset::ZERO {
                continue;
            }
            self.stop_carry(app, tab);
            across.set_value(app, across.value(app) - moved.dx());
            down.set_value(app, down.value(app) - moved.dy());
        }
    }

    /// Where the clip of each tab in flight is drawn now, by its top left corner: every tab short
    /// of its card, which the strip's position has a part in placing.
    fn clips_of_pages_in_flight(
        self: Handle<Self>,
        app: &App,
    ) -> std::collections::HashMap<TabId, Offset> {
        let layout = self.layout(app);
        let (deck, cards) = (self.position(app), self.card_positions(app));
        let order = &app.get(self).followed.order;
        order
            .iter()
            .zip(&cards)
            .enumerate()
            .filter(|(_, (_, card))| card.flight < 1.0)
            .map(|(index, (tab, card))| {
                (*tab, layout.place_tab(index, card, &deck).clip.top_left())
            })
            .collect()
    }
}
