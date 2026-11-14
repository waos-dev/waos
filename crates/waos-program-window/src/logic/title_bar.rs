//! A window's title bar between Waft's terms and a program's: macOS's title bar as the program
//! asked for it, and what the system's settings say a double-click on a title bar does, as the
//! program reads it.

use waos::bindings::wasdk::platform::windows::{MacosTitleBar, TitleBarAction};

/// macOS's title bar the program asked for, in Waft's terms.
pub(crate) fn waft_title_bar(title_bar: MacosTitleBar) -> waft::MacosTitleBar {
    match title_bar {
        MacosTitleBar::Standard => waft::MacosTitleBar::Standard,
        MacosTitleBar::Hidden(height) => waft::MacosTitleBar::Hidden { height },
    }
}

/// What Waft's window says a double-click on a title bar does, as the program reads it: none
/// where Waft cannot tell, the system having no such setting.
pub(crate) fn title_bar_action_of(action: waft::TitleBarAction) -> Option<TitleBarAction> {
    match action {
        waft::TitleBarAction::Maximize => Some(TitleBarAction::Maximize),
        waft::TitleBarAction::Minimize => Some(TitleBarAction::Minimize),
        waft::TitleBarAction::Nothing => Some(TitleBarAction::Nothing),
        waft::TitleBarAction::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_title_bar_keeps_its_bands_height() {
        assert_eq!(
            waft_title_bar(MacosTitleBar::Standard),
            waft::MacosTitleBar::Standard
        );
        assert_eq!(
            waft_title_bar(MacosTitleBar::Hidden(52.0)),
            waft::MacosTitleBar::Hidden { height: 52.0 }
        );
    }

    #[test]
    fn a_double_click_keeps_its_name_and_one_waft_cannot_tell_is_none() {
        assert_eq!(
            title_bar_action_of(waft::TitleBarAction::Maximize),
            Some(TitleBarAction::Maximize)
        );
        assert_eq!(
            title_bar_action_of(waft::TitleBarAction::Minimize),
            Some(TitleBarAction::Minimize)
        );
        assert_eq!(
            title_bar_action_of(waft::TitleBarAction::Nothing),
            Some(TitleBarAction::Nothing)
        );
        assert_eq!(title_bar_action_of(waft::TitleBarAction::Unknown), None);
    }
}
