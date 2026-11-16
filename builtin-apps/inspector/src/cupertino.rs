//! The app at a phone's width, in Cupertino's widgets, as a phone's settings are: the programs in
//! a list, each its icon, name, and what is said of it and what it used; a tap opens a page of what
//! the program did over the last seven days, a section per capability, with a way back to the
//! list.

use std::rc::Rc;

use waft::{
    App, BuildContext, Builder, EdgeInsetsGeometry, Entity, Icon, IntoWidget, ListView, Listener,
    MainAxisSize, MediaQuery, Row, StatelessWidget, Task, Text, WidgetRef,
};
use waft_cupertino::{
    CupertinoActivityIndicator, CupertinoApp, CupertinoButton, CupertinoColors, CupertinoIcons,
    CupertinoListSection, CupertinoListTile, CupertinoListTileChevron, CupertinoNavigationBar,
    CupertinoPageScaffold, ObstructingPreferredSizeWidgetRef,
};

use crate::inspector::{Found, Inspector, Seen, want_icon};
use crate::program_icon::ProgramIcon;
use crate::words;

const ICON_SIDE: f64 = 40.0;

/// The app in Cupertino's widgets, in its own app.
#[derive(Debug)]
pub(crate) struct CupertinoInspector {
    pub(crate) inspector: Entity<Inspector>,
}

impl StatelessWidget for CupertinoInspector {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        CupertinoApp::new()
            .title(words::APP_NAME)
            .home(Pages {
                inspector: self.inspector.clone(),
            })
            .into_widget()
    }
}

/// The list, or the page of the program chosen.
#[derive(Debug)]
struct Pages {
    inspector: Entity<Inspector>,
}

impl StatelessWidget for Pages {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let read = self.inspector.read(app);
        let (found, chosen) = (read.found, read.chosen().cloned());
        match (found, chosen) {
            (Found::Ready, Some(seen)) => program_page(&self.inspector, &seen, app),
            (found, _) => list_page(&self.inspector, found, app),
        }
    }
}

/// The programs, a tap on one opening its page.
fn list_page(inspector: &Entity<Inspector>, found: Found, app: &mut App) -> WidgetRef {
    let programs = inspector.read(app).programs.clone();
    let sections = match found {
        Found::Looking => vec![section(
            None,
            vec![
                CupertinoListTile::new(Text::new(words::LOOKING))
                    .leading(CupertinoActivityIndicator::new())
                    .into_widget(),
            ],
        )],
        Found::NoDevice => vec![section(None, vec![note(words::NO_DEVICE)])],
        Found::Ready if programs.is_empty() => vec![section(None, vec![note(words::NO_PROGRAMS)])],
        Found::Ready => {
            let tiles = programs
                .iter()
                .map(|seen| program_tile(app, inspector, seen))
                .collect();
            vec![section(Some(words::PROGRAMS), tiles)]
        }
    };
    let bar = CupertinoNavigationBar::new().middle(Text::new(words::APP_NAME));
    scaffold(bar, sections)
}

/// A program's tile: its icon, name, what is said of it and what it used. Its icon is asked for as
/// the tile is first shown.
fn program_tile(app: &mut App, inspector: &Entity<Inspector>, seen: &Seen) -> WidgetRef {
    want_icon(app, inspector, &seen.program.id);
    let icon = ProgramIcon {
        png: inspector.read(app).icon(&seen.program.id),
        side: ICON_SIDE,
    };
    let marks = words::marks(seen);
    let said = if marks.is_empty() {
        words::detail(seen)
    } else {
        format!("{}\n{marks}", words::detail(seen))
    };
    let (inspector, id) = (inspector.clone(), seen.program.id.clone());
    let open = Rc::new(move |app: &mut App| {
        let id = Some(id.clone());
        inspector.update(app, |inspector, cx| inspector.choose(id, cx));
        Task::ready(())
    });
    CupertinoListTile::new(Text::new(seen.program.name.clone()))
        .subtitle(Text::new(said))
        .leading(icon)
        .leading_size(ICON_SIDE)
        .trailing(CupertinoListTileChevron::new())
        .on_tap(open)
        .into_widget()
}

/// What the program did over the last seven days, a section per capability.
fn program_page(inspector: &Entity<Inspector>, seen: &Seen, app: &mut App) -> WidgetRef {
    let icon = ProgramIcon {
        png: inspector.read(app).icon(&seen.program.id),
        side: ICON_SIDE,
    };
    let header = CupertinoListTile::new(Text::new(seen.program.name.clone()))
        .subtitle(Text::new(words::detail(seen)))
        .leading(icon)
        .leading_size(ICON_SIDE)
        .into_widget();
    let mut sections = vec![section(None, vec![header])];
    let said = seen.activity.sections();
    if said.is_empty() {
        sections.push(section(
            Some(words::LAST_DAYS),
            vec![note(words::USED_NOTHING)],
        ));
    }
    sections.extend(said.into_iter().map(|said| {
        let tiles = said.lines.iter().map(|line| note(line)).collect();
        section(Some(said.title), tiles)
    }));
    let bar = CupertinoNavigationBar::new()
        .leading(back_to_list(inspector))
        .middle(Text::new(seen.program.name.clone()));
    scaffold(bar, sections)
}

/// The navigation bar's way back to the list, named after it, as iOS names a back button.
fn back_to_list(inspector: &Entity<Inspector>) -> WidgetRef {
    let inspector = inspector.clone();
    let back = Listener::new(move |app| {
        inspector.update(app, |inspector, cx| inspector.choose(None, cx));
    });
    let label = Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(2.0)
        .children([
            Icon::new(Some(CupertinoIcons::back())).into_widget(),
            Text::new(words::PROGRAMS).into_widget(),
        ]);
    CupertinoButton::new(label.into_widget(), Some(back))
        .padding(EdgeInsetsGeometry::all(0.0))
        .into_widget()
}

/// `sections` under `bar`, clear of it, on the grouped background.
fn scaffold(bar: CupertinoNavigationBar, sections: Vec<WidgetRef>) -> WidgetRef {
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

/// An inset-grouped section, under `title` where given.
fn section(title: Option<&str>, tiles: Vec<WidgetRef>) -> WidgetRef {
    let section = CupertinoListSection::inset_grouped().children(tiles);
    match title {
        Some(title) => section.header(Text::new(title)),
        None => section,
    }
    .into_widget()
}

/// A tile that only says `text`.
fn note(text: &str) -> WidgetRef {
    CupertinoListTile::new(Text::new(text.to_owned())).into_widget()
}
