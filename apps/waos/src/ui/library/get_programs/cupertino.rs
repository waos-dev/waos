//! Getting programs at a phone's size, in Cupertino's widgets: a page of its own behind the home's
//! plus, with Back to the home, as grouped lists in iOS's Settings are: first the field that adds
//! a source, then each source, its header the source, with each program it lists, its icon, name
//! and version, and Get, Update, Installing… or Installed at the end of its row, or what the
//! source says while it is read or could not be; and at each source's foot the row that removes
//! it.
//!
//! What the page keeps of its own is the text of a source being added; what it shows is the
//! library's.

use std::rc::Rc;

use waft::{
    App, BuildContext, EdgeInsetsGeometry, Entity, Handle, IntoWidget, ListView, Listener, State,
    StateData, StatefulWidget, Task, Text, TextEditingController, TextEditingControllerLeaf,
    TextStyle, WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoButtonSize, CupertinoColors, CupertinoListSection, CupertinoListTile,
    CupertinoNavigationBar, CupertinoPageScaffold, CupertinoTextField,
    ObstructingPreferredSizeWidgetRef,
};
use waos_store::Source;

use super::{ListedRow, RowAction, listed_rows};
use crate::state::library::Library;
use crate::ui::library::cupertino::{back_to_home, under_bar};
use crate::ui::library::home::cupertino::resolved;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The side of a listed program's icon, as the App Store's lists show them.
const ICON_SIDE: f64 = 40.0;

/// Getting programs, in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoGetPrograms {
    pub(crate) library: Entity<Library>,
}

impl StatefulWidget for CupertinoGetPrograms {
    type State = CupertinoGetProgramsState;

    fn create_state(&self) -> CupertinoGetProgramsState {
        CupertinoGetProgramsState {
            state: StateData::new(),
            source_text: None,
        }
    }
}

pub(crate) struct CupertinoGetProgramsState {
    state: StateData<CupertinoGetPrograms>,
    /// What is typed as a source to add.
    source_text: Option<Handle<TextEditingController>>,
}

impl State for CupertinoGetProgramsState {
    type Widget = CupertinoGetPrograms;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let source_text = TextEditingController::new(app);
        app.get_mut(self).source_text = Some(source_text);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(source_text) = app.get_mut(self).source_text.take() {
            app.get_mut(source_text).dispose();
            app.destroy(source_text);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let page = Page {
            library: self.widget(app).library.clone(),
            source_text: app.get(self).source_text.expect("an initialized page"),
        };
        let mut sections = vec![page.add_source()];
        let sources = page.library.read(app).sources();
        if sources.is_empty() {
            sections.push(section(None, vec![note(words::NO_SOURCES)]));
        }
        let sources: Vec<_> = sources
            .iter()
            .map(|listing| {
                (
                    listing.source.clone(),
                    listed_rows(listing, page.library.read(app)),
                )
            })
            .collect();
        for (source, rows) in sources {
            sections.push(page.source(&source, rows, app, context));
        }
        let bar = CupertinoNavigationBar::new()
            .leading(back_to_home(&page.library))
            .middle(Text::new(words::GET_PROGRAMS));
        CupertinoPageScaffold::new(under_bar(move |top| {
            ListView::new()
                .padding(EdgeInsetsGeometry::from_ltrb(0.0, top, 0.0, 24.0))
                .children(sections.clone())
                .into_widget()
        }))
        .navigation_bar(ObstructingPreferredSizeWidgetRef::new(bar))
        .background_color(CupertinoColors::SYSTEM_GROUPED_BACKGROUND)
        .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Page {
    library: Entity<Library>,
    source_text: Handle<TextEditingController>,
}

impl Page {
    /// The field a source is typed in, and Add.
    fn add_source(&self) -> WidgetRef {
        let (library, text) = (self.library.clone(), self.source_text);
        let add = Listener::new(move |app| {
            let typed = text.text_value(app).to_owned();
            library.update(app, |library, cx| library.add_source(&typed, cx));
            if library.read(app).problem().is_none() {
                text.clear(app);
            }
        });
        let field = CupertinoTextField::new()
            .controller(self.source_text.as_controller())
            .placeholder(words::SOURCE_PLACEHOLDER);
        let tile = CupertinoListTile::new(field)
            .trailing(CupertinoButton::new(
                Text::new(words::ADD).into_widget(),
                Some(add),
            ))
            .into_widget();
        section(Some(words::ADD_SOURCE), vec![tile])
    }

    /// A source, with what it lists, and the row that removes it.
    fn source(
        &self,
        source: &Source,
        rows: Result<Vec<ListedRow>, String>,
        app: &mut App,
        context: BuildContext,
    ) -> WidgetRef {
        let mut tiles = match rows {
            Ok(rows) if rows.is_empty() => vec![note(words::LISTS_NOTHING)],
            Ok(rows) => rows
                .into_iter()
                .map(|row| self.listed(source, row))
                .collect(),
            Err(said) => vec![note(&said)],
        };
        tiles.push(self.remove_source(source, app, context));
        section(Some(&source.to_string()), tiles)
    }

    /// A program the source lists: its icon, name and version, and what can be done with it.
    fn listed(&self, source: &Source, row: ListedRow) -> WidgetRef {
        let icon = ProgramIcon {
            png: row.icon.clone(),
            side: ICON_SIDE,
            badge: None,
        };
        let tile = CupertinoListTile::new(Text::new(row.program.name.clone()))
            .subtitle(Text::new(row.detail.clone()))
            .leading(icon)
            .leading_size(ICON_SIDE);
        match row.action {
            RowAction::Installing => tile.additional_info(Text::new(words::INSTALLING)),
            RowAction::Installed => tile.additional_info(Text::new(words::INSTALLED)),
            RowAction::Get | RowAction::Update => {
                let label = if row.action == RowAction::Get {
                    words::GET
                } else {
                    words::UPDATE
                };
                let (library, source, listed) =
                    (self.library.clone(), source.clone(), row.program.clone());
                let install = Listener::new(move |app| {
                    library.update(app, |library, cx| {
                        library.install_from(&source, &listed, cx)
                    })
                });
                tile.trailing(
                    CupertinoButton::tinted(Text::new(label).into_widget(), Some(install))
                        .size_style(CupertinoButtonSize::Small),
                )
            }
        }
        .into_widget()
    }

    /// The row that removes `source`, in the destructive colour.
    fn remove_source(&self, source: &Source, app: &mut App, context: BuildContext) -> WidgetRef {
        let (library, source) = (self.library.clone(), source.clone());
        let remove = Rc::new(move |app: &mut App| {
            library.update(app, |library, cx| library.remove_source(&source, cx));
            Task::ready(())
        });
        let style = TextStyle::new().color(resolved(CupertinoColors::SYSTEM_RED, app, context));
        CupertinoListTile::new(Text::new(words::REMOVE_SOURCE).style(style))
            .on_tap(remove)
            .into_widget()
    }
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
    CupertinoListTile::new(Text::new(text)).into_widget()
}
