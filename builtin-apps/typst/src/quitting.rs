//! Quitting: a request to end, ⌘Q, Quit in the Dock's menu or the window closing, is agreed to
//! at once while the document has no unsaved change; otherwise the window, brought forward,
//! asks whether to save it, and the program ends once it is saved or its changes are let go. A
//! request while one is being answered waits for the same answer.
//!
//! Closing the window quits, as Waft's `run_app_with` closes its last window, since WaOS gives
//! a program no way to be reopened with no window.

use std::cell::RefCell;
use std::rc::Rc;

use waft::ui::{AppExitResponse, AppExitType};
use waft::{App, AppLifecycleListener, Completer, Task};
use waft_services::ServicesBinding;

use crate::documents_app::DocumentsApp;
use crate::filing::settle_edits;
use crate::window::bring_forward;

/// The answer to a request to end, while it is being decided.
type QuitDecision = Rc<RefCell<Option<Completer<AppExitResponse>>>>;

/// From now on, a request to end the program is answered as the module says.
pub(crate) fn answer_quits(app: &mut App, documents: &DocumentsApp) {
    let documents = documents.clone();
    let deciding = QuitDecision::default();
    AppLifecycleListener::new(app).on_exit_requested(
        app,
        Rc::new(move |app: &mut App| answer_quit(app, &documents, &deciding)),
    );
}

/// Asks to quit, as ⌘Q does, so the request reaches [`answer_quits`]'s listener.
pub(crate) fn ask_to_quit(app: &mut App, _documents: &DocumentsApp) {
    let services = ServicesBinding::instance(app);
    let _answer = services.exit_application(app, AppExitType::Cancelable, 0);
}

/// Answers a request to end: once the document is saved or let go, or with the answer of the
/// request already `deciding`.
fn answer_quit(
    app: &mut App,
    documents: &DocumentsApp,
    deciding: &QuitDecision,
) -> Task<AppExitResponse> {
    let ongoing = deciding.borrow().clone();
    let decision = ongoing.unwrap_or_else(|| decide_quit(app, documents, deciding));
    let decided = decision.future();
    app.spawn(async move |_cx| decided.await)
}

/// Settles the document's unsaved changes, and completes the decision that answers, which
/// `deciding` holds until then.
fn decide_quit(
    app: &mut App,
    documents: &DocumentsApp,
    deciding: &QuitDecision,
) -> Completer<AppExitResponse> {
    let decision = Completer::new();
    *deciding.borrow_mut() = Some(decision.clone());
    if documents.document.read(app).is_edited() {
        bring_forward(app);
    }
    let settled = settle_edits(app, documents);
    let (deciding, decided) = (Rc::clone(deciding), decision.clone());
    app.spawn(async move |cx| {
        let response = if settled.await {
            AppExitResponse::Exit
        } else {
            AppExitResponse::Cancel
        };
        deciding.borrow_mut().take();
        cx.update(|app| decided.complete(app, response));
    });
    decision
}
