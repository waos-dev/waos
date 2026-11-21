//! The programs' file dialogs on the main thread, over Waft's: each shown as a sheet on the
//! window of the program's view it names, or, where it names none, over the program's own screen,
//! its front window or the window reserved for its first request (decision 11 of
//! `docs/proposals/2026-11-21-views-a-program-asks-for.md`), never over a screen Waft would pick;
//! and answered with the paths the user chose, which the engine mounts. The dialogs are the
//! engine's shell's only where Waft's platform has them.
//!
//! The invariant: every dialog arriving is answered, with the paths chosen or with why none were,
//! so no program waits for an answer that cannot come.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::ui::{Platform, ViewId};
use waft::{App, Center, Directionality, Entity, IntoWidget, Text, TextDirection};
use waos::ShellFileDialogs;
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos_program_window::{PromptWindows, ShownPrompt, picked_file_paths, picked_folder_path};

use crate::logic::file_dialog_calls::{Dialog, DialogCall, FileDialogsOnRequest};
use crate::state::ViewWindows;

/// Waft's file dialogs, served to the engine as its shell's.
pub(crate) struct FileDialogs {
    calls: FileDialogsOnRequest,
    arriving: UnboundedReceiver<DialogCall>,
}

impl FileDialogs {
    /// Waft's file dialogs on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<FileDialogs> {
        platform.file_dialogs()?;
        let (calls, arriving) = FileDialogsOnRequest::new();
        Some(FileDialogs { calls, arriving })
    }

    /// The dialogs as the engine's shell has them: each goes to the main thread, where
    /// [`serve`](Self::serve) shows it.
    pub(crate) fn shell(&self) -> Arc<dyn ShellFileDialogs> {
        Arc::new(self.calls.clone())
    }

    /// From now on, each dialog a program asks for is shown through Waft's, on the window of
    /// the program's view in `view_windows` it names, or over the program's own screen; and, where
    /// `screens_of_their_own` places prompts in windows of their own, a phone's, over a screen
    /// opened for the dialog where the program has none, closed once the dialog is answered.
    pub(crate) fn serve(
        self,
        app: &mut App,
        view_windows: Entity<ViewWindows>,
        screens_of_their_own: Option<Rc<PromptWindows>>,
    ) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            while let Some(call) = arriving.next().await {
                let parent = call.parent_of();
                let program = call.program.clone();
                let DialogCall {
                    dialog, mut chosen, ..
                } = call;
                if chosen.is_canceled() {
                    continue;
                }
                let over = cx.update(|app| parent_view(app, &view_windows, &program, parent));
                let (over, screen_of_its_own) = match (over, &screens_of_their_own) {
                    (None, Some(prompt_windows)) => {
                        let (shown, arrived) =
                            cx.update(|app| screen_of_its_own(app, prompt_windows, &program));
                        (arrived.await.ok(), Some(shown))
                    }
                    (over, _) => (over, None),
                };
                let shown = cx.update(|app| show(app, over, dialog));
                cx.update(|app| {
                    app.spawn(async move |_cx| {
                        let mut shown = std::pin::pin!(shown);
                        let outcome =
                            match futures::future::select(shown.as_mut(), chosen.cancellation())
                                .await
                            {
                                futures::future::Either::Left((picked, _)) => Some(picked),
                                futures::future::Either::Right(((), _)) => None,
                            };
                        // The screen opened for the dialog closes once it is answered.
                        drop(screen_of_its_own);
                        if let Some(picked) = outcome {
                            // A program that ended meanwhile hears nothing.
                            let _ = chosen.send(picked);
                        }
                    })
                });
            }
        });
    }
}

impl DialogCall {
    /// The program and its view the dialog is a sheet on.
    fn parent_of(&self) -> Option<(waos::RunningProgram, u64)> {
        self.parent.map(|view| (self.program.clone(), view))
    }
}

/// The Waft view a dialog of `program` is shown over: the window of its view `parent` names, or
/// else its own screen; none where it has neither.
fn parent_view(
    app: &App,
    view_windows: &Entity<ViewWindows>,
    program: &waos::RunningProgram,
    parent: Option<(waos::RunningProgram, u64)>,
) -> Option<ViewId> {
    let windows = view_windows.read(app);
    let named = parent.and_then(|(program, view)| {
        let (waft_view, _ratio) = windows.waft_view_of(app, |shown, shown_view| {
            *shown == program && shown_view.id() == view
        })?;
        Some(waft_view)
    });
    named.or_else(|| windows.screen_of(program))
}

/// A screen opened for a dialog of `program`, which has none of its own: a window of its own
/// saying whose dialog it is, and what resolves with its view once it arrives.
fn screen_of_its_own(
    app: &mut App,
    prompt_windows: &PromptWindows,
    program: &waos::RunningProgram,
) -> (ShownPrompt, oneshot::Receiver<ViewId>) {
    let name = Center::new().child(Text::new(program.name().to_owned()));
    // A window of its own has no app's reading direction; a name reads the same either way.
    let words = Directionality::new(TextDirection::Ltr, name).into_widget();
    prompt_windows.show_on_a_screen_of_its_own(app, program, words, Rc::new(|_app| {}))
}

