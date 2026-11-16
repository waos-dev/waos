//! A question the app asks in its window, as a WinUI `ContentDialog`: what it asks, why, the
//! button that does it, and Cancel, the default, so that Return never loses anything.

use waft_foundation::{App, Ref, Task};
use waft_widgets::{GlobalKey, Text};
use waft_winui::{ContentDialog, ContentDialogButton, ContentDialogResult};

/// What is asked: whether to do what `confirm` names, which loses something.
pub(crate) struct Question {
    pub(crate) title: String,
    pub(crate) message: String,
    /// The primary button's label.
    pub(crate) confirm: &'static str,
}

/// The close button's label, the default.
const CANCEL: &str = "Cancel";

/// Asks `question` over the window of the navigator `navigator` keeps, and answers whether the
/// user chose `confirm`; no where the navigator is not built yet, or another dialog is open.
pub(crate) fn ask(app: &mut App, navigator: &GlobalKey, question: Question) -> Task<bool> {
    let Some(context) = navigator.current_context(app) else {
        return Task::ready(false);
    };
    let dialog = dialog_asking(app, question);
    let Ok(shown) = dialog.show_async(app, context) else {
        dialog.dispose(app);
        return Task::ready(false);
    };
    app.spawn(async move |cx| {
        let result = shown.await;
        cx.update(|app| dialog.dispose(app));
        result == ContentDialogResult::Primary
    })
}

fn dialog_asking(app: &mut App, question: Question) -> Ref<ContentDialog> {
    let dialog = ContentDialog::new(app);
    dialog.title(app, Text::new(question.title));
    dialog.content(app, Text::new(question.message));
    dialog.primary_button_text(app, question.confirm);
    dialog.close_button_text(app, CANCEL);
    dialog.default_button(app, ContentDialogButton::Close);
    dialog
}
