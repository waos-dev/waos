//! Copy, Paste and Select All on a session: the selection's text goes to the clipboard, the
//! clipboard's text is typed as pasted text, and everything the buffer holds is selected.

use waft_foundation::{App, Entity};
use waft_services::{Clipboard, ClipboardData};

use crate::paste::pasted_bytes;
use crate::screen::BufferPoint;
use crate::selection::{CellRange, Selection};
use crate::session::{Session, type_bytes};

/// Copies the selected text; nothing while nothing is selected, so the clipboard keeps what it
/// held.
pub(crate) fn copy_selection(app: &mut App, session: &Entity<Session>) {
    let text = session.update(app, |session, _cx| {
        let selection = session.selection?;
        Some(
            session
                .screen
                .text_between(selection.start(), selection.end()),
        )
    });
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        // The task ends by itself: nothing is done once the clipboard has the text.
        let _copying = Clipboard::set_data(app, ClipboardData::new(text));
    }
}

/// Types the clipboard's text into the shell, marked as pasted where the shell asked.
pub(crate) fn paste(app: &mut App, session: &Entity<Session>) {
    let reading = Clipboard::get_data(app, Clipboard::K_TEXT_PLAIN);
    let session = session.downgrade();
    app.spawn(async move |cx| {
        let Some(text) = reading.await.and_then(|data| data.text) else {
            return;
        };
        cx.update(|app| {
            let Some(session) = session.upgrade() else {
                return;
            };
            let bracketed = session.read(app).screen.brackets_paste();
            type_bytes(app, &session, &pasted_bytes(&text, bracketed));
        });
    });
}

/// Selects everything the buffer holds, the scrollback with the screen.
pub(crate) fn select_all(app: &mut App, session: &Entity<Session>) {
    session.update(app, |session, cx| {
        let screen = &session.screen;
        let whole = CellRange {
            first: BufferPoint {
                line: screen.first_line(),
                column: 0,
            },
            last: BufferPoint {
                line: screen.last_line(),
                column: screen.size().columns - 1,
            },
        };
        session.selection = Some(Selection::of(whole));
        cx.notify();
    });
}
