//! The app at a phone's width, in Cupertino's widgets, as a phone's settings are: the programs in
//! a list, each its icon, name, and what is said of it and what it used; a tap opens the program's
//! report, as iOS's App Privacy Report shows an app's, and from it each live panel, a page each,
//! with a way back.
//!
//! The invariant: the page shown follows the entities alone: the list with no program chosen,
//! else the chosen program's tab; a tap or the way back changes the program chosen, the list's
//! selection, as a click in the desktop's list does.

mod panel;
mod report;
mod storage;

use std::rc::Rc;

use waft::{
    AnyColor, App, BuildContext, Builder, Color, EdgeInsetsGeometry, Entity, Icon, IntoWidget,
    ListView, Listener, MainAxisSize, MediaQuery, Row, StatelessWidget, Task, Text, TextStyle,
    WidgetRef,
};
use waft_cupertino::{
    CupertinoActivityIndicator, CupertinoApp, CupertinoButton, CupertinoColors,
    CupertinoDynamicColor, CupertinoIcons, CupertinoListSection, CupertinoListTile,
    CupertinoListTileChevron, CupertinoNavigationBar, CupertinoPageScaffold,
    ObstructingPreferredSizeWidgetRef,
};

use crate::program_icon::ProgramIcon;
use crate::state::page::{ProgramPage, Tab};
use crate::state::programs::{Found, Programs, Seen, choose, want_icon};
use crate::words;

const ICON_SIDE: f64 = 40.0;

/// The app in Cupertino's widgets, in its own app.
#[derive(Debug)]
pub(crate) struct CupertinoInspector {
    pub(crate) page: Entity<ProgramPage>,
}

impl StatelessWidget for CupertinoInspector {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        CupertinoApp::new()
            .title(words::APP_NAME)
            .debug_show_checked_mode_banner(false)
            .home(Pages {
                page: self.page.clone(),
            })
            .into_widget()
    }
}

/// The list, or the page of the program chosen: its report, or one of its panels.
#[derive(Debug)]
struct Pages {
    page: Entity<ProgramPage>,
}

impl StatelessWidget for Pages {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let programs = self.page.read(app).programs.clone();
        let read = programs.read(app);
        let (found, chosen) = (read.found, read.chosen(app).cloned());
        let tab = self.page.read(app).tab;
        match (found, chosen) {
            (Found::Ready, Some(seen)) if tab == Tab::Report => {
                report::report_page(&self.page, &seen, app)
            }
            (Found::Ready, Some(_)) if tab == Tab::Storage => storage::storage_page(&self.page),
            (Found::Ready, Some(seen)) => panel::panel_page(&self.page, &seen, tab, app),
            (found, _) => list_page(&programs, found, app),
        }
    }
}

/// The programs, a tap on one opening its page.
fn list_page(programs_entity: &Entity<Programs>, found: Found, app: &mut App) -> WidgetRef {
    let programs = programs_entity.read(app).list.clone();
    let sections = match found {
        Found::Looking => vec![section(
            None,
            None,
            vec![
                CupertinoListTile::new(Text::new(words::LOOKING))
                    .leading(CupertinoActivityIndicator::new())
                    .into_widget(),
            ],
        )],
        Found::NoDevice => vec![section(None, None, vec![note(words::NO_DEVICE)])],
        Found::Ready if programs.is_empty() => {
            vec![section(None, None, vec![note(words::NO_PROGRAMS)])]
        }
        Found::Ready => {
            let tiles = programs
                .iter()
                .enumerate()
                .map(|(index, seen)| program_tile(app, programs_entity, (index, seen)))
                .collect();
            vec![section(Some(words::PROGRAMS), None, tiles)]
        }
    };
    let bar = CupertinoNavigationBar::new().middle(Text::new(words::APP_NAME));
    scaffold(bar, sections)
}

