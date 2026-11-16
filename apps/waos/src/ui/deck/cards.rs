//! The overview's cards: tapping one to grow it back into its page, throwing one away sideways to
//! close its tab and end its program, and the others moving into the place it leaves.
//!
//! A thrown card follows the finger one to one to the left and resists to the right. Let go headed
//! far enough left, by where the finger's speed would carry it (`projection`), it is leaving: it
//! flies off on a spring that starts at the finger's velocity, the other cards move into its place
//! at once, and its tab closes when it is gone, so a finger that catches it on the way keeps the
//! tab, and the others make room again. Let go short of that, or on its way back, it springs back.
//! Every card springs to its place from where it is at the speed it has,
//! so closing several cards quickly never jumps. The home alone cannot be closed, so its card
//! always springs back.
//!
//! A card's gestures are offered once the overview is open enough to be used
//! (`overview_takes_touches`); before that a finger does nothing but on the page in flight, which
//! it takes over (`overview_transition`). The throw's states are the card's grip. Free: the card
//! springs back to its place, or flies off while it is leaving. Held: a finger came down on it and
//! it stands still; a card caught in flight is leaving no more from that moment, the others making
//! room for it again, and a finger lifted without dragging, or whose drag went up or down, lets it
//! spring back to its place. Dragged: the card follows the finger; the release decides whether it
//! leaves. A grip dropped, because the deck was sent elsewhere, lets the card spring back too.
//!
//! The invariant: a tab is closed only by its card's flight ending while the card is still
//! leaving; a finger down on a card that is leaving ends its leaving, and that finger's tap does
//! nothing more than that.

use std::rc::Rc;

use waft::{AnimationController, App, Handle, Listener, Ref};

use super::axis_drag::AxisDrag;
use super::grip::{Grip, Owner, Released};
use super::overscroll::{DragRange, followed_by_finger};
use super::projection::projected;
use super::{DeckViewState, StripFollows, springs};
use crate::logic::tab_list::TabId;

/// How far left, as a fraction of its width, a card let go must be headed, by where its release
/// would carry it, to close its tab.
const THROW_DISTANCE: f64 = 0.45;
/// The speed the close button sends a card off at.
const CLOSE_BUTTON_VELOCITY: f64 = -1400.0;
/// How far past the view's edge a card flies before it is gone.
const FLIGHT_MARGIN: f64 = 40.0;

/// One card's animated values, and who has hold of its throw.
pub(super) struct CardMotions {
    /// The grid slot the card is in, fractional while it moves to a new one.
    pub(super) slot: Ref<AnimationController>,
    /// The slot it is in or moving to.
    pub(super) slot_heading: f64,
    /// How far the card is dragged or thrown sideways, in points.
    pub(super) throw: Ref<AnimationController>,
    pub(super) throw_grip: Grip,
    /// Whether the finger that last came down on the card caught it in flight: its tap then
    /// keeps the tab and grows nothing.
    pub(super) caught_in_flight: bool,
    /// How far the tab is from its page on the strip, 0, to its card in the grid, 1
    /// (`overview_transition`).
    pub(super) flight: Ref<AnimationController>,
    /// Where the flight's spring is settling, on the page or on the card, while it is.
    pub(super) flight_heading: Option<f64>,
    /// How far the tab's page is carried on screen off its flight's line, across and down, by the
    /// finger that drags it, in points.
    pub(super) carry: [Ref<AnimationController>; 2],
    /// Whether the spring that takes each of the two back is running.
    pub(super) carry_settling: [bool; 2],
}

impl DeckViewState {
    /// A tap on the card of `tab`: the deck heads to its page, the card growing into it.
    pub(super) fn tap_card(self: Handle<Self>, app: &mut App, tab: TabId) {
        let on_its_way_out = app.get(self).leaving.contains(&tab)
            || self
                .card(app, tab)
                .is_some_and(|card| card.caught_in_flight);
        if !self.overview_takes_touches(app) || on_its_way_out {
            return;
        }
        self.change_tabs(app, StripFollows::Unseen, move |tabs, cx| {
            tabs.show_page(tab, cx);
        });
    }

    /// The close button on the card of `tab`: the card leaves, as if thrown.
    pub(super) fn close_card(self: Handle<Self>, app: &mut App, tab: TabId) {
        if self.overview_takes_touches(app) && self.may_close(app, tab) {
            self.send_card_away(app, tab, CLOSE_BUTTON_VELOCITY);
        }
    }

    /// The sideways drag on the card of `tab`.
    pub(super) fn card_throw(self: Handle<Self>, tab: TabId) -> AxisDrag {
        let owner = Owner::Card(tab);
        AxisDrag {
            on_down: Rc::new(move |app| self.hold_card(app, tab, owner)),
            on_start: Rc::new(move |app, _| self.start_card_throw(app, tab, owner)),
            on_update: Rc::new(move |app, moved| self.drag_card(app, tab, owner, moved.along)),
            on_end: Rc::new(move |app, velocity| self.release_card(app, tab, owner, velocity)),
            on_cancel: Rc::new(move |app| self.cancel_card_hold(app, tab, owner)),
        }
    }

