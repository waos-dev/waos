//! A button above the note: a subtle Fluent button showing an icon, named by its tooltip, which
//! does a command.

use waft_foundation::{App, Listener};
use waft_widgets::{IntoWidget, WidgetRef};
use waft_winui::{Button, ButtonStyle, FontIcon, ToolTip, ToolTipService};

use crate::notes_app::NotesApp;

/// A button showing `icon`, named `name`, which does `command` on `notes`.
pub(crate) fn icon_button(
    icon: FontIcon,
    name: &str,
    notes: &NotesApp,
    command: fn(&mut App, &NotesApp),
) -> WidgetRef {
    let notes = notes.clone();
    let button = Button::new(
        icon,
        Listener::new(move |app: &mut App| command(app, &notes)),
    )
    .style(ButtonStyle::Subtle);
    ToolTipService::new(button, ToolTip::text(name)).into_widget()
}
