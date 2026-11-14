//! View focus between the framework and the platform: a change the platform reports, and one
//! the framework asks for. Both sides speak Flutter's `ViewFocusEvent` vocabulary, so this is a
//! rename; the platform's event leaves out the view, which its stream already names.

use waft_embedder::{ViewFocusDirection, ViewFocusEvent, ViewFocusState, ViewId};
use wasdk::views;

/// The platform's focus change on the view `id`, as Flutter's `onViewFocusChange` reports it.
pub(crate) fn framework_focus_event(id: ViewId, event: views::ViewFocusEvent) -> ViewFocusEvent {
    ViewFocusEvent {
        view_id: id,
        state: match event.state {
            views::ViewFocusState::Unfocused => ViewFocusState::Unfocused,
            views::ViewFocusState::Focused => ViewFocusState::Focused,
        },
        direction: match event.direction {
            views::ViewFocusDirection::Undefined => ViewFocusDirection::Undefined,
            views::ViewFocusDirection::Forward => ViewFocusDirection::Forward,
            views::ViewFocusDirection::Backward => ViewFocusDirection::Backward,
        },
    }
}

/// The state the framework asks a view's focus to change to.
pub(crate) fn platform_focus_state(state: ViewFocusState) -> views::ViewFocusState {
    match state {
        ViewFocusState::Unfocused => views::ViewFocusState::Unfocused,
        ViewFocusState::Focused => views::ViewFocusState::Focused,
    }
}

/// The way the framework asks the focus to move.
pub(crate) fn platform_focus_direction(direction: ViewFocusDirection) -> views::ViewFocusDirection {
    match direction {
        ViewFocusDirection::Undefined => views::ViewFocusDirection::Undefined,
        ViewFocusDirection::Forward => views::ViewFocusDirection::Forward,
        ViewFocusDirection::Backward => views::ViewFocusDirection::Backward,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_focus_change_names_the_view_it_arrived_on() {
        let event = framework_focus_event(
            ViewId(3),
            views::ViewFocusEvent {
                state: views::ViewFocusState::Focused,
                direction: views::ViewFocusDirection::Backward,
            },
        );
        assert_eq!(event.view_id, ViewId(3));
        assert_eq!(event.state, ViewFocusState::Focused);
        assert_eq!(event.direction, ViewFocusDirection::Backward);
    }

    #[test]
    fn a_focus_request_keeps_its_state_and_direction() {
        assert_eq!(
            platform_focus_state(ViewFocusState::Unfocused),
            views::ViewFocusState::Unfocused
        );
        assert_eq!(
            platform_focus_direction(ViewFocusDirection::Forward),
            views::ViewFocusDirection::Forward
        );
    }
}