    fn hold_card(self: Handle<Self>, app: &mut App, tab: TabId, owner: Owner) {
        if !self.overview_takes_touches(app) {
            return;
        }
        let held = self.with_card(app, tab, |card| card.throw_grip.hold(owner));
        if held == Some(true) {
            self.stop_throw(app, tab);
            self.catch_card(app, tab);
        }
    }

    /// A finger came down on the card of `tab`: where the card was leaving, it leaves no more,
    /// and the other cards make room for it again.
    fn catch_card(self: Handle<Self>, app: &mut App, tab: TabId) {
        let caught = app.get_mut(self).leaving.remove(&tab);
        self.with_card(app, tab, |card| card.caught_in_flight = caught);
        if caught {
            self.move_cards_to_their_slots(app);
            self.keep_grid_in_range(app);
        }
    }

    fn start_card_throw(self: Handle<Self>, app: &mut App, tab: TabId, owner: Owner) {
        let dragged = self.with_card(app, tab, |card| {
            card.throw_grip.is_held_by(owner) && card.throw_grip.drag(owner)
        });
        if dragged == Some(true) {
            self.stop_throw(app, tab);
        }
    }

    fn drag_card(self: Handle<Self>, app: &mut App, tab: TabId, owner: Owner, finger_delta: f64) {
        let moved = self.with_card(app, tab, |card| card.throw_grip.moved(owner, finger_delta));
        let (Some(true), Some(throw)) = (moved, self.throw_of(app, tab)) else {
            return;
        };
        let view_width = self.layout(app).page_box().width();
        let range = DragRange {
            min: -view_width * 2.0,
            max: 0.0,
            viewport: view_width,
        };
        let offset = followed_by_finger(throw.value(app), finger_delta, range);
        throw.set_value(app, offset);
    }

    fn release_card(
        self: Handle<Self>,
        app: &mut App,
        tab: TabId,
        owner: Owner,
        finger_velocity: f64,
    ) {
        let released = self.with_card(app, tab, |card| card.throw_grip.release(owner));
        match released.flatten() {
            None => {}
            Some(Released::HardlyMoved) => self.return_card(app, tab, 0.0),
            Some(Released::Dragged) if self.is_thrown(app, tab, finger_velocity) => {
                self.send_card_away(app, tab, finger_velocity);
            }
            Some(Released::Dragged) => self.return_card(app, tab, finger_velocity),
        }
    }

    fn cancel_card_hold(self: Handle<Self>, app: &mut App, tab: TabId, owner: Owner) {
        let cancelled = self.with_card(app, tab, |card| card.throw_grip.cancel(owner));
        if cancelled == Some(true) {
            self.return_card(app, tab, 0.0);
        }
    }

    /// Whether the card of `tab`, let go at `finger_velocity`, was thrown away: headed far enough
    /// to the left, by where the release would carry it (`projection`); never the home alone.
    fn is_thrown(self: Handle<Self>, app: &App, tab: TabId, finger_velocity: f64) -> bool {
        let Some(card) = self.card(app, tab) else {
            return false;
        };
        let card_width = self.layout(app).card_size().width();
        let headed_to = projected(card.throw.value(app), finger_velocity);
        headed_to <= -card_width * THROW_DISTANCE && self.may_close(app, tab)
    }

    /// Whether `tab` may close: any but the home alone.
    fn may_close(self: Handle<Self>, app: &App, tab: TabId) -> bool {
        let followed = &app.get(self).followed;
        !(followed.order == [tab] && followed.home == Some(tab))
    }

    /// Springs the card of `tab` back to its place, from where it is at `finger_velocity`.
    fn return_card(self: Handle<Self>, app: &mut App, tab: TabId, finger_velocity: f64) {
        let Some(throw) = self.throw_of(app, tab) else {
            return;
        };
        if throw.value(app) == 0.0 && finger_velocity == 0.0 {
            return;
        }
        springs::settle(
            app,
            throw,
            0.0,
            finger_velocity,
            springs::card_return(),
            1.0,
        );
    }

    /// The card of `tab` is leaving: the others move into its place, it flies off the left of the
    /// view from where it is at `velocity`, and its tab closes once it is gone.
    fn send_card_away(self: Handle<Self>, app: &mut App, tab: TabId, velocity: f64) {
        let (Some(slot), Some(throw)) = (self.slot_of(app, tab), self.throw_of(app, tab)) else {
            return;
        };
        app.get_mut(self).leaving.insert(tab);
        self.move_cards_to_their_slots(app);
        self.keep_grid_in_range(app);
        let grid_scroll = self.grid_scroll(app).value(app);
        let card_right = self
            .layout(app)
            .card_rect(slot.value(app), grid_scroll)
            .right;
        let gone = -(card_right + FLIGHT_MARGIN);
        springs::settle(
            app,
            throw,
            gone.min(throw.value(app)),
            velocity.min(0.0),
            springs::card_throw(),
            1.0,
        )
        .when_complete(
            app,
            Listener::new(move |app| self.close_tab_of_gone_card(app, tab)),
        );
    }

