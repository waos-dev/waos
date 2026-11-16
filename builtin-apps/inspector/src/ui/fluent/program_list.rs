//! The programs, in the side pane: WinUI's `ItemsView` of `ItemContainer`s over the programs'
//! own `ItemsViewSource`, each its icon, name, what is said of it and whether it runs, under a
//! count of them, as Windows Settings lists its pages.
//!
//! The invariant: the list's selection is the program chosen, held by the programs' entity; the
//! list only shows it and lets the user change it.

use std::rc::Rc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded,
    IntoWidget, MainAxisSize, Padding, Row, StatelessWidget, WidgetRef,
};
use waft_winui::{
    ItemContainer, ItemsView, ItemsViewSelectionMode, TextBlockStyle, ThemeResources,
};

use super::{GAP, MARGIN, caption, line, said, status_dot};
use crate::program_icon::ProgramIcon;
use crate::state::programs::{Programs, Seen, want_icon};
use crate::words;

const ROW_ICON: f64 = 28.0;

/// The programs, a selectable list.
#[derive(Debug)]
pub(super) struct ProgramList {
    pub(super) programs: Entity<Programs>,
}

impl StatelessWidget for ProgramList {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let read = self.programs.read(app);
        if read.list.is_empty() {
            return said(words::NO_PROGRAMS, &resources);
        }
        let rows = Rc::new(read.list.clone());
        let count = rows.len().to_string();
        let programs = self.programs.clone();
        let list = ItemsView::new(read.selection(), move |app, context, index| {
            let resources = ThemeResources::of(app, context);
            ItemContainer::new(program_row(app, &programs, &rows[index], &resources))
        })
        .selection_mode(ItemsViewSelectionMode::Single)
        .padding([4.0, 0.0, 4.0, GAP]);
        let title = Row::new().children([
            Expanded::new(line(
                words::PROGRAMS,
                TextBlockStyle::BodyStrong,
                resources.text_fill_color_primary,
                &resources,
            ))
            .into_widget(),
            caption(&count, &resources),
        ]);
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Padding::new(EdgeInsetsGeometry::from_ltrb(
                    GAP + 4.0,
                    MARGIN,
                    GAP + 4.0,
                    4.0,
                ))
                .child(title)
                .into_widget(),
                Expanded::new(list).into_widget(),
            ])
            .into_widget()
    }
}

/// A program's row: its icon, its name, what is said of it, and whether it runs. Its icon is
/// asked for as the row is first shown.
fn program_row(
    app: &mut App,
    programs: &Entity<Programs>,
    seen: &Seen,
    resources: &ThemeResources,
) -> WidgetRef {
    want_icon(app, programs, &seen.program.id);
    let icon = ProgramIcon {
        png: programs.read(app).icon(&seen.program.id),
        side: ROW_ICON,
    };
    let text = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children([
            line(
                &seen.program.name,
                TextBlockStyle::Body,
                resources.text_fill_color_primary,
                resources,
            ),
            caption(&words::detail(seen), resources),
        ]);
    Padding::new(EdgeInsetsGeometry::symmetric(8.0, GAP))
        .child(Row::new().spacing(GAP).children([
            icon.into_widget(),
            Expanded::new(text).into_widget(),
            status_dot(seen.program.running, resources),
        ]))
        .into_widget()
}
