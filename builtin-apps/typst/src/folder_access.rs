//! The files beside a document: a picture it shows, a chapter it includes. WaOS gives a program
//! the file its user opens and nothing else of its folder, so while the text names a file there
//! that the program cannot reach, a bar above the document offers to give it the folder, in the
//! system's folder dialog; the pages are set again once it is given. WaOS keeps the folder among
//! the program's mounts, so the question is asked once for a folder.

use std::path::{Path, PathBuf};

use waft::ui::{FolderOptions, PickedFolder};
use waft::{
    App, BuildContext, EdgeInsetsGeometry, IntoWidget, Listener, Padding, SizedBox,
    StatelessWidget, WidgetRef,
};
use waft_winui::{Button, InfoBar, InfoBarSeverity};

use crate::documents_app::DocumentsApp;
use crate::notices::say;
use crate::pages::typeset_now;
use crate::window::main_view;

/// The bar that offers to give the document's folder, while the text needs it.
#[derive(Debug)]
pub(crate) struct FolderBar {
    pub(crate) documents: DocumentsApp,
}

impl StatelessWidget for FolderBar {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let Some(folder) = self.documents.pages.read(app).closed_folder() else {
            return SizedBox::new().into_widget();
        };
        let (folder, documents) = (folder.to_owned(), self.documents.clone());
        let message = format!(
            "This document uses files beside it, in “{}”, which Documents cannot read yet.",
            name_of(&folder)
        );
        let give = Button::text(
            "Give Access…",
            Listener::new(move |app: &mut App| give_folder(app, &documents, folder.clone())),
        );
        let bar = InfoBar::new(true, |_app, _open| {})
            .severity(InfoBarSeverity::Informational)
            .message(message)
            .is_closable(false)
            .action_button(give);
        Padding::new(EdgeInsetsGeometry::from_ltrb(8.0, 0.0, 8.0, 8.0))
            .child(bar)
            .into_widget()
    }
}

/// Asks for `folder` in the system's folder dialog, and sets the pages again once one is given.
fn give_folder(app: &mut App, documents: &DocumentsApp, folder: PathBuf) {
    let options = FolderOptions {
        initial_folder: Some(folder),
        confirm_label: Some("Give Access".to_owned()),
        writable: false,
    };
    let parent = main_view(app).map(|view| view.id());
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        return say(
            app,
            documents,
            InfoBarSeverity::Error,
            "This system has no folder dialog.",
        );
    };
    let picking = dialogs.open_folder(parent, options);
    let documents = documents.clone();
    app.spawn(async move |cx| match picking.await {
        Ok(Some(PickedFolder::Path(_))) => cx.update(|app| typeset_now(app, &documents)),
        Ok(_) => (),
        Err(error) => cx.update(|app| {
            let said = format!("The folder dialog failed: {error}");
            say(app, &documents, InfoBarSeverity::Error, said);
        }),
    });
}

fn name_of(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
