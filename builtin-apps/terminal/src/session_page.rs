//! What a session's window shows, on a page clear of a phone's bars and keyboard: the page that asks for a machine while the program holds none,
//! and otherwise the terminal, over a line once the shell has ended or could not start.

use std::fmt;

use waft_embedder::{Color, TextAlign};
use waft_foundation::{App, Entity, Listener};
use waft_widgets::{
    Align, Alignment, AlignmentGeometry, BoxConstraints, BuildContext, Center, ColoredBox, Column,
    ConstrainedBox, CrossAxisAlignment, EdgeInsetsGeometry, Expanded, IntoWidget, MainAxisSize,
    MediaQuery, Padding, SizedBox, StatelessWidget, Text, WidgetRef,
};
use waft_winui::{Brush, Button, ButtonStyle, Page, TextBlockStyle, ThemeResources};

use crate::grid_painter::{TextSettings, color};
use crate::palette::Palette;
use crate::session::{Session, Stage, choose_machine};
use crate::terminal_view::TerminalView;

/// The widest the page that asks for a machine sets its text, in logical pixels.
const MESSAGE_WIDTH: f64 = 360.0;

/// The page of `session`, its terminal drawn in the font `text` holds.
pub(crate) struct SessionPage {
    pub(crate) session: Entity<Session>,
    pub(crate) text: Entity<TextSettings>,
}

impl fmt::Debug for SessionPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionPage").finish_non_exhaustive()
    }
}

impl StatelessWidget for SessionPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let stage = self.session.read(app).stage.clone();
        let background = background(app, context, &stage);
        let content = self.content(app, context, stage);
        Page::new()
            .background(Brush::Solid(background))
            .content(content)
            .into_widget()
    }
}

impl SessionPage {
    /// What the page shows at `stage`.
    fn content(&self, app: &mut App, context: BuildContext, stage: Stage) -> WidgetRef {
        let terminal = TerminalView {
            session: self.session.clone(),
            text: self.text.clone(),
        };
        match stage {
            Stage::NoMachines => message(
                app,
                context,
                "This WaOS gives programs no computer to run commands on.",
                None,
            ),
            Stage::NeedsMachine => {
                let session = self.session.clone();
                let choose = Listener::new(move |app| choose_machine(app, &session));
                message(
                    app,
                    context,
                    "Terminal runs a shell on a computer you choose. What it runs there can reach everything you can.",
                    Some(Button::text("Choose a Computer…", choose).style(ButtonStyle::Accent)),
                )
            }
            Stage::Starting | Stage::Running => over_line(app, context, terminal, None),
            Stage::Left => over_line(app, context, terminal, Some("The shell ended.")),
            Stage::Ended(how) => over_line(app, context, terminal, Some(&how)),
            Stage::Failed(why) => {
                let what = format!("The shell did not start: {why}");
                over_line(app, context, terminal, Some(&what))
            }
        }
    }
}

/// What the page is painted with, under a phone's bars too: the terminal's own background once
/// there is a terminal, so that it reaches the screen's edges, and Fluent's otherwise.
fn background(app: &mut App, context: BuildContext, stage: &Stage) -> Color {
    match stage {
        Stage::NoMachines | Stage::NeedsMachine => {
            ThemeResources::of(app, context).solid_background_fill_color_base
        }
        Stage::Starting | Stage::Running | Stage::Left | Stage::Ended(_) | Stage::Failed(_) => {
            let look = MediaQuery::platform_brightness_of(app, context);
            color(Palette::of_look(look).background)
        }
    }
}

/// `text` in the middle of the page, with `action` under it where there is one.
fn message(app: &mut App, context: BuildContext, text: &str, action: Option<Button>) -> WidgetRef {
    let r = ThemeResources::of(app, context);
    let style = TextBlockStyle::Body.text_style(&r, r.text_fill_color_secondary);
    let mut lines = vec![
        Text::new(text.to_owned())
            .style(style)
            .text_align(TextAlign::Center)
            .into_widget(),
    ];
    if let Some(action) = action {
        lines.push(SizedBox::new().height(20.0).into_widget());
        lines.push(action.into_widget());
    }
    let column = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(lines);
    let bounded = ConstrainedBox::new(BoxConstraints::new().max_width(MESSAGE_WIDTH)).child(column);
    Center::new()
        .child(Padding::new(EdgeInsetsGeometry::all(24.0)).child(bounded))
        .into_widget()
}

/// `terminal`, over a line that says `ended`, what became of its shell, and how to go on, once
/// it has ended. The terminal keeps its place in the page either way, and so its focus.
fn over_line(
    app: &mut App,
    context: BuildContext,
    terminal: TerminalView,
    ended: Option<&str>,
) -> WidgetRef {
    let mut rows = vec![Expanded::new(terminal).into_widget()];
    if let Some(what) = ended {
        rows.push(ended_line(app, context, what));
    }
    Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(rows)
        .into_widget()
}

/// A line that says `what` happened to the shell and how to start it again.
fn ended_line(app: &mut App, context: BuildContext, what: &str) -> WidgetRef {
    let r = ThemeResources::of(app, context);
    let style = TextBlockStyle::Caption.text_style(&r, r.text_fill_color_secondary);
    let line = Text::new(format!("{what} Press Enter to start it again."))
        .style(style)
        .max_lines(2);
    ColoredBox::new(r.solid_background_fill_color_base)
        .child(
            Padding::new(EdgeInsetsGeometry::symmetric(8.0, 12.0)).child(
                Align::new()
                    .alignment(AlignmentGeometry::Alignment(Alignment::CENTER_LEFT))
                    .child(line),
            ),
        )
        .into_widget()
}
