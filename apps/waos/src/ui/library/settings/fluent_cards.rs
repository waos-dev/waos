//! The cards the desktop's settings pages are made of, as Windows's Settings draws a setting:
//! a card of one row or several, each row its mark, its name over what it does, and its control
//! at the trailing side; rows of one card are parted by a line, as an expander's are.
//!
//! The invariant: a control is in a row's trailing side or the row's whole content, never
//! loose on the page, so every setting reads the same way.

use waft::{
    App, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxDecoration, ColoredBox, Column,
    CrossAxisAlignment, DecoratedBox, EdgeInsetsGeometry, Expanded, IntoWidget, MainAxisSize,
    Padding, Row, SizedBox, Text, WidgetRef,
};
use waft_winui::{TextBlockStyle, ThemeResources, ThemeScope, ToggleSwitch};

use crate::ui::library::words;

/// How round a card's corners are: WinUI's control corners.
pub(crate) const CARD_CORNER_RADIUS: f64 = 4.0;

/// The room between two cards, and inside a row around what it holds.
pub(crate) const CARD_GAP: f64 = 4.0;
const ROW_PADDING: [f64; 2] = [16.0, 12.0];

/// The least height of a row, so a row of a name alone is as tall as one with a control.
const ROW_MIN_HEIGHT: f64 = 32.0;

/// `rows` as one card: on the card's ground, inside its edge, a line between two rows.
pub(crate) fn card(resources: &ThemeResources, rows: Vec<WidgetRef>) -> WidgetRef {
    let surface = BoxDecoration {
        color: Some(resources.card_background_fill_color_default.into()),
        border: Some(
            Border::all(
                resources.card_stroke_color_default,
                1.0,
                BorderStyle::Solid,
                BorderSide::STROKE_ALIGN_INSIDE,
            )
            .into(),
        ),
        border_radius: Some(BorderRadiusGeometry::circular(CARD_CORNER_RADIUS)),
        ..BoxDecoration::default()
    };
    let mut parted = Vec::new();
    for row in rows {
        if !parted.is_empty() {
            let line = ColoredBox::new(resources.card_stroke_color_default)
                .child(SizedBox::new().height(1.0));
            parted.push(line.into_widget());
        }
        parted.push(
            Padding::new(EdgeInsetsGeometry::symmetric(
                ROW_PADDING[1],
                ROW_PADDING[0],
            ))
            .child(row)
            .into_widget(),
        );
    }
    let rows = Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(parted);
    Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, CARD_GAP))
        .child(DecoratedBox::new(surface).child(rows))
        .into_widget()
}

/// A card's row: its `mark` where it has one, `name` over `detail` where that is said, and its
/// `control` at the trailing side where it has one.
pub(crate) fn card_row(
    resources: &ThemeResources,
    mark: Option<WidgetRef>,
    name: &str,
    detail: Option<&str>,
    control: Option<WidgetRef>,
) -> WidgetRef {
    let mut lines = vec![
        Text::new(name)
            .style(TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary))
            .into_widget(),
    ];
    if let Some(detail) = detail {
        lines.push(note(resources, detail));
    }
    let text = Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .children(lines);
    let said = Row::new().spacing(16.0).children(
        mark.into_iter()
            .chain([Expanded::new(text).into_widget()])
            .chain(control),
    );
    waft::ConstrainedBox::new(waft::BoxConstraints::new().min_height(ROW_MIN_HEIGHT))
        .child(said)
        .into_widget()
}

/// A switch that is `on` or off as Windows's Settings sets one in a card: On or Off said
/// before it, the switch at the row's end with no words of its own and no width kept for them,
/// telling `changed` when the user flips it.
pub(crate) fn card_switch(
    resources: &ThemeResources,
    on: bool,
    changed: impl Fn(&mut App, bool) + 'static,
) -> WidgetRef {
    let said = if on { words::ON } else { words::OFF };
    let switch = ToggleSwitch::new(on, changed)
        .on_content(SizedBox::shrink())
        .off_content(SizedBox::shrink());
    // WinUI's way to restyle one control: the resource its template reads.
    let switch = ThemeScope::overriding(switch).any(|resources| {
        resources.toggle_switch_theme_min_width = 0.0;
    });
    Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(12.0)
        .children([
            Text::new(said)
                .style(
                    TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary),
                )
                .into_widget(),
            switch.into_widget(),
        ])
        .into_widget()
}

/// What a card says where it sets nothing: caption-sized, in the secondary colour.
pub(crate) fn note(resources: &ThemeResources, text: &str) -> WidgetRef {
    Text::new(text)
        .style(TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_secondary))
        .into_widget()
}
