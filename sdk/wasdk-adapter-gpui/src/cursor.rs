//! GPUI's cursor styles as wasdk's cursors, which are CSS's: each by what it means.

use gpui::CursorStyle;
use wasdk::views::Cursor;

pub(crate) fn cursor_of(style: CursorStyle) -> Cursor {
    match style {
        CursorStyle::Arrow => Cursor::Default,
        CursorStyle::IBeam => Cursor::Text,
        CursorStyle::IBeamCursorForVerticalLayout => Cursor::VerticalText,
        CursorStyle::Crosshair => Cursor::Crosshair,
        CursorStyle::ClosedHand => Cursor::Grabbing,
        CursorStyle::OpenHand => Cursor::Grab,
        CursorStyle::PointingHand => Cursor::Pointer,
        CursorStyle::ResizeLeft => Cursor::WResize,
        CursorStyle::ResizeRight => Cursor::EResize,
        CursorStyle::ResizeLeftRight => Cursor::EwResize,
        CursorStyle::ResizeUp => Cursor::NResize,
        CursorStyle::ResizeDown => Cursor::SResize,
        CursorStyle::ResizeUpDown => Cursor::NsResize,
        CursorStyle::ResizeUpLeftDownRight => Cursor::NwseResize,
        CursorStyle::ResizeUpRightDownLeft => Cursor::NeswResize,
        CursorStyle::ResizeColumn => Cursor::ColResize,
        CursorStyle::ResizeRow => Cursor::RowResize,
        CursorStyle::OperationNotAllowed => Cursor::NotAllowed,
        CursorStyle::DragLink => Cursor::Alias,
        CursorStyle::DragCopy => Cursor::Copy,
        CursorStyle::ContextualMenu => Cursor::ContextMenu,
    }
}
