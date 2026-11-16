//! The question the app asks before it lets go of a document with changes its file does not
//! hold, as a WinUI `ContentDialog`: Save, the default, Discard, or Cancel.

use waft::{App, GlobalKey, Ref, Task, Text};
use waft_winui::{ContentDialog, ContentDialogButton, ContentDialogResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Save,
    Discard,
    Cancel,
}

/// Asks whether to save the changes to the document called `name`, over the window of the
/// navigator `navigator` keeps; Cancel where the navigator is not built yet, or another dialog is
/// open.
pub(crate) fn ask_to_save(app: &mut App, navigator: &GlobalKey, name: &str) -> Task<Answer> {
    let Some(context) = navigator.current_context(app) else {
        return Task::ready(Answer::Cancel);
    };
    let dialog = dialog_asking(app, name);
    let Ok(shown) = dialog.show_async(app, context) else {
        dialog.dispose(app);
        return Task::ready(Answer::Cancel);
    };
    app.spawn(async move |cx| {
        let result = shown.await;
        cx.update(|app| dialog.dispose(app));
        match result {
            ContentDialogResult::Primary => Answer::Save,
            ContentDialogResult::Secondary => Answer::Discard,
            ContentDialogResult::None => Answer::Cancel,
        }
    })
}

fn dialog_asking(app: &mut App, name: &str) -> Ref<ContentDialog> {
    let dialog = ContentDialog::new(app);
    dialog.title(app, Text::new(format!("Save the changes to “{name}”?")));
    dialog.content(
        app,
        Text::new("Your changes will be lost if you don't save them."),
    );
    dialog.primary_button_text(app, "Save");
    dialog.secondary_button_text(app, "Discard");
    dialog.close_button_text(app, "Cancel");
    dialog.default_button(app, ContentDialogButton::Primary);
    dialog
}