    /// The card of `tab` flew off: its tab closes, which ends its program. A tab that stays all
    /// the same, the home alone by then, has its card back.
    fn close_tab_of_gone_card(self: Handle<Self>, app: &mut App, tab: TabId) {
        if !app.get(self).leaving.contains(&tab) {
            return;
        }
        self.change_tabs(app, StripFollows::Unseen, move |tabs, cx| {
            tabs.close(tab, cx)
        });
        if app.get_mut(self).leaving.remove(&tab) {
            self.move_cards_to_their_slots(app);
            self.return_card(app, tab, 0.0);
        }
    }

    /// Takes every card out of the fingers' hands: a finger still down on one does nothing more,
    /// and the card it held or dragged springs back to its place.
    pub(super) fn drop_card_grips(self: Handle<Self>, app: &mut App) {
        let in_hand: Vec<TabId> = app
            .get(self)
            .cards
            .iter()
            .filter(|(_, card)| !card.throw_grip.is_free())
            .map(|(tab, _)| *tab)
            .collect();
        for tab in in_hand {
            self.with_card(app, tab, |card| card.throw_grip.drop());
            self.return_card(app, tab, 0.0);
        }
    }

    /// Stops the throw of the card of `tab` where it is.
    fn stop_throw(self: Handle<Self>, app: &mut App, tab: TabId) {
        if let Some(throw) = self.throw_of(app, tab) {
            throw.stop(app, true);
        }
    }

    /// The tabs whose cards are staying, in order: every tab but those whose cards are leaving.
    pub(super) fn cards_staying(self: Handle<Self>, app: &App) -> Vec<TabId> {
        let state = app.get(self);
        state
            .followed
            .order
            .iter()
            .filter(|tab| !state.leaving.contains(tab))
            .copied()
            .collect()
    }

    /// Sends every card that stays to the slot its tab has among those staying, on a spring from
    /// where it is at the speed it has; at once while the overview is closed, where no card can
    /// be seen moving.
    pub(super) fn move_cards_to_their_slots(self: Handle<Self>, app: &mut App) {
        let unseen = !app.get(self).grid_stands;
        let card_width = self.layout(app).card_size().width();
        for (slot, tab) in self.cards_staying(app).into_iter().enumerate() {
            let slot = slot as f64;
            let heading = self.with_card(app, tab, |card| {
                std::mem::replace(&mut card.slot_heading, slot)
            });
            let (Some(heading), Some(slot_motion)) = (heading, self.slot_of(app, tab)) else {
                continue;
            };
            if unseen {
                slot_motion.set_value(app, slot);
            } else if heading != slot {
                let velocity = slot_motion.velocity(app);
                springs::settle(
                    app,
                    slot_motion,
                    slot,
                    velocity,
                    springs::card_reflow(),
                    card_width,
                );
            }
        }
    }

    pub(super) fn card(self: Handle<Self>, app: &App, tab: TabId) -> Option<&CardMotions> {
        app.get(self).cards.get(&tab)
    }

    /// The card of `tab`'s grid slot.
    pub(super) fn slot_of(
        self: Handle<Self>,
        app: &App,
        tab: TabId,
    ) -> Option<Handle<AnimationController>> {
        Some(self.card(app, tab)?.slot.handle())
    }

    /// How far the card of `tab` is thrown sideways.
    pub(super) fn throw_of(
        self: Handle<Self>,
        app: &App,
        tab: TabId,
    ) -> Option<Handle<AnimationController>> {
        Some(self.card(app, tab)?.throw.handle())
    }

    /// The flight of `tab` between its page and its card.
    pub(super) fn flight_of(
        self: Handle<Self>,
        app: &App,
        tab: TabId,
    ) -> Option<Handle<AnimationController>> {
        Some(self.card(app, tab)?.flight.handle())
    }

    /// How far the page of `tab` is carried off its flight's line, across and down.
    pub(super) fn carry_of(
        self: Handle<Self>,
        app: &App,
        tab: TabId,
    ) -> Option<[Handle<AnimationController>; 2]> {
        let [across, down] = &self.card(app, tab)?.carry;
        Some([across.handle(), down.handle()])
    }

    /// What `change` answers of the card of `tab`, which it may change; none without the card.
    pub(super) fn with_card<R>(
        self: Handle<Self>,
        app: &mut App,
        tab: TabId,
        change: impl FnOnce(&mut CardMotions) -> R,
    ) -> Option<R> {
        app.get_mut(self).cards.get_mut(&tab).map(change)
    }
}
