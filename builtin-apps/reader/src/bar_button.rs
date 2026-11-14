//! A button above the content: a subtle Fluent button showing an icon, named by its tooltip, which
//! does a command.

use waft::{App, IntoWidget, Listener, WidgetRef};
use waft_winui::{Button, ButtonStyle, FontIcon, ToolTip, ToolTipService};

use crate::reader_app::ReaderApp;

/// A button showing `icon`, named `name`, which does `command` on `reader`.
pub(crate) fn icon_button(
    icon: FontIcon,
    name: &str,
    reader: &ReaderApp,
    command: fn(&mut App, &ReaderApp),
) -> WidgetRef {
    let reader = reader.clone();
    let button = Button::new(
        icon,
        Listener::new(move |app: &mut App| command(app, &reader)),
    )
    .style(ButtonStyle::Subtle);
    ToolTipService::new(button, ToolTip::text(name)).into_widget()
}
