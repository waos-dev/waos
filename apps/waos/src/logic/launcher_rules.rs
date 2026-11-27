//! When the launcher shows and goes on a desktop, as Spotlight shows and goes:
//!
//! 1. It is summoned by WaOS's icon in the Dock or the taskbar, by its icon in the menu bar or the
//!    tray, by a second start of WaOS, by WaOS's first start, and by the library asking to be seen.
//! 2. It is dismissed by Escape or the system's close command, by a click outside once it has had
//!    the focus, and by opening a program; not by the focus going to the file picker the library
//!    opened for a package, which is the launcher's own, until the picker answers.
//! 3. Dismissed, it is minimized where a desktop shows a running app in its taskbar only through
//!    the app's windows and WaOS is kept there, so its button stays; otherwise it is hidden, and
//!    hidden by the user, it hands the focus back to where it was.
//! 4. A minimized launcher the user restores from its button is summoned again: it says so by
//!    taking the focus while dismissed.
//!
//! The invariant: only focus the launcher has had since it showed can be lost: at launch AppKit
//! reports the window unfocused before it first reports it focused, while WaOS is still becoming
//! the active application (measured: AppKit reports the window unfocused before it first reports it
//! focused).

/// What happens to the launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LauncherStep {
    Summon,
    Dismiss(Dismissal),
    /// Leave it as it is.
    Keep,
}

/// How the launcher goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dismissal {
    /// Minimized, its button staying in the taskbar or the dock.
    Minimize,
    /// Hidden, handing the focus back to where it was before the launcher showed, where the user
    /// dismissed it; the focus is already elsewhere otherwise.
    Hide { hand_back_focus: bool },
}

/// How the launcher goes whenever it is dismissed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WhenDismissed {
    Minimized,
    Hidden,
}

impl WhenDismissed {
    /// How the launcher goes on a desktop that shows a running app in its taskbar or dock only
    /// through the app's windows, as Windows and Linux do, where `apps_shown_by_windows` says, and
    /// with WaOS kept there, where `kept_in_dock` says: minimized only where both hold, since a
    /// hidden launcher would leave nothing there to click.
    pub(crate) fn on_desktop(apps_shown_by_windows: bool, kept_in_dock: bool) -> WhenDismissed {
        if apps_shown_by_windows && kept_in_dock {
            WhenDismissed::Minimized
        } else {
            WhenDismissed::Hidden
        }
    }
}

/// Whether the launcher shows, whether it has had the focus since, and how it goes.
#[derive(Debug)]
pub(crate) struct LauncherRules {
    shown: bool,
    focused_since_shown: bool,
    when_dismissed: WhenDismissed,
}

impl LauncherRules {
    /// The launcher, not shown, going as `when_dismissed` says.
    pub(crate) fn new(when_dismissed: WhenDismissed) -> LauncherRules {
        LauncherRules {
            shown: false,
            focused_since_shown: false,
            when_dismissed,
        }
    }

    pub(crate) fn is_shown(&self) -> bool {
        self.shown
    }

    pub(crate) fn when_dismissed(&self) -> WhenDismissed {
        self.when_dismissed
    }

    /// The launcher goes as `when_dismissed` says from its next dismissal on.
    pub(crate) fn go_when_dismissed(&mut self, when_dismissed: WhenDismissed) {
        self.when_dismissed = when_dismissed;
    }

    /// Something summoned the launcher: it shows, where it does not.
    pub(crate) fn summoned(&mut self) -> LauncherStep {
        if self.shown {
            return LauncherStep::Keep;
        }
        self.shown = true;
        self.focused_since_shown = false;
        LauncherStep::Summon
    }

    /// The user dismissed the launcher: Escape, or the system's close command.
    pub(crate) fn dismissed_by_user(&mut self) -> LauncherStep {
        self.dismiss(true)
    }

    /// A program was opened from the launcher, and its window comes forward.
    pub(crate) fn program_opened(&mut self) -> LauncherStep {
        self.dismiss(false)
    }

    /// The launcher's window gained or lost the focus, while the library's file picker is open
    /// where `picking` says: a click elsewhere dismisses it, the picker taking the focus does not,
    /// and a minimized one regaining the focus was restored by the user.
    pub(crate) fn focus_changed(&mut self, focused: bool, picking: bool) -> LauncherStep {
        if focused {
            if !self.shown && self.when_dismissed == WhenDismissed::Minimized {
                self.shown = true;
            }
            self.focused_since_shown = self.shown;
            return LauncherStep::Keep;
        }
        if !self.focused_since_shown || picking {
            return LauncherStep::Keep;
        }
        self.dismiss(false)
    }

