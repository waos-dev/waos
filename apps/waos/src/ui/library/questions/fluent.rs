//! What the library asks or says, on a desktop: WinUI's content dialog over the panel: an install
//! question with the program's icon over its title, its version, file and publisher, Install as
//! the default button and Don't install; a remove question with Remove and Keep; a problem with
//! OK. Escape answers as the dismissing button does.
//!
//! The invariant: the dialog shows while the library says something and hides once it says
//! nothing, `FluentAsked` setting it to what is said after each frame that saw a change; its
//! buttons answer through the library and keep it open, so the next thing said takes its place.

use std::sync::Arc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, Entity, FrameCallback, Handle, IntoWidget,
    MainAxisSize, SchedulerBinding, State, StateData, StatefulWidget, Text, WidgetRef,
};
use waft_winui::{ContentDialog, ContentDialogButton};

use super::{Said, confirm, dismiss, said_now};
use crate::state::library::Library;
use crate::ui::library::program_icon::ProgramIcon;

/// The side of the program's icon over an install question.
const ICON_SIDE: f64 = 64.0;

/// `child`, with what the library asks or says shown as a dialog over the window.
#[derive(Debug)]
pub(crate) struct FluentAsked {
    pub(crate) library: Entity<Library>,
    pub(crate) child: WidgetRef,
}

impl StatefulWidget for FluentAsked {
    type State = FluentAskedState;

    fn create_state(&self) -> FluentAskedState {
        FluentAskedState {
            state: StateData::new(),
            dialog: None,
            showing: false,
        }
    }
}

pub(crate) struct FluentAskedState {
    state: StateData<FluentAsked>,
    dialog: Option<Handle<ContentDialog>>,
    /// Whether the dialog is shown, from its showing until it has closed.
    showing: bool,
}

impl State for FluentAskedState {
    type Widget = FluentAsked;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let library = self.widget(app).library.clone();
        let (yes, no) = (library.clone(), library);
        let dialog = ContentDialog::new(app)
            .primary_button_click(app, move |app, args| {
                args.set_cancel(true);
                confirm(&yes, app);
            })
            .close_button_click(app, move |app, args| {
                args.set_cancel(true);
                dismiss(&no, app);
            });
        app.get_mut(self).dialog = Some(dialog);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(dialog) = app.get_mut(self).dialog.take() {
            dialog.dispose(app);
        }
    }

    /// Reads the library, so a change of what is said rebuilds, and the dialog follows after the
    /// frame.
    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let _reads = said_now(self.widget(app).library.read(app));
        SchedulerBinding::add_post_frame_callback(
            app,
            FrameCallback::new(move |app, _| self.follow(app, context)),
            None,
        );
        self.widget(app).child.clone()
    }
}

impl FluentAskedState {
    /// Sets the dialog to what the library says and shows it, or hides it once it says nothing.
    fn follow(self: Handle<Self>, app: &mut App, context: BuildContext) {
        if !self.mounted(app) {
            return;
        }
        let Some(dialog) = app.get(self).dialog else {
            return;
        };
        let said = said_now(self.widget(app).library.read(app));
        let showing = app.get(self).showing;
        match said {
            Some(said) => {
                set_to(dialog, &said, app);
                if !showing {
                    self.show(app, context, dialog);
                }
            }
            None if showing => dialog.hide(app),
            None => {}
        }
    }

    fn show(
        self: Handle<Self>,
        app: &mut App,
        context: BuildContext,
        dialog: Handle<ContentDialog>,
    ) {
        match dialog.show_async(app, context) {
            Ok(closed) => {
                app.get_mut(self).showing = true;
                closed.then(app, move |app, _| {
                    if self.mounted(app) {
                        app.get_mut(self).showing = false;
                        // Something said meanwhile, as the dialog closed, shows it again.
                        self.follow(app, context);
                    }
                });
            }
            Err(error) => tracing::warn!(?error, "the library's dialog could not be shown"),
        }
    }
}

/// Sets `dialog`'s title, message and buttons to what `said` says.
fn set_to(dialog: Handle<ContentDialog>, said: &Said, app: &mut App) {
    let title = title(said);
    let (confirm, default) = match &said.confirm {
        Some(confirm) if confirm.destructive => (confirm.label, ContentDialogButton::Close),
        Some(confirm) => (confirm.label, ContentDialogButton::Primary),
        None => ("", ContentDialogButton::Close),
    };
    dialog
        .title(app, title)
        .content(app, Text::new(said.message.clone()))
        .primary_button_text(app, confirm)
        .close_button_text(app, said.dismiss)
        .default_button(app, default);
}

/// The dialog's title, under the program's icon where it has one.
fn title(said: &Said) -> WidgetRef {
    let text = Text::new(said.title.clone()).into_widget();
    let Some(png) = &said.icon else {
        return text;
    };
    let icon = ProgramIcon {
        png: Some(Arc::clone(png)),
        side: ICON_SIDE,
        badge: None,
    };
    Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(12.0)
        .children([icon.into_widget(), text])
        .into_widget()
}
