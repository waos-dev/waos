//! The program's commands, on the right of its header: Start inspected while it does not run,
//! Restart inspected and Stop while it does; none for the inspector itself. They are a list of
//! commands drawn as a row of buttons, the shape WinUI's `CommandBar` takes its primary commands
//! in, so the row becomes one once Waft has it.
//!
//! The invariant: the commands are said once, as `Command`s, and drawn by `command_row` alone.

use waft::{App, Entity, IntoWidget, Listener, MainAxisSize, Row, Text, WidgetRef};
use waft_winui::{Button, ButtonStyle, FluentSymbol, FontIcon};

use crate::state::page::{ProgramPage, end_program, start_inspected};
use crate::state::programs::{OWN_ID, Seen};
use crate::words;

/// A command: WinUI's `AppBarButton`, its label, its icon and what it does.
struct Command {
    label: &'static str,
    symbol: FluentSymbol,
    /// Whether it is the page's main action, drawn in the accent.
    accent: bool,
    pressed: fn(&mut App, &Entity<ProgramPage>),
}

/// The commands of the program `seen`, as its header shows them.
pub(super) fn commands(page: &Entity<ProgramPage>, seen: &Seen) -> WidgetRef {
    let commands = if seen.program.id == OWN_ID {
        Vec::new()
    } else if seen.program.running {
        vec![
            Command {
                label: words::RESTART_INSPECTED,
                symbol: FluentSymbol::ArrowRepeatAll,
                accent: false,
                pressed: start_inspected,
            },
            Command {
                label: words::STOP,
                symbol: FluentSymbol::Stop,
                accent: false,
                pressed: end_program,
            },
        ]
    } else {
        vec![Command {
            label: words::START_INSPECTED,
            symbol: FluentSymbol::Play,
            accent: true,
            pressed: start_inspected,
        }]
    };
    command_row(page, commands)
}

/// `commands` as a row of buttons, each its icon and label.
fn command_row(page: &Entity<ProgramPage>, commands: Vec<Command>) -> WidgetRef {
    let buttons = commands.into_iter().map(|command| {
        let page = page.clone();
        let pressed = command.pressed;
        let content = Row::new()
            .main_axis_size(MainAxisSize::Min)
            .spacing(8.0)
            .children([
                FontIcon::symbol(command.symbol)
                    .font_size(14.0)
                    .into_widget(),
                Text::new(command.label).into_widget(),
            ]);
        let style = if command.accent {
            ButtonStyle::Accent
        } else {
            ButtonStyle::Subtle
        };
        Button::new(content, Listener::new(move |app| pressed(app, &page)))
            .style(style)
            .into_widget()
    });
    Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(4.0)
        .children(buttons.collect::<Vec<_>>())
        .into_widget()
}