/// A program's tile: its icon, name, what is said of it and what it used. Its icon is asked for as
/// the tile is first shown.
fn program_tile(
    app: &mut App,
    programs: &Entity<Programs>,
    (index, seen): (usize, &Seen),
) -> WidgetRef {
    want_icon(app, programs, &seen.program.id);
    let icon = ProgramIcon {
        png: programs.read(app).icon(&seen.program.id),
        side: ICON_SIDE,
    };
    let marks = words::marks(seen);
    let said = if marks.is_empty() {
        words::detail(seen)
    } else {
        format!("{}\n{marks}", words::detail(seen))
    };
    let programs = programs.clone();
    let open = Rc::new(move |app: &mut App| {
        choose(app, &programs, Some(index));
        Task::ready(())
    });
    CupertinoListTile::new(Text::new(seen.program.name.clone()))
        .subtitle(Text::new(said).max_lines(2))
        .padding(TALL_TILE_PADDING)
        .leading(icon)
        .leading_size(ICON_SIDE)
        .trailing(CupertinoListTileChevron::new())
        .on_tap(open)
        .into_widget()
}

/// The padding of a tile whose subtitle takes more than a line: a subtitled tile's, with iOS's
/// space above and below, which Flutter's tile, centred in its least height, has none of.
pub(super) const TALL_TILE_PADDING: EdgeInsetsGeometry =
    EdgeInsetsGeometry::directional(20.0, 10.0, 14.0, 10.0);

/// The most lines a line beneath a row takes: a tile's subtitle takes one unless told otherwise,
/// as Flutter's does.
pub(super) const BENEATH_LINES: i32 = 3;

/// The navigation bar's way back, named after where it goes, as iOS names a back button: `to`,
/// which `going` makes the page.
pub(super) fn back(
    to: &'static str,
    page: &Entity<ProgramPage>,
    going: fn(&mut App, &Entity<ProgramPage>),
) -> WidgetRef {
    let page = page.clone();
    let back = Listener::new(move |app| going(app, &page));
    back_by(to, back)
}

/// The way back to `to`, by `pressed`.
pub(super) fn back_by(to: impl Into<String>, pressed: Listener) -> WidgetRef {
    let label = Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(2.0)
        .children([
            Icon::new(Some(CupertinoIcons::back())).into_widget(),
            Text::new(to.into()).into_widget(),
        ]);
    CupertinoButton::new(label.into_widget(), Some(pressed))
        .padding(EdgeInsetsGeometry::all(0.0))
        .into_widget()
}

/// `sections` under `bar`, clear of it, on the grouped background.
pub(super) fn scaffold(bar: CupertinoNavigationBar, sections: Vec<WidgetRef>) -> WidgetRef {
    let body = Builder::new(move |app, context| {
        let top = MediaQuery::padding_of(app, context).top;
        ListView::new()
            .padding(EdgeInsetsGeometry::from_ltrb(0.0, top, 0.0, 24.0))
            .children(sections.clone())
            .into_widget()
    });
    CupertinoPageScaffold::new(body)
        .navigation_bar(ObstructingPreferredSizeWidgetRef::new(bar))
        .background_color(CupertinoColors::SYSTEM_GROUPED_BACKGROUND)
        .into_widget()
}

/// The size of a section's footer, iOS's footnote: Flutter leaves an inset grouped section's
/// footer in the body's style, as UIKit does not.
const FOOTNOTE: f64 = 13.0;

/// An inset-grouped section, under `title` and over `footer` where given.
pub(super) fn section(
    title: Option<&str>,
    footer: Option<&str>,
    tiles: Vec<WidgetRef>,
) -> WidgetRef {
    let mut section = CupertinoListSection::inset_grouped().children(tiles);
    if let Some(title) = title {
        section = section.header(Text::new(title.to_owned()));
    }
    if let Some(footer) = footer {
        let footer = footer.to_owned();
        section = section.footer(Builder::new(move |app, context| {
            let color = resolved(CupertinoColors::SECONDARY_LABEL, app, context);
            Text::new(footer.clone())
                .style(TextStyle::new().font_size(FOOTNOTE).color(color))
                .into_widget()
        }));
    }
    section.into_widget()
}

/// A tile that only says `text`, as many lines as it takes.
pub(super) fn note(text: &str) -> WidgetRef {
    let text = Text::new(text.to_owned()).max_lines(i32::MAX);
    CupertinoListTile::new(text)
        .padding(TALL_TILE_PADDING)
        .into_widget()
}

/// `color` as the theme of `context` resolves it.
pub(super) fn resolved(color: AnyColor, app: &mut App, context: BuildContext) -> Color {
    CupertinoDynamicColor::resolve(&color, app, context).color()
}
