//! The programs, in a selectable list: WinUI's `ItemsView` of `ItemContainer`s, each its icon,
//! name, what is said of it and what it used.
//!
//! The invariant: the user's selection chooses the program shown beside the list, and the list
//! selects the program chosen, or the first where none is listed, each time it is given its
//! items, as a WinUI app selects right after setting `ItemsSource`; it is given them again only
//! when the programs or their order change, since giving them clears the selection.

use std::rc::Rc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded, Handle,
    IntoWidget, MainAxisSize, Padding, Ref, Row, State, StateData, StatefulWidget, Text, WidgetRef,
};
use waft_winui::{
    ItemContainer, ItemsView, ItemsViewSelectionMode, ItemsViewSource, TextBlockStyle,
    ThemeResources,
};

use super::{GAP, MARGIN, one_line, said};
use crate::inspector::{Inspector, Seen, want_icon};
use crate::program_icon::ProgramIcon;
use crate::words;

const ROW_ICON: f64 = 32.0;

/// The programs, a selectable list.
#[derive(Debug)]
pub(super) struct ProgramList {
    pub(super) inspector: Entity<Inspector>,
}

impl StatefulWidget for ProgramList {
    type State = ProgramListState;

    fn create_state(&self) -> ProgramListState {
        ProgramListState {
            state: StateData::new(),
            source: None,
            listed: Vec::new(),
            listing: false,
        }
    }
}

pub(super) struct ProgramListState {
    state: StateData<ProgramList>,
    /// The list's items and selection.
    source: Option<Ref<ItemsViewSource>>,
    /// The programs the source was given, by id, in its order.
    listed: Vec<String>,
    /// Whether the list is being given its programs and selecting, while the tree builds.
    listing: bool,
}

impl ProgramListState {
    fn source(self: Handle<Self>, app: &App) -> Handle<ItemsViewSource> {
        app.get(self)
            .source
            .as_ref()
            .map(Ref::handle)
            .expect("made in `init_state`")
    }

    /// Gives the list the programs where they or their order changed, then selects the program
    /// chosen, or the first where none is; the program it selects is chosen once the tree is
    /// built, since the inspector may not change while it builds.
    fn list_programs(self: Handle<Self>, app: &mut App) {
        let inspector = self.widget(app).inspector.clone();
        let programs: Vec<String> = (inspector.read(app).programs.iter())
            .map(|seen| seen.program.id.clone())
            .collect();
        if programs == app.get(self).listed {
            return;
        }
        app.get_mut(self).listing = true;
        self.source(app).set_items(app, programs.len());
        app.get_mut(self).listed = programs;
        self.select_the_choice(app);
        app.get_mut(self).listing = false;
        let _choosing = app.spawn(async move |cx| {
            cx.update(|app| {
                if app.contains(self) && self.mounted(app) {
                    self.choose_selected(app);
                }
            });
        });
    }

    /// Selects the program chosen at its place now, or the first where none is chosen or the one
    /// chosen is no longer listed.
    fn select_the_choice(self: Handle<Self>, app: &mut App) {
        let inspector = self.widget(app).inspector.clone();
        let read = inspector.read(app);
        let chosen = (read.chosen.as_deref())
            .and_then(|chosen| (read.programs.iter()).position(|seen| seen.program.id == chosen));
        let index = chosen.or((!read.programs.is_empty()).then_some(0));
        let source = self.source(app);
        match index {
            Some(index) => source.select(app, index),
            None => source.deselect_all(app),
        }
    }

    /// The selection changed: the program selected is chosen, at once where the user selected it;
    /// after the build where the list did, as it was given its programs.
    fn selected(self: Handle<Self>, app: &mut App) {
        if !app.get(self).listing {
            self.choose_selected(app);
        }
    }

    fn choose_selected(self: Handle<Self>, app: &mut App) {
        let Some(index) = self.source(app).selected_index(app) else {
            return;
        };
        let inspector = self.widget(app).inspector.clone();
        let id = inspector
            .read(app)
            .programs
            .get(index)
            .map(|seen| seen.program.id.clone());
        let unchanged = id.is_none() || inspector.read(app).chosen == id;
        if !unchanged {
            inspector.update(app, |inspector, cx| inspector.choose(id, cx));
        }
    }
}

impl State for ProgramListState {
    type Widget = ProgramList;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let source = ItemsViewSource::new(app, 0);
        app.get_mut(self).source = Some(source);
        self.list_programs(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, _old: &ProgramList) {
        self.list_programs(app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        self.source(app).dispose(app);
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let inspector = self.widget(app).inspector.clone();
        let programs = inspector.read(app).programs.clone();
        if programs.is_empty() {
            return said(words::NO_PROGRAMS, &resources);
        }
        let source = app.get(self).source.clone().expect("made in `init_state`");
        let rows = Rc::new(programs);
        let list = ItemsView::new(source, move |app, context, index| {
            let resources = ThemeResources::of(app, context);
            ItemContainer::new(program_row(app, &inspector, &rows[index], &resources))
        })
        .selection_mode(ItemsViewSelectionMode::Single)
        .selection_changed(move |app| self.selected(app))
        .padding([GAP, 0.0, GAP, GAP]);
        let title = Text::new(words::PROGRAMS).style(
            TextBlockStyle::Subtitle.text_style(&resources, resources.text_fill_color_primary),
        );
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Padding::new(EdgeInsetsGeometry::from_ltrb(GAP, MARGIN, GAP, 0.0))
                    .child(title)
                    .into_widget(),
                Expanded::new(list).into_widget(),
            ])
            .into_widget()
    }
}

/// A program's row: its icon, its name, what is said of it, and what it used. Its icon is asked
/// for as the row is first shown.
fn program_row(
    app: &mut App,
    inspector: &Entity<Inspector>,
    seen: &Seen,
    resources: &ThemeResources,
) -> WidgetRef {
    want_icon(app, inspector, &seen.program.id);
    let icon = ProgramIcon {
        image: inspector.read(app).icon(&seen.program.id),
        side: ROW_ICON,
    };
    let mut lines = vec![
        one_line(&seen.program.name, TextBlockStyle::Body, resources, true),
        one_line(
            &words::detail(seen),
            TextBlockStyle::Caption,
            resources,
            false,
        ),
    ];
    let marks = words::marks(seen);
    if !marks.is_empty() {
        lines.push(one_line(&marks, TextBlockStyle::Caption, resources, false));
    }
    let text = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(lines);
    Padding::new(EdgeInsetsGeometry::symmetric(GAP, 8.0))
        .child(
            Row::new()
                .spacing(GAP)
                .children([icon.into_widget(), Expanded::new(text).into_widget()]),
        )
        .into_widget()
}
