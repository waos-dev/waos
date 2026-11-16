//! What the library asks or says, on a phone: a `CupertinoAlertDialog` over everything under the
//! navigator it is shown from, the deck's, so over the home or a program's tab alike: an install
//! question with the program's icon over its title, its version, file and publisher, Don't install
//! and Install; a remove question with Keep and Remove in the destructive colour; a problem with
//! OK. Neither the barrier nor the system's Back dismisses it: a question is answered.
//!
//! The invariant: the dialog's route is pushed once something is said and removed once nothing
//! is, by `CupertinoAsked` after the frame that saw the change; the dialog itself reads what is
//! said each time it builds.

use std::rc::Rc;

use waft::{
    App, BuildContext, Column, Entity, FrameCallback, Handle, IntoWidget, Listener, MainAxisSize,
    Navigator, OptionRetained, PopScope, Ref, Route, SchedulerBinding, SizedBox, State, StateData,
    StatefulWidget, StatelessWidget, Text, WidgetRef,
};
use waft_cupertino::{CupertinoAlertDialog, CupertinoDialogAction, CupertinoDialogRoute};

use super::{Said, confirm, dismiss, said_now};
use crate::state::library::Library;
use crate::ui::library::program_icon::ProgramIcon;

/// The side of the program's icon over an install question.
const ICON_SIDE: f64 = 60.0;

/// `child`, with what the library asks or says shown as an alert over it, and over all else under
/// the root navigator.
#[derive(Debug)]
pub(crate) struct CupertinoAsked {
    pub(crate) library: Entity<Library>,
    pub(crate) child: WidgetRef,
}

impl StatefulWidget for CupertinoAsked {
    type State = CupertinoAskedState;

    fn create_state(&self) -> CupertinoAskedState {
        CupertinoAskedState {
            state: StateData::new(),
            route: None,
        }
    }
}

pub(crate) struct CupertinoAskedState {
    state: StateData<CupertinoAsked>,
    /// The alert's route, while it is on the navigator.
    route: Option<Ref<CupertinoDialogRoute>>,
}

impl State for CupertinoAskedState {
    type Widget = CupertinoAsked;
    waft_widgets::state_accessors!();

    /// Reads the library, so a change of what is said rebuilds, and the alert follows after the
    /// frame.
    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let says = said_now(self.widget(app).library.read(app)).is_some();
        if says != app.get(self).route.is_some() {
            SchedulerBinding::add_post_frame_callback(
                app,
                FrameCallback::new(move |app, _| self.follow(app, context)),
                None,
            );
        }
        self.widget(app).child.clone()
    }
}

impl CupertinoAskedState {
    /// Shows the alert while the library says something, and takes it away once it says nothing.
    fn follow(self: Handle<Self>, app: &mut App, context: BuildContext) {
        if !self.mounted(app) {
            return;
        }
        let library = self.widget(app).library.clone();
        let says = said_now(library.read(app)).is_some();
        match (says, app.get(self).route.is_some()) {
            (true, false) => self.show(app, context, library),
            (false, true) => {
                let route = app.get_mut(self).route.take().expect("on the navigator");
                let navigator = Navigator::of(app, context, true);
                navigator.remove_route(app, route.as_route(), None);
            }
            _ => {}
        }
    }

    fn show(self: Handle<Self>, app: &mut App, context: BuildContext, library: Entity<Library>) {
        let alert = AskedAlert { library };
        let route = CupertinoDialogRoute::new(
            app,
            Rc::new(move |_app, _context| alert.clone().into_widget()),
            context,
        )
        .barrier_dismissible(app, false);
        let pushed = route.handle();
        let navigator = Navigator::of(app, context, true);
        let popped = navigator.push(app, route.erase());
        app.get_mut(self).route = Some(route);
        popped.then(app, move |app, _| {
            if self.mounted(app) && app.get(self).route.handle() == Some(pushed) {
                app.get_mut(self).route = None;
            }
        });
    }
}

/// The alert, saying what the library says now.
#[derive(Clone, Debug)]
struct AskedAlert {
    library: Entity<Library>,
}

impl StatelessWidget for AskedAlert {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let Some(said) = said_now(self.library.read(app)) else {
            return SizedBox::shrink().into_widget();
        };
        let title = title(&said);
        let mut actions = vec![
            CupertinoDialogAction::new(Text::new(said.dismiss))
                .on_pressed(self.answer(dismiss))
                .into_widget(),
        ];
        if let Some(yes) = &said.confirm {
            actions.push(
                CupertinoDialogAction::new(Text::new(yes.label))
                    .on_pressed(self.answer(confirm))
                    .is_default_action(!yes.destructive)
                    .is_destructive_action(yes.destructive)
                    .into_widget(),
            );
        }
        let alert = CupertinoAlertDialog::new()
            .title(title)
            .content(Text::new(said.message))
            .actions(actions);
        PopScope::new(alert).can_pop(false).into_widget()
    }
}

impl AskedAlert {
    fn answer(&self, answer: fn(&Entity<Library>, &mut App)) -> Listener {
        let library = self.library.clone();
        Listener::new(move |app| answer(&library, app))
    }
}

/// The alert's title, under the program's icon where it has one.
fn title(said: &Said) -> WidgetRef {
    let text = Text::new(said.title.clone()).into_widget();
    let Some(png) = &said.icon else {
        return text;
    };
    let icon = ProgramIcon {
        png: Some(std::sync::Arc::clone(png)),
        side: ICON_SIDE,
        badge: None,
    };
    Column::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(10.0)
        .children([icon.into_widget(), text])
        .into_widget()
}
