//! What takes the touches on the deck while it is not on a page or on its way to one: a layer over
//! every page, with a box over each card for the card's own gestures, its tap and its sideways
//! throw, one over its close button, and the vertical drag everywhere, on a card or between the
//! cards, that catches a page's flight or scrolls the grid. It draws nothing: what shows around a
//! card is the card's (`card_chrome`).
//!
//! The layer is above the pages rather than around them: while the deck rests on a page, or the
//! current page flies to it with no finger on it, the layer takes nothing, and the page takes
//! every touch on it with no gesture of the deck's in the way. Each card's box is there at every
//! build, with the same gestures (`axis_drag`); only whether the layer takes touches changes. A
//! close button's box is there only while the button shows.
//!
//! The invariant: exactly one of the pages and this layer takes touches: the pages while the
//! current tab's page heads to its page with no finger on it, the deck resting on it or the page
//! flying there; this layer otherwise, while the overview is open or opening, or a finger has the
//! page.

use waft::{
    App, Clip, GestureDetector, Handle, IgnorePointer, IntoWidget, KeyedSubtree, Listener,
    Positioned, Stack, StackFit, ValueKey, WidgetRef,
};
use waft_rendering::HitTestBehavior;

use super::DeckViewState;
use super::axis_drag::AxisDragCallbacks;
use super::card_chrome::close_button_area;
use super::frames::PlacedTab;
use super::grip::Owner;
use crate::logic::tab_list::TabId;

/// The boxes of a card in the layer's stack, for keys unique among its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CardBox {
    Gestures,
    CloseButton,
}

impl DeckViewState {
    /// The layer, over `placed`, the tabs as they are drawn now.
    pub(super) fn touch_layer(self: Handle<Self>, app: &App, placed: &[PlacedTab]) -> WidgetRef {
        let between_the_cards = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .vertical_axis_drag(self.vertical_drag(Owner::Overview));
        let mut children = vec![between_the_cards.into_widget()];
        for card in self.in_paint_order(app, placed) {
            let Some(tab) = card.tab.tab else {
                continue;
            };
            children.push(self.card_gestures(tab, card));
            if card.placement.chrome_opacity > 0.0 {
                children.push(self.close_button(app, tab, card));
            }
        }
        IgnorePointer::new()
            .ignoring(!self.touch_layer_takes_touches(app))
            .child(
                Stack::new()
                    .fit(StackFit::Expand)
                    .clip_behavior(Clip::None)
                    .children(children),
            )
            .into_widget()
    }

    /// The box over the card of `tab` that takes its tap, its sideways throw and the vertical drag
    /// on it.
    fn card_gestures(self: Handle<Self>, tab: TabId, card: &PlacedTab) -> WidgetRef {
        let gestures = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(Listener::new(move |app| self.tap_card(app, tab)))
            .horizontal_axis_drag(self.card_throw(tab))
            .vertical_axis_drag(self.vertical_drag(Owner::Card(tab)));
        keyed(
            tab,
            CardBox::Gestures,
            Positioned::from_rect(card.placement.clip, gestures),
        )
    }

    /// The box over the close button of the card of `tab`, once the overview takes touches.
    fn close_button(self: Handle<Self>, app: &App, tab: TabId, card: &PlacedTab) -> WidgetRef {
        let button = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(Listener::new(move |app| self.close_card(app, tab)));
        keyed(
            tab,
            CardBox::CloseButton,
            Positioned::from_rect(
                close_button_area(card),
                IgnorePointer::new()
                    .ignoring(!self.overview_takes_touches(app))
                    .child(button),
            ),
        )
    }
}

fn keyed(tab: TabId, part: CardBox, child: Positioned) -> WidgetRef {
    KeyedSubtree::new(child)
        .key(ValueKey::new((tab, part)).into())
        .into_widget()
}
