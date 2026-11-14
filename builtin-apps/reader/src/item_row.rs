//! One item in the list: its title, in strong type until it is read, with a dot beside it; the
//! first lines of its text; the feed it is of and when it was published; and its picture at the
//! right, where it has one. The row lights under the pointer, and a click opens the item.

use std::rc::Rc;

use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BoxShape, BuildContext, ClipRRect, Column, Container,
    CrossAxisAlignment, DecoratedBox, EdgeInsetsGeometry, Expanded, GestureDetector, Handle,
    IntoWidget, Listener, MainAxisSize, MouseRegion, Padding, Row, SizedBox, State, StateData,
    StatefulWidget, Text, TextOverflow, WidgetRef,
};
use waft_winui::{TextBlockStyle, ThemeResources};

use crate::library::ItemRef;
use crate::picture_view::{PictureView, Shape};
use crate::reader_app::ReaderApp;
use crate::showing::read_item;

/// The space a row keeps within its lit ground, above, below and at its right, in logical points.
pub(crate) const ROW_PADDING: f64 = 12.0;

/// The room a row's picture takes, in logical points.
const PICTURE_WIDTH: f64 = 96.0;
const PICTURE_HEIGHT: f64 = 72.0;

/// The dot beside an item not read, and the room left for it beside one that is.
const DOT_SIZE: f64 = 8.0;
pub(crate) const DOT_ROOM: f64 = 20.0;

const CORNER_RADIUS: f64 = 6.0;

/// What a row shows of an item.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowData {
    pub(crate) item: ItemRef,
    pub(crate) title: String,
    pub(crate) preview: String,
    /// The feed the item is of and when it was published, on one line.
    pub(crate) line: String,
    pub(crate) picture: Option<String>,
    pub(crate) read: bool,
}

#[derive(Debug)]
pub(crate) struct ItemRow {
    pub(crate) reader: ReaderApp,
    pub(crate) data: RowData,
}

impl StatefulWidget for ItemRow {
    type State = ItemRowState;

    fn create_state(&self) -> ItemRowState {
        ItemRowState {
            state: StateData::new(),
            under_pointer: false,
        }
    }
}

pub(crate) struct ItemRowState {
    state: StateData<ItemRow>,
    under_pointer: bool,
}

impl State for ItemRowState {
    type Widget = ItemRow;
    waft::state_accessors!();

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let widget = self.widget(app);
        let (reader, item) = (widget.reader.clone(), widget.data.item.clone());
        let mut parts = vec![
            unread_dot(&resources, widget.data.read),
            Expanded::new(words(&resources, &widget.data)).into_widget(),
        ];
        parts.extend(picture_of(&widget.reader, &widget.data));
        let lit = if app.get(self).under_pointer {
            resources.subtle_fill_color_secondary
        } else {
            resources.subtle_fill_color_transparent
        };
        let row = Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(lit)
                    .border_radius(BorderRadiusGeometry::circular(CORNER_RADIUS)),
            )
            .padding(EdgeInsetsGeometry::from_ltrb(
                0.0,
                ROW_PADDING,
                ROW_PADDING,
                ROW_PADDING,
            ))
            .child(
                Row::new()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .children(parts),
            );
        let open = Listener::new(move |app: &mut App| read_item(app, &reader, item.clone()));
        MouseRegion::new()
            .on_enter(Rc::new(move |app, _| self.set_under_pointer(app, true)))
            .on_exit(Rc::new(move |app, _| self.set_under_pointer(app, false)))
            .child(GestureDetector::new().on_tap(open).child(row))
            .into_widget()
    }
}

impl ItemRowState {
    fn set_under_pointer(self: Handle<Self>, app: &mut App, under_pointer: bool) {
        if self.mounted(app) {
            self.set_state(app, |state| state.under_pointer = under_pointer);
        }
    }
}

/// The dot of an item not read, in the accent colour, in the room every row leaves for it.
fn unread_dot(resources: &ThemeResources, read: bool) -> WidgetRef {
    let dot = if read {
        SizedBox::shrink().into_widget()
    } else {
        let decoration = BoxDecoration::new()
            .color(resources.accent_fill_color_default_brush)
            .shape(BoxShape::Circle);
        SizedBox::new()
            .width(DOT_SIZE)
            .height(DOT_SIZE)
            .child(DecoratedBox::new(decoration))
            .into_widget()
    };
    SizedBox::new()
        .width(DOT_ROOM)
        .child(Padding::new(EdgeInsetsGeometry::from_ltrb(6.0, 6.0, 0.0, 0.0)).child(dot))
        .into_widget()
}

/// The item's title, the first lines of its text, and the line beneath.
fn words(resources: &ThemeResources, data: &RowData) -> WidgetRef {
    let (title_style, title_colour) = if data.read {
        (TextBlockStyle::Body, resources.text_fill_color_secondary)
    } else {
        (
            TextBlockStyle::BodyStrong,
            resources.text_fill_color_primary,
        )
    };
    let mut lines = vec![
        Text::new(data.title.clone())
            .style(title_style.text_style(resources, title_colour))
            .max_lines(2)
            .overflow(TextOverflow::Ellipsis)
            .into_widget(),
    ];
    if !data.preview.is_empty() {
        let style = TextBlockStyle::Body.text_style(resources, resources.text_fill_color_secondary);
        lines.push(
            Text::new(data.preview.clone())
                .style(style)
                .max_lines(2)
                .overflow(TextOverflow::Ellipsis)
                .into_widget(),
        );
    }
    let style = TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_tertiary);
    lines.push(
        Text::new(data.line.clone())
            .style(style)
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .into_widget(),
    );
    Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(4.0)
        .children(lines)
        .into_widget()
}

/// The item's picture in its room at the row's right, its corners rounded.
fn picture_of(reader: &ReaderApp, data: &RowData) -> Option<WidgetRef> {
    let address = data.picture.clone()?;
    let picture = PictureView {
        reader: reader.clone(),
        address,
        shape: Shape::Filling,
        description: String::new(),
    };
    let room = SizedBox::new()
        .width(PICTURE_WIDTH)
        .height(PICTURE_HEIGHT)
        .child(
            ClipRRect::new()
                .border_radius(BorderRadiusGeometry::circular(CORNER_RADIUS))
                .child(picture),
        );
    let beside = Padding::new(EdgeInsetsGeometry::from_ltrb(16.0, 0.0, 0.0, 0.0)).child(room);
    Some(beside.into_widget())
}
