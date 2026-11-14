//! An item's blocks as the reading page lays them out, one widget to a block: headings, paragraphs,
//! list items behind their markers, quotations behind a bar, code on a quiet ground that scrolls
//! sideways, pictures with what is said of them beneath, and rules.

use waft::ui::TextAlign;
use waft::{
    App, Axis, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxDecoration, BuildContext,
    ClipRRect, ColoredBox, Container, CrossAxisAlignment, EdgeInsetsGeometry, Expanded, IntoWidget,
    Padding, Row, SingleChildScrollView, SizedBox, Text, WidgetRef,
};
use waft_winui::ThemeResources;

use crate::article::{Block, Marker, Span};
use crate::picture_view::{PictureView, Shape};
use crate::reader_app::ReaderApp;
use crate::rich_text::{RichText, TextKind, code_style};

/// The room a list item's marker takes at each depth, in logical points.
const MARKER_ROOM: f64 = 28.0;

const CORNER_RADIUS: f64 = 6.0;

/// `block` as a widget, with the space above it that sets it apart from the block before.
pub(crate) fn block_view(
    app: &mut App,
    context: BuildContext,
    reader: &ReaderApp,
    block: &Block,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let text = |spans: &[Span], kind| text(reader, spans, kind, TextAlign::Start);
    let (above, view) = match block {
        Block::Heading { level, spans } => (28.0, text(spans, TextKind::Heading(*level))),
        Block::Paragraph(spans) => (14.0, text(spans, TextKind::Body)),
        Block::ListItem {
            marker,
            depth,
            spans,
        } => (
            8.0,
            list_item(&resources, *marker, *depth, text(spans, TextKind::Body)),
        ),
        Block::Quote(spans) => (14.0, quotation(&resources, text(spans, TextKind::Quote))),
        Block::Code(code) => (14.0, code_view(&resources, code)),
        Block::Picture {
            address,
            description,
        } => (20.0, picture_view(reader, address, description)),
        Block::Caption(spans) => (
            8.0,
            self::text(reader, spans, TextKind::Caption, TextAlign::Center),
        ),
        Block::Rule => (24.0, rule(&resources)),
    };
    Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, above, 0.0, 0.0))
        .child(view)
        .into_widget()
}

/// A picture of the reading page: whole, its corners rounded.
pub(crate) fn picture_view(reader: &ReaderApp, address: &str, description: &str) -> WidgetRef {
    let picture = PictureView {
        reader: reader.clone(),
        address: address.to_owned(),
        shape: Shape::Whole,
        description: description.to_owned(),
    };
    ClipRRect::new()
        .border_radius(BorderRadiusGeometry::circular(CORNER_RADIUS))
        .child(picture)
        .into_widget()
}

fn text(reader: &ReaderApp, spans: &[Span], kind: TextKind, align: TextAlign) -> WidgetRef {
    RichText {
        reader: reader.clone(),
        spans: spans.to_vec(),
        kind,
        align,
    }
    .into_widget()
}

/// `words` behind `marker`, set in as far as the item is `depth` lists deep.
fn list_item(resources: &ThemeResources, marker: Marker, depth: u8, words: WidgetRef) -> WidgetRef {
    let said = match marker {
        Marker::Bullet => "•".to_owned(),
        Marker::Number(number) => format!("{number}."),
        Marker::Continued => String::new(),
    };
    let style = TextKind::Body
        .style(resources)
        .color(resources.text_fill_color_secondary);
    let marker = SizedBox::new()
        .width(MARKER_ROOM * f64::from(depth.max(1)))
        .child(
            Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 0.0, 10.0, 0.0))
                .child(Text::new(said).style(style).text_align(TextAlign::End)),
        );
    Row::new()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children([marker.into_widget(), Expanded::new(words).into_widget()])
        .into_widget()
}

/// `words` behind a bar at their left, as a quotation is set.
fn quotation(resources: &ThemeResources, words: WidgetRef) -> WidgetRef {
    let bar = BorderSide::new(
        resources.divider_stroke_color_default,
        3.0,
        BorderStyle::Solid,
        BorderSide::STROKE_ALIGN_INSIDE,
    );
    let at_the_left = Border {
        left: bar,
        ..Border::from_border_side(BorderSide::NONE)
    };
    Container::new()
        .decoration(BoxDecoration::new().border(at_the_left))
        .padding(EdgeInsetsGeometry::from_ltrb(16.0, 2.0, 0.0, 2.0))
        .child(words)
        .into_widget()
}

/// `code` as written, on a quiet ground, scrolling sideways where its lines are longer than the
/// column.
fn code_view(resources: &ThemeResources, code: &str) -> WidgetRef {
    let style = code_style(TextKind::Body.style(resources).font_size(13.5).height(1.45));
    let lines = SingleChildScrollView::new()
        .scroll_direction(Axis::Horizontal)
        .padding(EdgeInsetsGeometry::all(14.0))
        .child(Text::new(code).style(style).soft_wrap(false));
    let ground = BoxDecoration::new()
        .color(resources.subtle_fill_color_secondary)
        .border_radius(BorderRadiusGeometry::circular(CORNER_RADIUS));
    // As wide as the column, however short the code's lines.
    SizedBox::new()
        .width(f64::INFINITY)
        .child(Container::new().decoration(ground).child(lines))
        .into_widget()
}

fn rule(resources: &ThemeResources) -> WidgetRef {
    SizedBox::new()
        .height(1.0)
        .child(ColoredBox::new(resources.divider_stroke_color_default))
        .into_widget()
}
