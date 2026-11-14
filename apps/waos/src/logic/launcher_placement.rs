//! Where the launcher shows: in the middle of the screen the user is on, as Spotlight shows, which
//! is the screen under the pointer, or the main screen where the pointer is on none or unknown.
//!
//! Everything is in the coordinates of window positions: logical points from the top-left corner
//! of the main screen, y growing down, as Waft's host reports the screens and the pointer.
//!
//! The invariant: the launcher is centred in the screen's visible frame, clear of its menu bar,
//! Dock or taskbar, and lies within it wherever the frame is large enough to hold it.

use waft::ui::Screen;
use waft::{Offset, Rect, Size};

/// Where a launcher of `size` shows among `screens`, with the pointer at `pointer`; nothing
/// without a screen.
pub(crate) fn launcher_frame(
    size: Size,
    screens: &[Screen],
    pointer: Option<Offset>,
) -> Option<Rect> {
    let screen = pointer
        .and_then(|pointer| screens.iter().find(|screen| screen.frame.contains(pointer)))
        .or_else(|| screens.first())?;
    let visible = screen.visible_frame;
    let centre = visible.center();
    let left = (centre.dx() - size.width() / 2.0).max(visible.left);
    let top = (centre.dy() - size.height() / 2.0).max(visible.top);
    Some(Rect::from_ltwh(left, top, size.width(), size.height()))
}

#[cfg(test)]
mod tests {
    use waft::ui::ScreenId;

    use super::*;

    /// A screen at `frame`, its menu bar and Dock leaving `visible`.
    fn screen(id: u64, frame: Rect, visible: Rect) -> Screen {
        Screen {
            id: ScreenId(id),
            name: String::new(),
            frame,
            visible_frame: visible,
            scale: 2.0,
            refresh_rate: None,
        }
    }

    fn two_screens() -> Vec<Screen> {
        vec![
            screen(
                1,
                Rect::from_ltwh(0.0, 0.0, 1728.0, 1117.0),
                Rect::from_ltrb(0.0, 33.0, 1728.0, 1047.0),
            ),
            screen(
                2,
                Rect::from_ltwh(1728.0, 0.0, 1920.0, 1080.0),
                Rect::from_ltrb(1728.0, 25.0, 3648.0, 1080.0),
            ),
        ]
    }

    #[test]
    fn the_launcher_is_centred_in_the_visible_frame_of_the_pointers_screen() {
        let frame = launcher_frame(
            Size::new(680.0, 460.0),
            &two_screens(),
            Some(Offset::new(2000.0, 500.0)),
        );
        assert_eq!(
            frame,
            Some(Rect::from_ltwh(2688.0 - 340.0, 552.5 - 230.0, 680.0, 460.0))
        );
    }

    #[test]
    fn without_the_pointer_on_a_screen_the_launcher_is_on_the_main_one() {
        let frame = launcher_frame(Size::new(680.0, 460.0), &two_screens(), None).unwrap();
        assert_eq!(frame.center(), Offset::new(864.0, 540.0));
    }

    #[test]
    fn a_launcher_larger_than_the_screen_starts_at_its_visible_top_left() {
        let screens = [screen(
            1,
            Rect::from_ltwh(0.0, 0.0, 600.0, 400.0),
            Rect::from_ltrb(0.0, 25.0, 600.0, 400.0),
        )];
        let frame = launcher_frame(Size::new(680.0, 460.0), &screens, None).unwrap();
        assert_eq!((frame.left, frame.top), (0.0, 25.0));
    }

    #[test]
    fn no_screen_places_nothing() {
        assert_eq!(launcher_frame(Size::new(680.0, 460.0), &[], None), None);
    }
}
