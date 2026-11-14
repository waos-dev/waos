//! The views the platform hosts for WaOS on a phone that no one here asked for, told apart by
//! their initial data (`RestorableWindow`): WaOS's own, the launcher's on Android or the window of
//! a phone's size on a desktop, where the deck is; a program's first window, which Android brought
//! back from Recents to a WaOS started afresh, which starts its program again with that window as
//! its first; and a program's extra window brought back so, which closes at once, having no
//! program state behind it (decision 10 of `docs/proposals/2026-11-12-phone-programs-and-library.md`).
//! A window a running program asked for is its windows' (`ViewWindows`). What each starts is here,
//! as each arrives; what is built in each is `ui::every_window`'s.
//!
//! The invariant: each view is handled once, as it arrives, the views there at start before the
//! deck is built and every later one as Waft's binding tells its observers of it, after the
//! programs' windows heard of it, so a window a program asked for is known to be theirs.

use std::rc::Rc;

use waft::ui::ViewRef;
use waft::{App, Entity, WidgetsBinding, WidgetsBindingObserver};
use waos_package::ProgramId;
use waos_program_window::{RestorableWindow, WindowRole};

use super::ViewWindows;
use super::deck::tabs::Tabs;
use super::library::Library;

/// What a view the platform hosts for WaOS, and no running program asked for, is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum GivenView {
    /// WaOS's own, carrying no initial data: where the deck is.
    Deck,
    /// The first window of the installed program `id`.
    ProgramsFirst(ProgramId),
    /// A program's extra window, or one that names no program WaOS could install.
    ProgramsExtra,
    /// Another of WaOS's windows, which its own state answers for.
    Other,
}

/// What `view` is, by its initial data.
fn given_view_of(view: &ViewRef) -> GivenView {
    let Some(data) = view.initial_data() else {
        return GivenView::Deck;
    };
    let Some(window) = RestorableWindow::of(&data) else {
        return GivenView::Other;
    };
    match (window.role, window.program.parse::<ProgramId>()) {
        (WindowRole::First, Ok(id)) => GivenView::ProgramsFirst(id),
        (WindowRole::First, Err(_)) | (WindowRole::Extra, _) => GivenView::ProgramsExtra,
    }
}

/// From now on each view the platform gives is handled as it is: the deck of `tabs` is shown in
/// WaOS's own, `library` starts the program of a program's first window with it, kept in
/// `windows`, and a program's extra window closes; the views given already are handled now.
pub(crate) fn follow_given_views(
    app: &mut App,
    library: &Entity<Library>,
    tabs: &Entity<Tabs>,
    windows: &Entity<ViewWindows>,
) {
    let given = Rc::new(GivenViews {
        library: library.clone(),
        tabs: tabs.clone(),
        windows: windows.clone(),
    });
    for view in app.platform().views() {
        given.arrived(app, &view);
    }
    WidgetsBinding::instance(app).add_observer(app, given);
}

/// What handles each view the platform gives.
struct GivenViews {
    library: Entity<Library>,
    tabs: Entity<Tabs>,
    windows: Entity<ViewWindows>,
}

impl GivenViews {
    fn arrived(&self, app: &mut App, view: &ViewRef) {
        if self.windows.read(app).knows(view) {
            return;
        }
        let given = given_view_of(view);
        tracing::info!(view = view.id().0, ?given, "the platform gave a view");
        match given {
            GivenView::Deck => {
                let view = Rc::clone(view);
                self.tabs.update(app, |tabs, _| tabs.shown_in(view));
            }
            GivenView::ProgramsFirst(id) => {
                let (view, windows) = (Rc::clone(view), self.windows.clone());
                self.library.update(app, |library, cx| {
                    library.open_restored(&id, view, &windows, cx);
                });
            }
            GivenView::ProgramsExtra => {
                if let Some(window) = view.window() {
                    window.close();
                }
            }
            GivenView::Other => {}
        }
    }
}

impl WidgetsBindingObserver for GivenViews {
    fn did_add_view(&self, app: &mut App, view: &ViewRef) {
        self.arrived(app, view);
    }
}
