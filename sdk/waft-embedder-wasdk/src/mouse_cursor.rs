//! The cursor the framework asks for, as the view's: Waft's system cursor kinds by the CSS names
//! the WIT's `cursor` uses.

use waft_embedder::SystemMouseCursorKind;
use wasdk::views::Cursor;

/// The view's cursor for a system cursor kind, by the CSS name Flutter's web engine uses for it;
/// kinds with no CSS name show the arrow, as that engine falls back to `basic`.
pub(crate) fn cursor_of(kind: SystemMouseCursorKind) -> Cursor {
    match kind {
        SystemMouseCursorKind::None => Cursor::None,
        SystemMouseCursorKind::Basic | SystemMouseCursorKind::Disappearing => Cursor::Default,
        SystemMouseCursorKind::Click => Cursor::Pointer,
        SystemMouseCursorKind::Forbidden => Cursor::NotAllowed,
        SystemMouseCursorKind::Wait => Cursor::Wait,
        SystemMouseCursorKind::Progress => Cursor::Progress,
        SystemMouseCursorKind::ContextMenu => Cursor::ContextMenu,
        SystemMouseCursorKind::Help => Cursor::Help,
        SystemMouseCursorKind::Text => Cursor::Text,
        SystemMouseCursorKind::VerticalText => Cursor::VerticalText,
        SystemMouseCursorKind::Cell => Cursor::Cell,
        SystemMouseCursorKind::Precise => Cursor::Crosshair,
        SystemMouseCursorKind::Move => Cursor::Move,
        SystemMouseCursorKind::Grab => Cursor::Grab,
        SystemMouseCursorKind::Grabbing => Cursor::Grabbing,
        SystemMouseCursorKind::NoDrop => Cursor::NoDrop,
        SystemMouseCursorKind::Alias => Cursor::Alias,
        SystemMouseCursorKind::Copy => Cursor::Copy,
        SystemMouseCursorKind::AllScroll => Cursor::AllScroll,
        SystemMouseCursorKind::ResizeLeftRight => Cursor::EwResize,
        SystemMouseCursorKind::ResizeUpDown => Cursor::NsResize,
        SystemMouseCursorKind::ResizeUpLeftDownRight => Cursor::NwseResize,
        SystemMouseCursorKind::ResizeUpRightDownLeft => Cursor::NeswResize,
        SystemMouseCursorKind::ResizeUp => Cursor::NResize,
        SystemMouseCursorKind::ResizeDown => Cursor::SResize,
        SystemMouseCursorKind::ResizeLeft => Cursor::WResize,
        SystemMouseCursorKind::ResizeRight => Cursor::EResize,
        SystemMouseCursorKind::ResizeUpLeft => Cursor::NwResize,
        SystemMouseCursorKind::ResizeUpRight => Cursor::NeResize,
        SystemMouseCursorKind::ResizeDownLeft => Cursor::SwResize,
        SystemMouseCursorKind::ResizeDownRight => Cursor::SeResize,
        SystemMouseCursorKind::ResizeColumn => Cursor::ColResize,
        SystemMouseCursorKind::ResizeRow => Cursor::RowResize,
        SystemMouseCursorKind::ZoomIn => Cursor::ZoomIn,
        SystemMouseCursorKind::ZoomOut => Cursor::ZoomOut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cursor_with_a_css_name_keeps_it() {
        assert_eq!(cursor_of(SystemMouseCursorKind::Click), Cursor::Pointer);
        assert_eq!(cursor_of(SystemMouseCursorKind::Precise), Cursor::Crosshair);
        assert_eq!(
            cursor_of(SystemMouseCursorKind::Forbidden),
            Cursor::NotAllowed
        );
        assert_eq!(
            cursor_of(SystemMouseCursorKind::ResizeUpLeftDownRight),
            Cursor::NwseResize
        );
        assert_eq!(cursor_of(SystemMouseCursorKind::None), Cursor::None);
    }

    /// Flutter's web engine shows the arrow for a cursor with no CSS name of its own.
    #[test]
    fn a_cursor_with_no_css_name_shows_the_arrow() {
        assert_eq!(cursor_of(SystemMouseCursorKind::Basic), Cursor::Default);
        assert_eq!(
            cursor_of(SystemMouseCursorKind::Disappearing),
            Cursor::Default
        );
    }
}
