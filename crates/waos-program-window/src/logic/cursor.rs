//! The cursor a program asks for, as one of the application's.
//!
//! A program names cursors by their CSS names, which is how Flutter's web engine names the
//! system cursors; this is that table read the other way, and the inverse of the one the
//! program's own embedder uses to ask.

use waft_services::{SystemMouseCursor, SystemMouseCursors};
use waos::bindings::wasdk::platform::views::Cursor;

/// The application's cursor for the one `cursor` names.
pub(crate) fn cursor_of(cursor: Cursor) -> SystemMouseCursor {
    match cursor {
        Cursor::None => SystemMouseCursors::NONE,
        Cursor::Default => SystemMouseCursors::BASIC,
        Cursor::ContextMenu => SystemMouseCursors::CONTEXT_MENU,
        Cursor::Help => SystemMouseCursors::HELP,
        Cursor::Pointer => SystemMouseCursors::CLICK,
        Cursor::Progress => SystemMouseCursors::PROGRESS,
        Cursor::Wait => SystemMouseCursors::WAIT,
        Cursor::Cell => SystemMouseCursors::CELL,
        Cursor::Crosshair => SystemMouseCursors::PRECISE,
        Cursor::Text => SystemMouseCursors::TEXT,
        Cursor::VerticalText => SystemMouseCursors::VERTICAL_TEXT,
        Cursor::Alias => SystemMouseCursors::ALIAS,
        Cursor::Copy => SystemMouseCursors::COPY,
        Cursor::Move => SystemMouseCursors::MOVE,
        Cursor::NoDrop => SystemMouseCursors::NO_DROP,
        Cursor::NotAllowed => SystemMouseCursors::FORBIDDEN,
        Cursor::Grab => SystemMouseCursors::GRAB,
        Cursor::Grabbing => SystemMouseCursors::GRABBING,
        Cursor::EResize => SystemMouseCursors::RESIZE_RIGHT,
        Cursor::NResize => SystemMouseCursors::RESIZE_UP,
        Cursor::NeResize => SystemMouseCursors::RESIZE_UP_RIGHT,
        Cursor::NwResize => SystemMouseCursors::RESIZE_UP_LEFT,
        Cursor::SResize => SystemMouseCursors::RESIZE_DOWN,
        Cursor::SeResize => SystemMouseCursors::RESIZE_DOWN_RIGHT,
        Cursor::SwResize => SystemMouseCursors::RESIZE_DOWN_LEFT,
        Cursor::WResize => SystemMouseCursors::RESIZE_LEFT,
        Cursor::EwResize => SystemMouseCursors::RESIZE_LEFT_RIGHT,
        Cursor::NsResize => SystemMouseCursors::RESIZE_UP_DOWN,
        Cursor::NeswResize => SystemMouseCursors::RESIZE_UP_RIGHT_DOWN_LEFT,
        Cursor::NwseResize => SystemMouseCursors::RESIZE_UP_LEFT_DOWN_RIGHT,
        Cursor::ColResize => SystemMouseCursors::RESIZE_COLUMN,
        Cursor::RowResize => SystemMouseCursors::RESIZE_ROW,
        Cursor::AllScroll => SystemMouseCursors::ALL_SCROLL,
        Cursor::ZoomIn => SystemMouseCursors::ZOOM_IN,
        Cursor::ZoomOut => SystemMouseCursors::ZOOM_OUT,
    }
}
