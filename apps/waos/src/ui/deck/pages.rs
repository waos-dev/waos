//! A tab's page, laid out at the page's size: the home, which is the library, or a program's page.
//! A program's page is its icon and name while it starts, its view's canvas, which takes every
//! touch on it, while it runs, and, once it ended on its own, its last picture dimmed under its
//! icon, its name and how it ended: a plain line, and beneath it, for a failure, the engine's own
//! words, smaller and in gray, whole, for the user to read and report, the page scrolling where
//! they are longer than it.
//!
//! The picture of a program that ended is dimmed by a colour painted over it, never by an
//! opacity: a program's picture is the system's layer beneath WaOS's, showing through a clear
//! hole, and an opacity above the hole clears nothing.
//!
//! The invariant: a page is built once for what its tab shows and built again only when that
//! changes, so moving the deck never builds a page.

use std::sync::Arc;

use waft::{
    App, BuildContext, Center, ColoredBox, Column, EdgeInsetsGeometry, Entity, FontWeight,
    IntoWidget, MainAxisSize, Padding, SingleChildScrollView, SizedBox, Stack, StackFit,
    StatelessWidget, Text, TextAlign, TextStyle, WidgetRef,
};
use waft_cupertino::CupertinoColors;
use waos_package::ProgramId;
use waos_program_window::{ProgramCanvas, ProgramPicture, ShownView};

use super::words::{self, Ending};
use crate::state::deck::tabs::{TabContent, TabStage};
use crate::state::library::Library;
use crate::state::recordings::Recordings;
use crate::ui::library::LaidOutLibrary;
use crate::ui::library::home::cupertino::resolved;
use crate::ui::library::program_icon::ProgramIcon;

/// The side of a program's icon over its page while it cannot show itself.
const ICON_SIDE: f64 = 96.0;
/// How much of the page's background covers an ended program's picture, so what is written over
/// it reads while the picture still shows through.
const ENDED_DIMMING: f64 = 0.75;

/// What a tab's page shows, which a page is built for.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum PageShown {
    Home,
    Starting(ProgramId),
    Running(Entity<ShownView>),
    /// The program's view, which keeps its last picture, where it asked for one, and how it
    /// ended, in words.
    Ended(Option<Entity<ShownView>>, Ending),
}

/// A program as its tab names it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NamedProgram {
    pub(super) name: String,
    pub(super) icon: Option<Arc<[u8]>>,
}

impl NamedProgram {
    /// The program `id` as `library` has it installed: its name as its user knows it and its
    /// icon; by its id, with no icon, once it is no longer installed.
    pub(super) fn of(id: &ProgramId, library: &Library) -> NamedProgram {
        let name = library
            .installed()
            .iter()
            .find(|program| program.manifest.id() == id)
            .map_or_else(|| id.to_string(), |program| program.name());
        NamedProgram {
            name,
            icon: library.icon(id).cloned(),
        }
    }

    /// The program's icon, `side` across.
    pub(super) fn icon(&self, side: f64) -> WidgetRef {
        ProgramIcon {
            png: self.icon.clone(),
            side,
            badge: None,
        }
        .into_widget()
    }
}

impl PageShown {
    /// What the page of a tab holding `content` shows, its program named `program`.
    pub(super) fn of(content: &TabContent, program: Option<&NamedProgram>) -> PageShown {
        let TabContent::Program { id, stage } = content else {
            return PageShown::Home;
        };
        match stage {
            TabStage::Starting | TabStage::Waiting => PageShown::Starting((*id).clone()),
            TabStage::Running(shown) => PageShown::Running(shown.clone()),
            TabStage::Ended { shown, end } => {
                let name = program.map_or("", |program| program.name.as_str());
                PageShown::Ended(shown.clone(), words::ended(name, end))
            }
        }
    }
}

/// A tab's page.
#[derive(Clone, Debug)]
pub(super) struct TabPage {
    pub(super) shown: PageShown,
    /// The tab's program; none for the home.
    pub(super) program: Option<NamedProgram>,
    pub(super) library: Entity<Library>,
    pub(super) recordings: Entity<Recordings>,
}

