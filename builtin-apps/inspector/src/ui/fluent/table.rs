//! A table of typed rows, WinUI's `TableView` over the rows' own source: its columns made once from
//! the rows' type, each cell read from its row, the first led by the row's glyph, each in its
//! tone; numbers on the right. Panels and the report's domains are both drawn by it.
//!
//! The invariant: the columns are made once, as the table is first shown, so the user's widths and
//! sort stay while its rows change; and a cell is drawn from its row alone.

use std::fmt;
use std::marker::PhantomData;

use waft::{
    Align, AlignmentGeometry, App, BuildContext, EdgeInsetsGeometry, Handle, IntoWidget, Padding,
    Ref, Row, State, StateData, StatefulWidget, WidgetRef,
};
use waft_winui::{
    GridLength, TableView, TableViewColumn, TableViewDensity, TableViewGridLinesVisibility,
    TableViewSelection, TableViewSelectionMode, TableViewSource, TextBlockStyle, ThemeResources,
};

use super::{code_line, glyph, line, said, toned};
use crate::logic::panels::{Cell, Tabular, Width};

/// A table of `R`s.
pub(super) struct Table<R: 'static> {
    pub(super) rows: Ref<TableViewSource<R>>,
    /// The row selected, for a table whose rows have a detail.
    pub(super) selection: Option<Ref<TableViewSelection>>,
    /// What it says while it has no row.
    pub(super) empty: &'static str,
    /// Whether the detail pane is open beside it, so the columns that step out are out.
    pub(super) narrowed: bool,
}

impl<R> fmt::Debug for Table<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Table").field("empty", &self.empty).finish()
    }
}

impl<R: Tabular> StatefulWidget for Table<R> {
    type State = TableState<R>;

    fn create_state(&self) -> TableState<R> {
        TableState {
            state: StateData::new(),
            columns: Vec::new(),
            narrow: Vec::new(),
            rows: PhantomData,
        }
    }
}

pub(super) struct TableState<R: Tabular> {
    state: StateData<Table<R>>,
    columns: Vec<Ref<TableViewColumn<R>>>,
    /// The columns shown while the detail pane is open: the same columns, those stepping out
    /// left out.
    narrow: Vec<Ref<TableViewColumn<R>>>,
    rows: PhantomData<R>,
}

impl<R: Tabular> State for TableState<R> {
    type Widget = Table<R>;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let columns: Vec<Ref<TableViewColumn<R>>> = R::columns()
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let numeric = column.numeric;
                let made =
                    TableViewColumn::template(app, column.title, move |app, context, row: &R| {
                        let leading = if index == 0 { row.leading() } else { None };
                        cell(app, context, row.cell_at(index), leading, numeric)
                    });
                made.set_custom_sort_comparer(app, move |a: &R, b: &R| {
                    a.cell_at(index).text.cmp(&b.cell_at(index).text)
                });
                made.set_width(
                    app,
                    match column.width {
                        Width::Points(points) => GridLength::pixel(points),
                        Width::Rest => GridLength::STAR,
                    },
                );
                made
            })
            .collect();
        let narrow = (R::columns().iter().zip(&columns))
            .filter(|(column, _)| !column.steps_out)
            .map(|(_, made)| made.clone())
            .collect();
        let state = app.get_mut(self);
        state.columns = columns;
        state.narrow = narrow;
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let widget = self.widget(app);
        let (rows, selection, empty) =
            (widget.rows.clone(), widget.selection.clone(), widget.empty);
        let state = app.get(self);
        let columns = if self.widget(app).narrowed {
            state.narrow.clone()
        } else {
            state.columns.clone()
        };
        let mut table = TableView::new(rows, columns)
            .density(TableViewDensity::Compact)
            .grid_lines_visibility(TableViewGridLinesVisibility::Horizontal)
            .empty_template(move |app, context| {
                let resources = ThemeResources::of(app, context);
                said(empty, &resources)
            });
        table = match selection {
            Some(selection) => table
                .selection(selection)
                .selection_mode(TableViewSelectionMode::Single),
            None => table.selection_mode(TableViewSelectionMode::None),
        };
        let _ = context;
        table.into_widget()
    }
}

/// A cell: its text in its tone, led by `leading` where given, on the right where `numeric`.
fn cell(
    app: &mut App,
    context: BuildContext,
    cell: Cell,
    leading: Option<crate::logic::detail::Glyph>,
    numeric: bool,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let color = toned(cell.tone, &resources);
    let text = if cell.code {
        code_line(&cell.text, TextBlockStyle::Body, color, &resources)
    } else {
        line(&cell.text, TextBlockStyle::Body, color, &resources)
    };
    let content = match leading {
        Some(leading) => Row::new()
            .spacing(8.0)
            .children([
                glyph(leading, resources.text_fill_color_secondary),
                waft::Flexible::new(text).into_widget(),
            ])
            .into_widget(),
        None => text,
    };
    let alignment = if numeric {
        AlignmentGeometry::CENTER_RIGHT
    } else {
        AlignmentGeometry::CENTER_LEFT
    };
    // A template's cell is padded by its template, as WinUI's is; a text column's own padding.
    let [left, top, right, bottom] = resources.table_view_cell_padding_compact;
    Padding::new(EdgeInsetsGeometry::from_ltrb(left, top, right, bottom))
        .child(Align::new().alignment(alignment).child(content))
        .into_widget()
}
