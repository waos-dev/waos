//! Quitting: a request to end, ⌘Q, Quit in the Dock's menu or the window closing, is agreed to at
//! once when every note and file open is saved, as each change is saved as it is made. Where the
//! store could not keep a note, or a file could not be written, it tries again first, and where it
//! still cannot, or another app changed or removed a file under an edit, asks in a `ContentDialog`
//! whether to quit and lose the latest changes. A request while one is being answered waits for the
//! same answer.
//!
//! Closing the window quits, as Waft's `run_app_with` closes its last window, since WaOS gives a
//! program no way to be reopened with no window.

use std::cell::RefCell;
use std::rc::Rc;

use waft_embedder::{AppExitResponse, AppExitType};
use waft_foundation::{App, Completer, Task};
use waft_services::ServicesBinding;
use waft_widgets::AppLifecycleListener;

use crate::notes_app::NotesApp;
use crate::question::{Question, ask};
use crate::window::bring_forward;

/// The answer to a request to end, while it is being decided.
type QuitDecision = Rc<RefCell<Option<Completer<AppExitResponse>>>>;

/// From now on, a request to end the program is answered as the module says.
pub(crate) fn answer_quits(app: &mut App, notes: &NotesApp) {
    let notes = notes.clone();
    let deciding = QuitDecision::default();
    AppLifecycleListener::new(app).on_exit_requested(
        app,
        Rc::new(move |app: &mut App| answer_quit(app, &notes, &deciding)),
    );
}

/// Asks to quit, as ⌘Q does, so the request reaches [`answer_quits`]'s listener.
pub(crate) fn ask_to_quit(app: &mut App) {
    let services = ServicesBinding::instance(app);
    let _answer = services.exit_application(app, AppExitType::Cancelable, 0);
}

/// Answers a request to end: once every note is kept or let go, or with the answer of the request
/// already `deciding`.
fn answer_quit(app: &mut App, notes: &NotesApp, deciding: &QuitDecision) -> Task<AppExitResponse> {
    let ongoing = deciding.borrow().clone();
    let decision = ongoing.unwrap_or_else(|| decide_quit(app, notes, deciding));
    let decided = decision.future();
    app.spawn(async move |_cx| decided.await)
}

/// Settles the notes the store could not keep, and completes the decision it answers, which
/// `deciding` holds until then.
fn decide_quit(
    app: &mut App,
    notes: &NotesApp,
    deciding: &QuitDecision,
) -> Completer<AppExitResponse> {
    let decision = Completer::new();
    *deciding.borrow_mut() = Some(decision.clone());
    let settled = settle_unsaved(app, notes);
    let (deciding, decided) = (Rc::clone(deciding), decision.clone());
    app.spawn(async move |cx| {
        let response = settled.await;
        deciding.borrow_mut().take();
        cx.update(|app| decided.complete(app, response));
    });
    decision
}

/// `Exit` once every note and file is saved, trying again those that could not be; otherwise
/// asks, in the window brought forward, whether to quit all the same.
fn settle_unsaved(app: &mut App, notes: &NotesApp) -> Task<AppExitResponse> {
    notes.library.update(app, |library, cx| {
        library.save_unsaved();
        cx.notify();
    });
    notes.files.update(app, |files, cx| {
        files.save_unwritten();
        cx.notify();
    });
    let Some((title, why)) = first_unsaved(app, notes) else {
        return Task::ready(AppExitResponse::Exit);
    };
    let question = Question {
        title: format!("“{title}” could not be saved."),
        message: format!("Its latest changes will be lost if you quit: {why}."),
        confirm: "Quit Anyway",
    };
    bring_forward(app);
    let agreed = ask(app, &notes.navigator, question);
    app.spawn(async move |_cx| {
        if agreed.await {
            AppExitResponse::Exit
        } else {
            AppExitResponse::Cancel
        }
    })
}

/// The title of the first note, or the name of the first file, whose latest changes are not saved,
/// and why.
fn first_unsaved(app: &App, notes: &NotesApp) -> Option<(String, String)> {
    let unsaved_note = notes.library.read(app).unsaved();
    if let Some((note, why)) = unsaved_note {
        return Some((note.title().to_owned(), why.to_owned()));
    }
    let (file, why) = notes.files.read(app).unsaved()?;
    Some((file.name(), why.to_string()))
}
