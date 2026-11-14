//! Scrolling the overview's grid of cards, when they do not fit: the same resistance and the same
//! glide a Cupertino scroll view has, asked of its physics directly (`overscroll`).
//!
//! The gesture's states are the grid's grip. Free: the grid glides, and springs back inside its
//! range where a card closing left it outside. Held: a finger came down on the open overview and
//! the grid stands still. Dragged: the grid follows the finger, resisting past either end; the
//! release lets it glide at the finger's speed.

use waft::{App, Handle};

use super::DeckViewState;
use super::grip::{Owner, Released};
use super::overscroll::{DragRange, released, scrolled_by_finger};

impl DeckViewState {
    pub(super) fn hold_grid(self: Handle<Self>, app: &mut App, owner: Owner) {
        if app.get_mut(self).grid_grip.hold(owner) {
            self.grid_scroll(app).stop(app, true);
        }
    }

    pub(super) fn start_grid_drag(self: Handle<Self>, app: &mut App, owner: Owner) {
        if app.get_mut(self).grid_grip.drag(owner) {
            self.grid_scroll(app).stop(app, true);
        }
    }

    pub(super) fn drag_grid(self: Handle<Self>, app: &mut App, owner: Owner, finger_delta: f64) {
        if !app.get_mut(self).grid_grip.moved(owner, finger_delta) {
            return;
        }
        let grid_scroll = self.grid_scroll(app);
        let scrolled = scrolled_by_finger(
            grid_scroll.value(app),
            finger_delta,
            self.grid_scroll_range(app),
        );
        grid_scroll.set_value(app, scrolled);
    }

    pub(super) fn release_grid(
        self: Handle<Self>,
        app: &mut App,
        owner: Owner,
        finger_velocity: f64,
    ) {
        match app.get_mut(self).grid_grip.release(owner) {
            None => {}
            Some(Released::HardlyMoved) => self.glide_grid(app, 0.0),
            Some(Released::Dragged) => self.glide_grid(app, finger_velocity),
        }
    }

    pub(super) fn cancel_grid_hold(self: Handle<Self>, app: &mut App, owner: Owner) {
        if app.get_mut(self).grid_grip.cancel(owner) {
            self.glide_grid(app, 0.0);
        }
    }

    /// Lets the grid glide as after a finger lifted at `finger_velocity`, and spring back inside
    /// its range where it is outside; nothing while a finger has it.
    pub(super) fn glide_grid(self: Handle<Self>, app: &mut App, finger_velocity: f64) {
        if !app.get(self).grid_grip.is_free() {
            return;
        }
        let grid_scroll = self.grid_scroll(app);
        let range = self.grid_scroll_range(app);
        if let Some(glide) = released(grid_scroll.value(app), finger_velocity, range) {
            grid_scroll.animate_with(app, glide);
        }
    }

    /// Keeps the grid inside its range once the cards it holds changed, at the speed it has.
    pub(super) fn keep_grid_in_range(self: Handle<Self>, app: &mut App) {
        let grid_scroll = self.grid_scroll(app);
        let scrolling_at = grid_scroll.velocity(app);
        self.glide_grid(app, -scrolling_at);
    }

    fn grid_scroll_range(self: Handle<Self>, app: &App) -> DragRange {
        let layout = self.layout(app);
        DragRange {
            min: 0.0,
            max: layout.max_grid_scroll(self.cards_staying(app).len()),
            viewport: layout.page_box().height(),
        }
    }
}
