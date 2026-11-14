//! Where a prompt's dialog goes: the screen of the program's front window, or the main screen while
//! the program has none, and the middle of what the system's own bars leave free of it, in logical
//! points from the top-left of the main screen, y down, as Waft gives a window's frame.

use waft::ui::{Screen, ScreenId};
use waft::{Rect, Size};

/// The screen among `screens` whose id is `front`, the screen of the program's front window; the
/// main screen, Waft's first, while `front` is none or names no screen listed.
pub(crate) fn screen_for(screens: &[Screen], front: Option<ScreenId>) -> Option<&Screen> {
    front
        .and_then(|front| screens.iter().find(|screen| screen.id == front))
        .or_else(|| screens.first())
}

/// A frame of `size` in the middle of what `screen`'s menu bar and Dock leave free.
pub(crate) fn centred_frame(size: Size, screen: &Screen) -> Rect {
    let area = screen.visible_frame;
    Rect::from_ltwh(
        area.left + (area.width() - size.width()) / 2.0,
        area.top + (area.height() - size.height()) / 2.0,
        size.width(),
        size.height(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: u64, visible_frame: Rect) -> Screen {
        Screen {
            id: ScreenId(id),
            name: String::new(),
            frame: visible_frame,
            visible_frame,
            scale: 2.0,
            refresh_rate: None,
        }
    }

    #[test]
    fn a_dialog_goes_on_the_front_windows_screen_or_else_the_main_one() {
        let main = screen(1, Rect::from_ltwh(0.0, 0.0, 1512.0, 982.0));
        let beside = screen(2, Rect::from_ltwh(1512.0, 0.0, 2560.0, 1440.0));
        let screens = [main, beside];
        assert_eq!(
            screen_for(&screens, Some(ScreenId(2))).unwrap().id,
            ScreenId(2)
        );
        assert_eq!(screen_for(&screens, None).unwrap().id, ScreenId(1));
        assert_eq!(
            screen_for(&screens, Some(ScreenId(9))).unwrap().id,
            ScreenId(1),
            "a screen since gone"
        );
        assert!(screen_for(&[], None).is_none());
    }

    #[test]
    fn a_dialog_is_centred_in_what_the_bars_leave_free() {
        let below_the_menu_bar = screen(2, Rect::from_ltwh(1512.0, 25.0, 2560.0, 1415.0));
        let frame = centred_frame(Size::new(440.0, 348.0), &below_the_menu_bar);
        assert_eq!(frame, Rect::from_ltwh(2572.0, 558.5, 440.0, 348.0));
    }
}
