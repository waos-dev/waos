//! The list's pane, where the list is a pane over the note: closed over what a command shows, and
//! opened by Search, which moves the focus to the search field once the pane has opened, since a
//! pane opening over the note takes the focus itself.

use waft_foundation::App;

use crate::notes_app::NotesApp;

/// Closes the list's pane where it covers what is shown.
pub(crate) fn uncover(app: &mut App, notes: &NotesApp) {
    notes.browsing.update(app, |browsing, cx| {
        if browsing.narrow() && browsing.pane_open() {
            browsing.set_pane_open(false);
            cx.notify();
        }
    });
}

/// Moves the focus to the search field: at once where the list's pane is open; otherwise opens the
/// pane, and moves it once the pane has opened.
pub(crate) fn focus_search(app: &mut App, notes: &NotesApp) {
    if notes.browsing.read(app).pane_open() {
        return notes.search.take_focus(app);
    }
    notes.browsing.update(app, |browsing, cx| {
        browsing.set_pane_open(true);
        browsing.set_search_asked(true);
        cx.notify();
    });
}

/// The list's pane has opened: the search field takes the focus where Search asked for it.
pub(crate) fn pane_has_opened(app: &mut App, notes: &NotesApp) {
    let asked = notes.browsing.update(app, |browsing, _cx| {
        let asked = browsing.search_asked();
        browsing.set_search_asked(false);
        asked
    });
    if asked {
        notes.search.take_focus(app);
    }
}