    /// Dismisses the launcher, where it shows, `by_user` or not.
    fn dismiss(&mut self, by_user: bool) -> LauncherStep {
        if !self.shown {
            return LauncherStep::Keep;
        }
        self.shown = false;
        self.focused_since_shown = false;
        LauncherStep::Dismiss(match self.when_dismissed {
            WhenDismissed::Minimized => Dismissal::Minimize,
            WhenDismissed::Hidden => Dismissal::Hide {
                hand_back_focus: by_user,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rules shown and focused, going as `when_dismissed` says.
    fn shown_and_focused(when_dismissed: WhenDismissed) -> LauncherRules {
        let mut rules = LauncherRules::new(when_dismissed);
        rules.summoned();
        rules.focus_changed(true, false);
        rules
    }

    #[test]
    fn summoning_shows_the_launcher_once() {
        let mut rules = LauncherRules::new(WhenDismissed::Hidden);
        assert_eq!(rules.summoned(), LauncherStep::Summon);
        assert_eq!(rules.summoned(), LauncherStep::Keep);
        assert!(rules.is_shown());
    }

    #[test]
    fn escape_hides_the_launcher_and_hands_the_focus_back() {
        let mut rules = shown_and_focused(WhenDismissed::Hidden);
        assert_eq!(
            rules.dismissed_by_user(),
            LauncherStep::Dismiss(Dismissal::Hide {
                hand_back_focus: true
            })
        );
        assert_eq!(rules.dismissed_by_user(), LauncherStep::Keep);
    }

    #[test]
    fn a_launcher_kept_in_the_taskbar_is_minimized() {
        let mut rules = shown_and_focused(WhenDismissed::Minimized);
        assert_eq!(
            rules.dismissed_by_user(),
            LauncherStep::Dismiss(Dismissal::Minimize)
        );
    }

    #[test]
    fn losing_the_focus_dismisses_the_launcher_only_once_it_has_had_it() {
        let mut rules = LauncherRules::new(WhenDismissed::Hidden);
        rules.summoned();
        assert_eq!(rules.focus_changed(false, false), LauncherStep::Keep);
        assert_eq!(rules.focus_changed(true, false), LauncherStep::Keep);
        assert_eq!(
            rules.focus_changed(false, false),
            LauncherStep::Dismiss(Dismissal::Hide {
                hand_back_focus: false
            })
        );
    }

    #[test]
    fn the_focus_going_to_the_librarys_file_picker_keeps_the_launcher() {
        let mut rules = shown_and_focused(WhenDismissed::Hidden);
        assert_eq!(rules.focus_changed(false, true), LauncherStep::Keep);
        assert!(rules.is_shown());
        assert_eq!(rules.focus_changed(true, false), LauncherStep::Keep);
        assert_eq!(
            rules.focus_changed(false, false),
            LauncherStep::Dismiss(Dismissal::Hide {
                hand_back_focus: false
            }),
            "once the picker answered, a click elsewhere dismisses it"
        );
    }

    #[test]
    fn opening_a_program_dismisses_the_launcher() {
        let mut rules = shown_and_focused(WhenDismissed::Hidden);
        assert_eq!(
            rules.program_opened(),
            LauncherStep::Dismiss(Dismissal::Hide {
                hand_back_focus: false
            })
        );
    }

    #[test]
    fn a_minimized_launcher_taking_the_focus_was_restored_and_shows() {
        let mut rules = shown_and_focused(WhenDismissed::Minimized);
        rules.dismissed_by_user();
        assert_eq!(rules.focus_changed(false, false), LauncherStep::Keep);
        assert_eq!(rules.focus_changed(true, false), LauncherStep::Keep);
        assert!(rules.is_shown());
        assert_eq!(
            rules.focus_changed(false, false),
            LauncherStep::Dismiss(Dismissal::Minimize),
            "it goes again as a click elsewhere takes the focus"
        );
    }

    #[test]
    fn a_hidden_launcher_taking_the_focus_stays_dismissed() {
        let mut rules = shown_and_focused(WhenDismissed::Hidden);
        rules.dismissed_by_user();
        rules.focus_changed(true, false);
        assert!(!rules.is_shown());
        assert_eq!(rules.focus_changed(false, false), LauncherStep::Keep);
    }

    #[test]
    fn a_launcher_summoned_again_waits_for_the_focus_again() {
        let mut rules = shown_and_focused(WhenDismissed::Hidden);
        rules.dismissed_by_user();
        rules.summoned();
        assert_eq!(rules.focus_changed(false, false), LauncherStep::Keep);
    }

    #[test]
    fn the_dismissal_follows_the_choice_from_the_next_one_on() {
        let mut rules = shown_and_focused(WhenDismissed::Minimized);
        rules.go_when_dismissed(WhenDismissed::Hidden);
        assert_eq!(
            rules.dismissed_by_user(),
            LauncherStep::Dismiss(Dismissal::Hide {
                hand_back_focus: true
            })
        );
    }

    #[test]
    fn only_a_desktop_showing_apps_by_their_windows_and_keeping_waos_minimizes() {
        assert_eq!(
            WhenDismissed::on_desktop(true, true),
            WhenDismissed::Minimized
        );
        assert_eq!(
            WhenDismissed::on_desktop(true, false),
            WhenDismissed::Hidden
        );
        assert_eq!(
            WhenDismissed::on_desktop(false, true),
            WhenDismissed::Hidden
        );
    }
}