impl StatelessWidget for TabPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        match &self.shown {
            PageShown::Home => LaidOutLibrary {
                library: self.library.clone(),
                recordings: self.recordings.clone(),
                questions_here: false,
            }
            .into_widget(),
            PageShown::Starting(_) => self.starting(app, context),
            PageShown::Running(shown) => ProgramCanvas::new(shown.clone()).into_widget(),
            PageShown::Ended(shown, how) => self.ended(shown, how, app, context),
        }
    }
}

impl TabPage {
    /// The program's icon and name in the middle of the page's background, over a word that it
    /// starts.
    fn starting(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let secondary = resolved(CupertinoColors::SECONDARY_LABEL, app, context);
        let mut lines = self.identity(app, context);
        lines.push(SizedBox::new().height(8.0).into_widget());
        lines.push(
            Text::new(words::STARTING)
                .style(TextStyle::new().color(secondary).font_size(15.0))
                .text_align(TextAlign::Center)
                .into_widget(),
        );
        let background = resolved(CupertinoColors::SYSTEM_BACKGROUND, app, context);
        ColoredBox::new(background)
            .child(Center::new().child(padded(lines)))
            .into_widget()
    }

    /// The program's last picture, dimmed, where it had one, under its icon, its name and how it
    /// ended.
    fn ended(
        &self,
        shown: &Option<Entity<ShownView>>,
        ending: &Ending,
        app: &mut App,
        context: BuildContext,
    ) -> WidgetRef {
        let background = resolved(CupertinoColors::SYSTEM_BACKGROUND, app, context);
        let picture = match shown {
            Some(shown) => {
                let dimming = background.with_values(Some(ENDED_DIMMING), None, None, None, None);
                Stack::new()
                    .fit(StackFit::Expand)
                    .children([
                        ProgramPicture::new(shown.clone()).into_widget(),
                        ColoredBox::new(dimming).into_widget(),
                    ])
                    .into_widget()
            }
            None => ColoredBox::new(background).into_widget(),
        };
        Stack::new()
            .fit(StackFit::Expand)
            .children([picture, self.ending(ending, app, context)])
            .into_widget()
    }

    /// The program's icon and name in the middle of the page, over how it ended: its plain line,
    /// and the engine's words beneath, smaller and in gray, where there are any. It scrolls where
    /// it is longer than the page.
    fn ending(&self, ending: &Ending, app: &mut App, context: BuildContext) -> WidgetRef {
        let label = resolved(CupertinoColors::LABEL, app, context);
        let secondary = resolved(CupertinoColors::SECONDARY_LABEL, app, context);
        let mut lines = self.identity(app, context);
        lines.push(SizedBox::new().height(8.0).into_widget());
        lines.push(
            Text::new(ending.said.clone())
                .style(TextStyle::new().color(label).font_size(15.0))
                .text_align(TextAlign::Center)
                .into_widget(),
        );
        if let Some(detail) = &ending.detail {
            lines.push(SizedBox::new().height(12.0).into_widget());
            lines.push(
                Text::new(detail.clone())
                    .style(TextStyle::new().color(secondary).font_size(12.0))
                    .text_align(TextAlign::Center)
                    .into_widget(),
            );
        }
        Center::new()
            .child(SingleChildScrollView::new().child(padded(lines)))
            .into_widget()
    }

    /// The program's icon over its name; nothing for the home.
    fn identity(&self, app: &mut App, context: BuildContext) -> Vec<WidgetRef> {
        let Some(program) = &self.program else {
            return Vec::new();
        };
        let label = resolved(CupertinoColors::LABEL, app, context);
        vec![
            program.icon(ICON_SIDE),
            SizedBox::new().height(16.0).into_widget(),
            Text::new(program.name.clone())
                .style(
                    TextStyle::new()
                        .color(label)
                        .font_size(20.0)
                        .font_weight(FontWeight::W600),
                )
                .text_align(TextAlign::Center)
                .into_widget(),
        ]
    }
}

/// `lines` in a column as tall as they are, clear of the page's edges.
fn padded(lines: Vec<WidgetRef>) -> Padding {
    Padding::new(EdgeInsetsGeometry::all(32.0)).child(
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .children(lines),
    )
}
