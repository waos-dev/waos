//! The detail pane beside a panel's table, as DevTools' pane beside its requests: what the row
//! selected was and what it reached, under its glyph, with a way to close the pane; why WaOS
//! refused it, in an error bar; and its facts, a WinUI `Expander` a section.
//!
//! The invariant: the pane draws the row's `Detail` alone, so it takes any panel's row; and it
//! keeps only which of its sections the user closed.

use std::collections::HashSet;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Expanded, Handle,
    IntoWidget, ListView, Listener, MainAxisSize, Padding, Row, SizedBox, State, StateData,
    StatefulWidget, Text, WidgetRef,
};
use waft_winui::{
    Brush, Button, ButtonStyle, ControlBorder, Expander, FluentSymbol, FontIcon, InfoBar,
    InfoBarSeverity, TextBlockStyle, ThemeResources,
};

use super::{GAP, caption, code_style, glyph, line, toned};
use crate::logic::detail::{Detail, Section};
use crate::words;

/// The width of a fact's label.
const LABEL_WIDTH: f64 = 96.0;

/// The detail of a row, and what closes the pane.
#[derive(Debug)]
pub(super) struct DetailPane {
    pub(super) detail: Detail,
    pub(super) close: Listener,
}

impl StatefulWidget for DetailPane {
    type State = DetailPaneState;

    fn create_state(&self) -> DetailPaneState {
        DetailPaneState {
            state: StateData::new(),
            closed: HashSet::new(),
        }
    }
}

pub(super) struct DetailPaneState {
    state: StateData<DetailPane>,
    /// The sections the user closed, by title.
    closed: HashSet<&'static str>,
}

impl DetailPaneState {
    fn toggle(self: Handle<Self>, app: &mut App, section: &'static str, open: bool) {
        self.set_state(app, |state| {
            if open {
                state.closed.remove(section);
            } else {
                state.closed.insert(section);
            }
        });
    }
}

impl State for DetailPaneState {
    type Widget = DetailPane;
    waft::state_accessors!();

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let (detail, close) = {
            let widget = self.widget(app);
            (widget.detail.clone(), widget.close.clone())
        };
        let mut parts = vec![head(&detail, close, &resources)];
        if let Some(refusal) = &detail.refusal {
            parts.push(
                InfoBar::new(true, |_app, _open| {})
                    .severity(InfoBarSeverity::Error)
                    .is_closable(false)
                    .title(words::REFUSED_BY_WAOS)
                    .message(refusal.said.clone())
                    .into_widget(),
            );
        }
        for section in &detail.sections {
            let open = !app.get(self).closed.contains(section.title);
            let title = section.title;
            parts.push(
                Expander::new(open, move |app, open| self.toggle(app, title, open))
                    .header(Text::new(title))
                    .content(facts(section, app, &resources))
                    .into_widget(),
            );
        }
        ControlBorder::new(
            Brush::Solid(resources.layer_fill_color_default),
            Brush::Solid(resources.card_stroke_color_default),
        )
        .border_thickness_ltrb([1.0, 0.0, 0.0, 0.0])
        .child(
            ListView::new()
                .padding(EdgeInsetsGeometry::all(GAP))
                .children(
                    parts
                        .into_iter()
                        .map(|part| {
                            Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, 8.0))
                                .child(part)
                                .into_widget()
                        })
                        .collect::<Vec<_>>(),
                ),
        )
        .into_widget()
    }
}

/// What the row was, under its glyph, with the way to close the pane.
fn head(detail: &Detail, close: Listener, resources: &ThemeResources) -> WidgetRef {
    let tone = if detail.refusal.is_some() {
        resources.system_fill_color_critical
    } else {
        resources.text_fill_color_secondary
    };
    let text = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children([
            line(
                &detail.title,
                TextBlockStyle::BodyStrong,
                resources.text_fill_color_primary,
                resources,
            ),
            Text::new(detail.subtitle.clone())
                .style(code_style(
                    TextBlockStyle::Caption,
                    resources.text_fill_color_secondary,
                    resources,
                ))
                .max_lines(3)
                .into_widget(),
        ]);
    Row::new()
        .spacing(8.0)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children([
            Padding::new(EdgeInsetsGeometry::only(0.0, 2.0, 0.0, 0.0))
                .child(glyph(detail.glyph, tone))
                .into_widget(),
            Expanded::new(text).into_widget(),
            Button::new(
                FontIcon::symbol(FluentSymbol::Dismiss).font_size(12.0),
                close,
            )
            .style(ButtonStyle::Subtle)
            .into_widget(),
        ])
        .into_widget()
}

/// A section's facts, a label and its value a line.
fn facts(section: &Section, app: &App, resources: &ThemeResources) -> WidgetRef {
    let lines = section.fields.iter().map(|field| {
        Row::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([
                SizedBox::new()
                    .width(LABEL_WIDTH)
                    .child(caption(field.label, resources))
                    .into_widget(),
                Expanded::new(
                    Text::new(crate::ui::said(&field.value, app)).style(if field.code {
                        code_style(
                            TextBlockStyle::Caption,
                            toned(field.tone, resources),
                            resources,
                        )
                    } else {
                        TextBlockStyle::Caption.text_style(resources, toned(field.tone, resources))
                    }),
                )
                .into_widget(),
            ])
            .into_widget()
    });
    Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(6.0)
        .children(lines.collect::<Vec<_>>())
        .into_widget()
}
