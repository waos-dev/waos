//! A file another app changed or removed while Notes has it open. WaOS tells a program nothing of
//! such a change, so Notes looks: before each save (`open_file`), as a file is shown (`showing`),
//! and as the app comes back to the front, the lifecycle's resume, when it looks at every file
//! open. One changed while it held no edit of Notes' is read again; one changed under an edit, or
//! removed, waits in a bar above it for the user (`notices`), with Save Anyway or Revert, Save Again
//! or Close File.

use std::path::PathBuf;
use std::rc::Rc;

use waft_foundation::App;
use waft_widgets::AppLifecycleListener;
use waft_winui::InfoBarSeverity;

use crate::notes_app::NotesApp;
use crate::showing::reread;

/// From now on, every file open is looked at again as the app comes back to the front.
pub(crate) fn look_again_on_resume(app: &mut App, notes: &NotesApp) {
    let notes = notes.clone();
    AppLifecycleListener::new(app).on_resume(app, Rc::new(move |app| look_again(app, &notes)));
}

/// Writes the file shown over whatever is at its path now, as the user chose.
pub(crate) fn save_shown_anyway(app: &mut App, notes: &NotesApp) {
    let Some(path) = shown_file(app, notes) else {
        return;
    };
    notes.files.update(app, |files, cx| {
        files.save_anyway(&path);
        cx.notify();
    });
}

/// Reads the file shown again, letting go of the edits not written to it.
pub(crate) fn revert_shown(app: &mut App, notes: &NotesApp) {
    let Some(path) = shown_file(app, notes) else {
        return;
    };
    let reverted = notes.files.update(app, |files, cx| {
        cx.notify();
        files.revert(&path)
    });
    match reverted {
        Ok(()) => reread(app, notes, &path),
        Err(error) => {
            let said = format!("The file could not be read again: {error}.");
            notes.say(app, InfoBarSeverity::Error, said);
        }
    }
}

/// Looks at every file open again, and gives the editor the text of the one shown where it was
/// read again.
fn look_again(app: &mut App, notes: &NotesApp) {
    let reread_paths = notes.files.update(app, |files, cx| {
        cx.notify();
        files.look_again()
    });
    for path in reread_paths {
        reread(app, notes, &path);
    }
}

fn shown_file(app: &App, notes: &NotesApp) -> Option<PathBuf> {
    notes.browsing.read(app).shown_file().map(PathBuf::from)
}
