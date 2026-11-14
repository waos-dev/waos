//! A button in one of the window's bars: a subtle Fluent button showing an icon, named by its
//! tooltip, which does a command.

use waft::{App, IntoWidget, Listener, WidgetRef};
use waft_winui::{Button, ButtonStyle, ToolTip, ToolTipService};

use crate::documents_app::DocumentsApp;
use crate::icons::icon;

/// What a command does, given the app.
pub(crate) type Command = fn(&mut App, &DocumentsApp);

/// A button showing the icon `glyph`, named `name`, which does `command`.
pub(crate) fn icon_button(
    glyph: char,
    name: &str,
    documents: &DocumentsApp,
    command: Command,
) -> WidgetRef {
    let documents = documents.clone();
    let click = Listener::new(move |app: &mut App| command(app, &documents));
    let button = Button::new(icon(glyph), click).style(ButtonStyle::Subtle);
    ToolTipService::new(button, ToolTip::text(name)).into_widget()
}
