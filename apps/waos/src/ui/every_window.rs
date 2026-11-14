//! Every view WaOS shows, each built here from what its view is, as the platform hosts it: a
//! program's window, with the prompt its program asks its user through over it; a prompt's window
//! of its own, for a program with no window; on macOS, a prompt's view on a layer in a dialog of
//! the program's app; and WaOS's own, the library's: the desktop's launcher, whose view carries
//! the launcher's initial data, or the deck, whose view carries none. A view that is none of them
//! shows nothing.
//!
//! The invariant: a user's request to close a window goes to what the window shows, a program's
//! to its program, a prompt's dismissing it and the library's to what its place says.

use std::rc::Rc;

use waft::Entity;
use waft::ui::ViewRef;
use waft::{App, IntoWidget, MultiViewApp, ViewCloseHandler, WidgetRef};
use waos::RunningProgram;
use waos_program_window::{ProgramWindows, PromptWindows};

use crate::state::ViewWindows;

/// What goes in a view, where it is one of a kind of WaOS's views; none for any other.
pub(crate) type ViewContent = Rc<dyn Fn(&App, &ViewRef) -> Option<WidgetRef>>;

/// What WaOS's own window shows, the library, what a request to close it does, and the initial
/// data its view carries, which names it.
pub(crate) struct LibraryWindow {
    pub(crate) content: WidgetRef,
    pub(crate) close_requested: ViewCloseHandler,
    pub(crate) initial_data: Option<&'static str>,
}

/// The views: the library's, where `library` says what it shows, the programs' of
/// `program_windows`, the prompts' windows placed by `prompt_windows`, and the prompts' views in
/// the programs' apps, where `prompt_views` builds any.
pub(crate) fn every_window(
    library: Option<LibraryWindow>,
    program_windows: ProgramWindows<RunningProgram>,
    prompt_windows: Rc<PromptWindows>,
    prompt_views: Option<ViewContent>,
) -> WidgetRef {
    let windows = Rc::new(EveryWindow {
        library,
        program_windows,
        prompt_windows,
        prompt_views,
    });
    let closing = Rc::clone(&windows);
    MultiViewApp::new(move |app, view| windows.content_of(app, view))
        .on_close_requested(Rc::new(move |app, view| closing.close_requested(app, view)))
        .into_widget()
}

/// The programs' windows among `view_windows`, each with its program's prompt from
/// `prompt_windows` over it.
pub(crate) fn program_windows(
    view_windows: &Entity<ViewWindows>,
    prompt_windows: &PromptWindows,
) -> ProgramWindows<RunningProgram> {
    ProgramWindows {
        view_windows: view_windows.clone(),
        over_each: prompt_windows.over_window(),
    }
}

struct EveryWindow {
    library: Option<LibraryWindow>,
    program_windows: ProgramWindows<RunningProgram>,
    prompt_windows: Rc<PromptWindows>,
    prompt_views: Option<ViewContent>,
}

impl EveryWindow {
    /// What goes in the window of `view`.
    fn content_of(&self, app: &mut App, view: &ViewRef) -> Option<WidgetRef> {
        self.program_windows
            .content_of(app, view)
            .or_else(|| self.prompt_windows.content_of(app, view))
            .or_else(|| {
                let prompt_views = self.prompt_views.as_ref()?;
                prompt_views(app, view)
            })
            .or_else(|| self.library_content_of(view))
    }

    /// The library, where `view` is WaOS's own, carrying the library window's initial data.
    fn library_content_of(&self, view: &ViewRef) -> Option<WidgetRef> {
        let library = self.library.as_ref()?;
        (view.initial_data().as_deref() == library.initial_data).then(|| library.content.clone())
    }

    /// The user asked to close the window of `view`, one built here.
    fn close_requested(&self, app: &mut App, view: &ViewRef) {
        if self.program_windows.close_requested(app, view.id())
            || self.prompt_windows.close_requested(app, view.id())
        {
            return;
        }
        if let Some(library) = &self.library {
            (library.close_requested)(app, view);
        }
    }
}
