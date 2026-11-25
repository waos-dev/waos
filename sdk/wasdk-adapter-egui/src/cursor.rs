//! egui's cursor shapes as a view's: both are CSS's cursors, so each has its namesake.

use egui::CursorIcon;
use wasdk::views::Cursor;

pub(super) fn cursor_of(icon: CursorIcon) -> Cursor {
    match icon {
        CursorIcon::Default => Cursor::Default,
        CursorIcon::None => Cursor::None,
        CursorIcon::ContextMenu => Cursor::ContextMenu,
        CursorIcon::Help => Cursor::Help,
        CursorIcon::PointingHand => Cursor::Pointer,
        CursorIcon::Progress => Cursor::Progress,
        CursorIcon::Wait => Cursor::Wait,
        CursorIcon::Cell => Cursor::Cell,
        CursorIcon::Crosshair => Cursor::Crosshair,
        CursorIcon::Text => Cursor::Text,
        CursorIcon::VerticalText => Cursor::VerticalText,
        CursorIcon::Alias => Cursor::Alias,
        CursorIcon::Copy => Cursor::Copy,
        CursorIcon::Move => Cursor::Move,
        CursorIcon::NoDrop => Cursor::NoDrop,
        CursorIcon::NotAllowed => Cursor::NotAllowed,
        CursorIcon::Grab => Cursor::Grab,
        CursorIcon::Grabbing => Cursor::Grabbing,
        CursorIcon::AllScroll => Cursor::AllScroll,
        CursorIcon::ResizeHorizontal => Cursor::EwResize,
        CursorIcon::ResizeNeSw => Cursor::NeswResize,
        CursorIcon::ResizeNwSe => Cursor::NwseResize,
        CursorIcon::ResizeVertical => Cursor::NsResize,
        CursorIcon::ResizeEast => Cursor::EResize,
        CursorIcon::ResizeSouthEast => Cursor::SeResize,
        CursorIcon::ResizeSouth => Cursor::SResize,
        CursorIcon::ResizeSouthWest => Cursor::SwResize,
        CursorIcon::ResizeWest => Cursor::WResize,
        CursorIcon::ResizeNorthWest => Cursor::NwResize,
        CursorIcon::ResizeNorth => Cursor::NResize,
        CursorIcon::ResizeNorthEast => Cursor::NeResize,
        CursorIcon::ResizeColumn => Cursor::ColResize,
        CursorIcon::ResizeRow => Cursor::RowResize,
        CursorIcon::ZoomIn => Cursor::ZoomIn,
        CursorIcon::ZoomOut => Cursor::ZoomOut,
    }
}
