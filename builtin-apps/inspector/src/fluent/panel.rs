//! A live panel, as a table: the columns' titles, then a row an event or a use, the oldest first,
//! each with what is said beneath it; or why there is none. Drawn with what Waft has until WinUI's
//! table is ported, in its shape: columns by title and width, rows of cells.
//!
//! The invariant: a panel is drawn from its `Panel` alone, so every panel looks the same.

use waft::{
    Border, BorderSide, BorderStyle, BoxDecoration, Column as Stacked, CrossAxisAlignment,
    DecoratedBox, EdgeInsetsGeometry, Expanded, IntoWidget, ListView, MainAxisSize, Padding, Row,
    SizedBox, Text, WidgetRef,
};
use waft_winui::{TextBlockStyle, ThemeResources};

use super::{GAP, MARGIN, ROW_PADDING, one_line, paragraph, said};
use crate::panels::{Column, Panel};
use crate::words;

/// `panel` as a table, or why it has nothing: its program does not run, where nothing was `heard`.
pub(super) fn panel(panel: &dyn Panel, heard: bool, resources: &ThemeResources) -> WidgetRef {
    if !heard {
        return said(words::NOT_RUNNING, resources);
    }
    let shape = (panel.columns(), panel.wraps());
    let titles: Vec<String> = panel
        .columns()
        .iter()
        .map(|(title, _)| (*title).to_owned())
        .collect();
    let mut rows = vec![table_row(
        &titles,
        (panel.columns(), false),
        TextBlockStyle::BodyStrong,
        &[],
        resources,
    )];
    let drawn = panel.rows();
    if drawn.is_empty() {
        rows.push(paragraph(panel.nothing_yet(), resources));
    }
    rows.extend(drawn.iter().map(|row| {
        table_row(
            &row.cells,
            shape,
            TextBlockStyle::Body,
            &row.beneath,
            resources,
        )
    }));
    ListView::new()
        .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, GAP, MARGIN, MARGIN))
        .children(rows)
        .into_widget()
}

/// One row of a panel's table: each cell under its column, at its width, the cells of the columns
/// that share what is left wrapping where `wraps`; and what is said beneath it, a line each.
fn table_row(
    cells: &[String],
    (columns, wraps): (&[Column], bool),
    style: TextBlockStyle,
    beneath: &[String],
    resources: &ThemeResources,
) -> WidgetRef {
    let columns = cells.iter().zip(columns).map(|(cell, (_, width))| {
        if *width == 0.0 {
            let text = if wraps {
                wrapped(cell, style, resources)
            } else {
                one_line(cell, style, resources, true)
            };
            Expanded::new(text).into_widget()
        } else {
            let text = one_line(cell, style, resources, true);
            SizedBox::new().width(*width).child(text).into_widget()
        }
    });
    let line = Row::new()
        .spacing(GAP)
        .children(columns.collect::<Vec<_>>());
    let mut lines = vec![line.into_widget()];
    lines.extend(
        beneath
            .iter()
            .map(|line| one_line(line, TextBlockStyle::Caption, resources, false)),
    );
    let divider = resources.divider_stroke_color_default;
    let bottom = BorderSide::new(divider, 1.0, BorderStyle::Solid, -1.0);
    let border = Border {
        bottom,
        ..Border::from_border_side(BorderSide::NONE)
    };
    DecoratedBox::new(BoxDecoration::new().border(border))
        .child(
            Padding::new(EdgeInsetsGeometry::symmetric(ROW_PADDING, 0.0)).child(
                Stacked::new()
                    .main_axis_size(MainAxisSize::Min)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(lines),
            ),
        )
        .into_widget()
}

/// `text` as many lines as it takes, primary.
fn wrapped(text: &str, style: TextBlockStyle, resources: &ThemeResources) -> WidgetRef {
    Text::new(text.to_owned())
        .style(style.text_style(resources, resources.text_fill_color_primary))
        .into_widget()
}
