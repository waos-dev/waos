//! How a program's view stands on screen on the desktop, which the program's lifecycle state is
//! derived from: whether its window has the input focus, as Waft reports it; whether the program
//! hid its window; and whether the window is minimized or occluded, as Waft's window says each
//! time Waft reports that it changed.
//!
//! The window answers for itself, whatever the process's other windows show: a window minimized is
//! off screen, and one covered all over occluded, while another of the process's shows. An occluded
//! window is reported on screen and occluded, apart, since the engine decides what being covered
//! means: out of sight, unless a tool watches the program. Where the system does not say one of
//! them, as Windows does not say a window is covered, the window answers no. The invariant: a view
//! is off screen or occluded only for what is known, so a program is never frozen while its window
//! shows.

use waft::HostWindow;
use waos::Presence;

/// How a view's window stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowStanding {
    /// Whether the window has the input focus, as Waft last reported.
    pub(crate) focused: bool,
    /// Whether the program hid the window.
    pub(crate) hidden: bool,
    /// Whether the window is minimized, as it last said.
    pub(crate) minimized: bool,
    /// Whether the window is occluded, covered all over or otherwise not shown, as it last said.
    pub(crate) occluded: bool,
}

impl WindowStanding {
    /// A window as its configuration opens it, shown or hidden as `visible` says, and focused if
    /// it takes the focus when shown, until Waft reports otherwise, so a program is not told
    /// `inactive` for the moment before its new window's focus is reported; minimized and occluded
    /// as `window` says now.
    pub(crate) fn opened(
        visible: bool,
        activating: bool,
        window: &dyn HostWindow,
    ) -> WindowStanding {
        let mut standing = WindowStanding {
            focused: visible && activating,
            hidden: !visible,
            minimized: false,
            occluded: false,
        };
        standing.read_window(window);
        standing
    }

    /// Takes whether `window` is minimized and whether it is occluded, as it says now.
    pub(crate) fn read_window(&mut self, window: &dyn HostWindow) {
        self.minimized = window.is_minimized();
        self.occluded = window.is_occluded();
    }

    /// On screen unless the program hid the window or the window is minimized, and occluded as
    /// the window says.
    pub(crate) fn presence(self) -> Presence {
        Presence {
            on_screen: !self.hidden && !self.minimized,
            occluded: self.occluded,
            focused: self.focused,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use waft::WindowConfig;
    use waft_test::{TestView, TestWindow};

    use super::*;

    /// A window with no window server behind it, neither minimized nor occluded until a test says.
    fn window() -> Rc<TestWindow> {
        TestWindow::around(TestView::new(400.0, 300.0), WindowConfig::default())
    }

    fn shown_and_focused() -> WindowStanding {
        WindowStanding::opened(true, true, &*window())
    }

    #[test]
    fn a_window_opened_visible_and_activating_is_on_screen_and_focused() {
        let presence = shown_and_focused().presence();
        assert!(presence.on_screen && presence.focused);
    }

    #[test]
    fn a_window_opened_without_activating_is_on_screen_unfocused() {
        let presence = WindowStanding::opened(true, false, &*window()).presence();
        assert!(presence.on_screen && !presence.focused);
    }

    #[test]
    fn a_window_opened_hidden_is_off_screen_and_unfocused() {
        let presence = WindowStanding::opened(false, true, &*window()).presence();
        assert!(!presence.on_screen && !presence.focused);
    }

    #[test]
    fn a_window_opened_minimized_is_off_screen_and_one_opened_covered_occluded() {
        let minimized = window();
        minimized.set_minimized(true);
        let presence = WindowStanding::opened(true, true, &*minimized).presence();
        assert!(!presence.on_screen && !presence.in_sight());

        let occluded = window();
        occluded.set_occluded(true);
        let presence = WindowStanding::opened(true, true, &*occluded).presence();
        assert!(presence.on_screen && presence.occluded && !presence.in_sight());
    }

    #[test]
    fn a_minimized_window_is_off_screen_and_keeps_the_focus_reported() {
        let standing = WindowStanding {
            minimized: true,
            ..shown_and_focused()
        };
        let presence = standing.presence();
        assert!(!presence.on_screen && presence.focused);
    }

    #[test]
    fn an_occluded_window_is_on_screen_and_occluded_out_of_sight() {
        let standing = WindowStanding {
            occluded: true,
            ..shown_and_focused()
        };
        let presence = standing.presence();
        assert!(presence.on_screen && presence.occluded && presence.focused);
        assert!(!presence.in_sight());
    }

    #[test]
    fn a_window_is_in_sight_as_it_says_of_itself() {
        let window = window();
        let mut standing = WindowStanding::opened(true, true, &*window);
        window.set_occluded(true);
        standing.read_window(&*window);
        assert!(standing.presence().occluded, "covered");
        assert!(!standing.presence().in_sight(), "covered");

        window.set_occluded(false);
        window.set_minimized(true);
        standing.read_window(&*window);
        assert!(!standing.presence().on_screen, "minimized");

        window.set_minimized(false);
        standing.read_window(&*window);
        assert!(standing.presence().in_sight(), "shown again");
    }

    #[test]
    fn a_window_the_program_hid_is_off_screen_whatever_it_says() {
        let window = window();
        let mut standing = WindowStanding::opened(false, true, &*window);
        standing.read_window(&*window);
        assert!(!standing.presence().on_screen);
    }
}
