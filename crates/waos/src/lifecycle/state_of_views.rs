//! The state a program is in, derived from its views: the rule Flutter's desktop embedders
//! derive an application's state from its windows by, applied to views so a desktop's window and
//! a phone's tab are read the same way.
//!
//! The invariant: a program is in the state of its most shown view, and a program with no view is
//! in no state of its own, so whoever applies the rule leaves such a program where it was.

use crate::bindings::wasdk::platform::lifecycle::LifecycleState;

/// How one of a program's views stands, for the state it puts its program in. The shell reports
/// it into the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presence {
    /// The view is on screen: its window is neither minimized nor hidden, its tab is the one
    /// open, nor anything else the shell counts as off screen. A window other windows cover is on
    /// screen, and occluded.
    pub on_screen: bool,
    /// The platform says the view's window is occluded: covered all over by other windows, or
    /// otherwise not shown, as on another of macOS's desktops or a screen asleep. False where the
    /// platform does not say, as Windows does not.
    pub occluded: bool,
    /// The view has the input focus: the user works in it.
    pub focused: bool,
}

impl Presence {
    /// Whether the view is in sight: on screen and not occluded.
    pub fn in_sight(self) -> bool {
        self.on_screen && !self.occluded
    }
}

/// The state of a program whose views stand as `views` say: resumed while one of them is in
/// sight with the input focus, inactive while one is in sight, hidden once none is. `None` for a
/// program with no view, whose state is left where it was.
pub fn state_of_views(views: impl IntoIterator<Item = Presence>) -> Option<LifecycleState> {
    views
        .into_iter()
        .map(state_of_view)
        .min_by_key(|state| shown_first(*state))
}

/// The state one view alone would put its program in.
fn state_of_view(view: Presence) -> LifecycleState {
    match (view.in_sight(), view.focused) {
        (true, true) => LifecycleState::Resumed,
        (true, false) => LifecycleState::Inactive,
        (false, _) => LifecycleState::Hidden,
    }
}

/// Orders states by how much of the program they show, the most first: a program is in the
/// state of its most shown view.
fn shown_first(state: LifecycleState) -> u8 {
    match state {
        LifecycleState::Resumed => 0,
        LifecycleState::Inactive => 1,
        LifecycleState::Hidden => 2,
        LifecycleState::Paused => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOCUSED: Presence = Presence {
        on_screen: true,
        occluded: false,
        focused: true,
    };
    const ON_SCREEN: Presence = Presence {
        on_screen: true,
        occluded: false,
        focused: false,
    };
    const OFF_SCREEN: Presence = Presence {
        on_screen: false,
        occluded: false,
        focused: false,
    };

    #[test]
    fn a_program_is_in_the_state_of_its_most_shown_view() {
        assert_eq!(
            state_of_views([OFF_SCREEN, FOCUSED, ON_SCREEN]),
            Some(LifecycleState::Resumed)
        );
        assert_eq!(
            state_of_views([OFF_SCREEN, ON_SCREEN]),
            Some(LifecycleState::Inactive)
        );
        assert_eq!(
            state_of_views([OFF_SCREEN, OFF_SCREEN]),
            Some(LifecycleState::Hidden)
        );
    }

    #[test]
    fn a_focused_view_off_screen_shows_nothing() {
        let minimizing = Presence {
            on_screen: false,
            occluded: false,
            focused: true,
        };
        assert_eq!(state_of_views([minimizing]), Some(LifecycleState::Hidden));
    }

    #[test]
    fn an_occluded_view_on_screen_is_out_of_sight() {
        let covered = Presence {
            occluded: true,
            ..FOCUSED
        };
        assert!(!covered.in_sight());
        assert_eq!(state_of_views([covered]), Some(LifecycleState::Hidden));
        assert_eq!(
            state_of_views([covered, ON_SCREEN]),
            Some(LifecycleState::Inactive)
        );
    }

    #[test]
    fn a_program_with_no_view_is_left_where_it_was() {
        assert_eq!(state_of_views([]), None);
    }
}