/// Shows `dialog` through Waft's, over the view `parent`, and answers the paths chosen.
fn show(
    app: &mut App,
    parent: Option<ViewId>,
    dialog: Dialog,
) -> impl Future<Output = Result<Vec<PathBuf>, DialogError>> + use<> {
    let platform = app.platform();
    let shown = platform.file_dialogs().map(|dialogs| match dialog {
        Dialog::OpenFile(options) => picked_file_paths(dialogs.open_file(parent, options)),
        Dialog::OpenFiles(options) => picked_file_paths(dialogs.open_files(parent, options)),
        Dialog::OpenFolder(options) => picked_folder_path(dialogs.open_folder(parent, options)),
        Dialog::SaveFile(options) => picked_file_paths(dialogs.save_file(parent, options)),
    });
    async move {
        let Some(shown) = shown else {
            return Err(DialogError::Failed("no file dialogs".to_owned()));
        };
        shown.await
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::time::Duration;

    use futures::FutureExt;
    use futures::channel::oneshot;
    use waft::Shell;
    use waft::ui::{
        EmbedderClient, FileDialogFuture as WaftDialogFuture, FileDialogs as WaftFileDialogs,
        FolderOptions as WaftFolderOptions, OpenOptions as WaftOpenOptions, PickedFile,
        PickedFolder, SaveOptions as WaftSaveOptions,
    };
    use waft_test::TestPlatform;
    use waos::OpenOptions;
    use waos_test_programs::CODE_CACHE;

    use super::*;
    use crate::logic::placement::WindowsOnRequest;
    use crate::logic::program_faces::ProgramFaces;
    use crate::logic::test_engine;

    #[derive(Clone, Default)]
    struct PendingWaftDialogs {
        held: Rc<RefCell<Vec<oneshot::Sender<Option<PickedFile>>>>>,
    }

    impl PendingWaftDialogs {
        fn cancelled(&self, index: usize) -> bool {
            self.held
                .borrow()
                .get(index)
                .is_some_and(oneshot::Sender::is_canceled)
        }
    }

    impl WaftFileDialogs for PendingWaftDialogs {
        fn open_file(
            &self,
            _parent: Option<ViewId>,
            _options: WaftOpenOptions,
        ) -> WaftDialogFuture<Option<PickedFile>> {
            let (tx, rx) = oneshot::channel();
            self.held.borrow_mut().push(tx);
            Box::pin(rx.map(|picked| Ok(picked.unwrap_or(None))))
        }

        fn open_files(
            &self,
            _parent: Option<ViewId>,
            _options: WaftOpenOptions,
        ) -> WaftDialogFuture<Vec<PickedFile>> {
            Box::pin(std::future::ready(Ok(Vec::new())))
        }

        fn open_folder(
            &self,
            _parent: Option<ViewId>,
            _options: WaftFolderOptions,
        ) -> WaftDialogFuture<Option<PickedFolder>> {
            Box::pin(std::future::ready(Ok(None)))
        }

        fn save_file(
            &self,
            _parent: Option<ViewId>,
            _options: WaftSaveOptions,
        ) -> WaftDialogFuture<Option<PickedFile>> {
            Box::pin(std::future::ready(Ok(None)))
        }
    }

    #[test]
    fn dropping_a_file_dialogs_future_drops_wafts_dialog_future() {
        let waft_dialogs = PendingWaftDialogs::default();
        let platform =
            Rc::new(TestPlatform::new().with_file_dialogs(Rc::new(waft_dialogs.clone())));
        let folder = tempfile::tempdir().unwrap();
        let store = waos_store::Store::open(folder.path(), waos_store::SourceProxy::None).unwrap();
        let (placement, requests) = WindowsOnRequest::new(ProgramFaces::new(store, None));
        let engine = test_engine::engine(Arc::new(placement), Path::new(CODE_CACHE));
        let program = engine
            .load(Path::new(waos_test_programs::FOREVER))
            .expect("forever loads");
        let running = engine
            .run(&program, Default::default())
            .expect("forever runs");

        let served = Rc::new(RefCell::new(None));
        let kept = Rc::clone(&served);
        let mut shell = Shell::new(platform, move |app| {
            let dialogs = FileDialogs::of(&*app.platform()).expect("waft file dialogs");
            let windows = ViewWindows::listen(app, requests);
            *kept.borrow_mut() = Some(dialogs.shell());
            dialogs.serve(app, windows, None);
        });
        let dialogs_shell = served.borrow_mut().take().unwrap();
        let future = dialogs_shell.open_file(&running, None, OpenOptions::default());
        wake_until("waft dialog opens", &mut shell, || {
            waft_dialogs.held.borrow().len() == 1
        });
        assert!(!waft_dialogs.cancelled(0));

        drop(future);
        wake_until("waft dialog future is dropped", &mut shell, || {
            waft_dialogs.cancelled(0)
        });
        running.stop();
    }

    fn wake_until(what: &str, shell: &mut Shell, ready: impl Fn() -> bool) {
        for _ in 0..100 {
            if ready() {
                return;
            }
            shell.wake(Duration::ZERO);
        }
        panic!("{what} never happened");
    }
}
