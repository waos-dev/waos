//! Who has hold of one of the values the deck animates: nobody, so it settles on its own; a
//! finger that came down and has not moved far yet, which holds the value still; or a finger
//! dragging it. One finger at a time: the gestures' callbacks ask here before they touch the
//! value, so a second finger, or a callback of a gesture that was dropped, changes nothing.
//!
//! A lone gesture wins its finger the moment it comes down, with no touch's slop to cross, so a
//! tap is then a drag that went nowhere: the grip counts how far a drag moved, and one let go
//! having hardly moved is told apart, so a finger that only touched leaves the value going where
//! it was, whichever way its gesture was recognized.
//!
//! The invariant: every change of a value by a gesture is made by the owner the grip names, and a
//! grip let go, cancelled or dropped is free.

use waft_gestures::K_TOUCH_SLOP;

use crate::logic::tab_list::TabId;

/// Where a finger came down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Owner {
    /// On the bar.
    Bar,
    /// On the overview, between its cards.
    Overview,
    /// On the card of a tab, in the overview.
    Card(TabId),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Grip {
    #[default]
    Free,
    /// A finger of `Owner`'s is down and holds the value where it is.
    Held(Owner),
    /// A finger of `by`'s drags the value, and has moved `travel` points so far, there and back.
    Dragged { by: Owner, travel: f64 },
}

/// How a finger that dragged a value let go of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Released {
    /// After dragging it: where the value goes is the release's to say.
    Dragged,
    /// Having hardly moved, less than what makes a touch a drag: a finger that held the value and
    /// lifted, which leaves the value going where it was.
    HardlyMoved,
}

impl Grip {
    pub(super) fn is_free(self) -> bool {
        self == Grip::Free
    }

    /// Whether a finger of `owner`'s holds the value, down and not dragging yet.
    pub(super) fn is_held_by(self, owner: Owner) -> bool {
        self == Grip::Held(owner)
    }

    fn is_dragged_by_another(self, owner: Owner) -> bool {
        matches!(self, Grip::Dragged { by, .. } if by != owner)
    }

    /// A finger of `owner`'s came down: it holds the value, unless another's drags it. Whether
    /// it does.
    pub(super) fn hold(&mut self, owner: Owner) -> bool {
        if self.is_dragged_by_another(owner) {
            return false;
        }
        *self = Grip::Held(owner);
        true
    }

    /// The finger of `owner`'s starts to drag: unless another's drags the value. Whether it does.
    pub(super) fn drag(&mut self, owner: Owner) -> bool {
        if self.is_dragged_by_another(owner) {
            return false;
        }
        *self = Grip::Dragged {
            by: owner,
            travel: 0.0,
        };
        true
    }

    /// The finger of `owner`'s moved by `delta` points. Whether it drags the value, and so moves
    /// it.
    pub(super) fn moved(&mut self, owner: Owner, delta: f64) -> bool {
        match self {
            Grip::Dragged { by, travel } if *by == owner => {
                *travel += delta.abs();
                true
            }
            _ => false,
        }
    }

    /// The finger of `owner`'s that dragged the value lifted: how, or none where it was not
    /// dragging it.
    pub(super) fn release(&mut self, owner: Owner) -> Option<Released> {
        let Grip::Dragged { by, travel } = *self else {
            return None;
        };
        if by != owner {
            return None;
        }
        *self = Grip::Free;
        Some(if travel < K_TOUCH_SLOP {
            Released::HardlyMoved
        } else {
            Released::Dragged
        })
    }

    /// The finger of `owner`'s that held the value lifted without dragging it, or its drag went
    /// to another gesture. Whether it was holding it.
    pub(super) fn cancel(&mut self, owner: Owner) -> bool {
        let held = *self == Grip::Held(owner);
        if held {
            *self = Grip::Free;
        }
        held
    }

    /// Whoever had the value has it no more.
    pub(super) fn drop(&mut self) {
        *self = Grip::Free;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finger_holds_then_drags_then_lets_go() {
        let mut grip = Grip::default();
        assert!(grip.hold(Owner::Bar));
        assert!(
            !grip.moved(Owner::Bar, 5.0),
            "it holds, and drags nothing yet"
        );
        assert!(grip.drag(Owner::Bar));
        assert!(grip.moved(Owner::Bar, 30.0));
        assert!(
            !grip.cancel(Owner::Bar),
            "a drag is released, not cancelled"
        );
        assert_eq!(grip.release(Owner::Bar), Some(Released::Dragged));
        assert!(grip.is_free());
    }

    #[test]
    fn a_drag_that_hardly_moved_is_a_held_finger_lifting() {
        let mut grip = Grip::default();
        grip.drag(Owner::Bar);
        assert_eq!(grip.release(Owner::Bar), Some(Released::HardlyMoved));
        grip.drag(Owner::Bar);
        grip.moved(Owner::Bar, 6.0);
        grip.moved(Owner::Bar, -6.0);
        assert_eq!(grip.release(Owner::Bar), Some(Released::HardlyMoved));
        grip.drag(Owner::Bar);
        grip.moved(Owner::Bar, 12.0);
        grip.moved(Owner::Bar, -12.0);
        assert_eq!(
            grip.release(Owner::Bar),
            Some(Released::Dragged),
            "there and back is a drag"
        );
    }

    #[test]
    fn a_second_finger_takes_nothing_from_a_drag() {
        let mut grip = Grip::default();
        grip.drag(Owner::Bar);
        assert!(!grip.hold(Owner::Overview));
        assert!(!grip.drag(Owner::Overview));
        assert!(!grip.moved(Owner::Overview, 40.0));
        assert_eq!(grip.release(Owner::Overview), None);
        assert!(grip.moved(Owner::Bar, 40.0));
    }

    #[test]
    fn a_hold_passes_to_the_finger_that_came_down_last() {
        let mut grip = Grip::default();
        grip.hold(Owner::Bar);
        assert!(grip.hold(Owner::Overview));
        assert!(
            !grip.cancel(Owner::Bar),
            "the bar's finger holds it no more"
        );
        assert!(grip.cancel(Owner::Overview));
        assert!(grip.is_free());
    }

    #[test]
    fn a_dropped_grip_ignores_the_finger_still_down() {
        let mut grip = Grip::default();
        grip.drag(Owner::Bar);
        grip.drop();
        assert!(!grip.moved(Owner::Bar, 40.0));
        assert_eq!(grip.release(Owner::Bar), None);
    }
}
