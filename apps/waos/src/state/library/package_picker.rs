//! Installing a package from a file the user picks: the system's file picker, through Waft's,
//! offering WaOS's packages where the system filters by type (`logic::opened_package`), on the
//! window of the view the library shows in. The file picked goes where a file opened with WaOS
//! goes: a path is opened as it is, and a document without one, as Android's picker hands it,
//! copied first (`Library::open_documents`); then the library asks whether to install it.
//!
//! The invariant: a pick reaches the library exactly as a file opened with WaOS does, so nothing
//! here opens, verifies or installs a package; a cancelled pick changes nothing.

use waft::ui::{FileDialogError, OpenOptions, PickedFile, ViewId};
use waft::{App, Context};

use super::{Library, OpenedFiles};
use crate::logic::opened_package::package_file_type;

/// Whether the system has a file picker to install a package from.
pub(crate) fn picks_packages(app: &App) -> bool {
    app.platform().file_dialogs().is_some()
}

impl Library {
    /// Shows the system's file picker for a package to install, on the window of `parent`; the
    /// package picked is asked about as one opened with WaOS. A picker the system does not show is
    /// the library's problem.
    pub(crate) fn pick_package(&mut self, parent: Option<ViewId>, cx: &mut Context<Library>) {
        let options = OpenOptions {
            types: vec![package_file_type()],
            ..OpenOptions::default()
        };
        let platform = cx.platform();
        let Some(picked) = platform
            .file_dialogs()
            .map(|dialogs| dialogs.open_file(parent, options))
        else {
            return self.failed("this system has no file picker".to_owned(), cx);
        };
        let library = cx.weak_entity();
        // The task runs to its end whether or not its handle is kept.
        let _picking = cx.spawn(async move |cx| {
            let picked = picked.await;
            cx.update(|app: &mut App| {
                if let Some(library) = library.upgrade() {
                    library.update(app, |library, cx| library.package_picked(picked, cx));
                }
            });
        });
    }

    /// What the picker answered: the file picked, taken as one opened with WaOS; nothing for a
    /// cancelled pick; why not, for a picker that failed.
    fn package_picked(
        &mut self,
        picked: Result<Option<PickedFile>, FileDialogError>,
        cx: &mut Context<Library>,
    ) {
        let file = match picked {
            Ok(Some(file)) => file,
            Ok(None) => return,
            Err(error) => return self.failed(error.to_string(), cx),
        };
        tracing::info!(?file, "the user picked a package to install");
        let OpenedFiles { paths, documents } = OpenedFiles::of(vec![file]);
        if !paths.is_empty() {
            self.open_files(paths, cx);
        }
        self.open_documents(documents, cx);
    }
}
