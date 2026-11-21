//! The programs' windows: what goes in each, its view's canvas filling it and what the process the
//! windows are in shows over the program's window, as WaOS shows its questions over it, which
//! whoever builds the process's views puts in the window ([`ProgramWindows::content_of`]). A
//! window reserved for a program's first request, as one the platform restored or opened at the
//! program's launch, shows the program's name until its view does, with what is shown over its
//! windows once it has started, so its questions are put there.
//!
//! The invariant: each window's content reports every change of its window to the windows, which
//! hear of it from nowhere else while the app draws.

use std::fmt::{self, Debug};
use std::rc::Rc;

use waft::ui::{ViewId, ViewRef};
use waft::{
    AlignmentGeometry, App, BuildContext, Center, Directionality, Entity, IntoWidget, Listener,
    SizedBox, Stack, StackFit, StatelessWidget, Text, TextDirection, TextStyle, WidgetRef,
};

use crate::link::ProgramLink;
use crate::state::view_windows::ViewWindows;
use crate::ui::program_canvas::ProgramCanvas;
use crate::ui::window_changes::WindowChanges;

/// What is shown over each of a program's windows, above its canvas.
pub type OverWindow<P> = Rc<dyn Fn(&mut App, &P) -> WidgetRef>;

/// The programs' windows, and what is shown over each.
pub struct ProgramWindows<P: ProgramLink> {
    pub view_windows: Entity<ViewWindows<P>>,
    pub over_each: OverWindow<P>,
}

impl<P: ProgramLink> ProgramWindows<P> {
    /// What goes in the window of `view`, where it is a program's window: its program's view, once
    /// shown; none for any other view.
    pub fn content_of(&self, app: &App, view: &ViewRef) -> Option<WidgetRef> {
        self.view_windows.read(app).knows(view).then(|| {
            ProgramWindow {
                windows: self.clone(),
                view: view.id(),
            }
            .into_widget()
        })
    }

    /// The user asked to close the window of `view`: where it is a program's window, its program
    /// decides. Answers whether it was.
    pub fn close_requested(&self, app: &App, view: ViewId) -> bool {
        self.view_windows.read(app).close_requested(view, app)
    }
}

impl<P: ProgramLink> Clone for ProgramWindows<P> {
    fn clone(&self) -> ProgramWindows<P> {
        ProgramWindows {
            view_windows: self.view_windows.clone(),
            over_each: Rc::clone(&self.over_each),
        }
    }
}

impl<P: ProgramLink> Debug for ProgramWindows<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgramWindows")
            .field("view_windows", &self.view_windows)
            .finish_non_exhaustive()
    }
}

/// What is in one program's window: its program's view shown there, once it is, reporting each
/// change of the window to the windows.
struct ProgramWindow<P: ProgramLink> {
    windows: ProgramWindows<P>,
    view: ViewId,
}

impl<P: ProgramLink> Debug for ProgramWindow<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgramWindow")
            .field("view", &self.view.0)
            .finish_non_exhaustive()
    }
}

impl<P: ProgramLink> ProgramWindow<P> {
    /// The window while it waits for its program's first request: the program's name, with what
    /// is shown over the program's windows once it has started; nothing where it is no such
    /// window.
    fn reserved(&self, app: &mut App) -> WidgetRef {
        let given = self
            .windows
            .view_windows
            .read(app)
            .given_in(self.view)
            .map(|(face, program)| (face.name.clone(), program.cloned()));
        let Some((name, program)) = given else {
            return SizedBox::expand().into_widget();
        };
        let name = Text::new(name).style(TextStyle::new().font_size(17.0));
        // The window has no app's reading direction of its own; a name reads the same either way.
        let launch =
            Directionality::new(TextDirection::Ltr, Center::new().child(name)).into_widget();
        let over = match program {
            Some(program) => (self.windows.over_each)(app, &program),
            None => SizedBox::shrink().into_widget(),
        };
        // By the window's corners, as the program's canvas is placed.
        Stack::new()
            .alignment(AlignmentGeometry::TOP_LEFT)
            .fit(StackFit::Expand)
            .children([launch, over])
            .into_widget()
    }
}

impl<P: ProgramLink> StatelessWidget for ProgramWindow<P> {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let shown = self
            .windows
            .view_windows
            .read(app)
            .shown_in(self.view)
            .map(|window| (window.program.clone(), window.shown.clone()));
        let content = match shown {
            Some((program, shown)) => {
                let over = (self.windows.over_each)(app, &program);
                // By the window's corners: the canvas has no reading direction to place by.
                Stack::new()
                    .alignment(AlignmentGeometry::TOP_LEFT)
                    .fit(StackFit::Expand)
                    .children([ProgramCanvas::new(shown).into_widget(), over])
                    .into_widget()
            }
            None => self.reserved(app),
        };
        let (view_windows, view) = (self.windows.view_windows.clone(), self.view);
        let changed = Listener::new(move |app| {
            view_windows.update(app, |windows, cx| windows.window_changed(view, cx));
        });
        WindowChanges::new(changed, content).into_widget()
    }
}
