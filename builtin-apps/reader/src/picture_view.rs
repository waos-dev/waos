//! A picture from the network, shown where it is asked for: in a list's row, cut to fill the room
//! it is given, or on the reading page, whole, as wide as its column and no larger than it is.
//! Until it has loaded its room is a quiet grey; where it cannot be shown, a list's row keeps the
//! grey with a mark, and the reading page says what the picture was of.
//!
//! The picture is asked for as the widget is first built, which a long list does only for the
//! rows in view.

use waft::ui::Image;
use waft::{
    Align, AlignmentGeometry, App, AspectRatio, BoxConstraints, BoxFit, BuildContext, Center,
    ColoredBox, ConstrainedBox, Entity, Handle, ImageInfo, IntoWidget, RawImage, SizedBox, State,
    StateData, StatefulWidget, Text, WidgetRef,
};
use waft_winui::{TextBlockStyle, ThemeResources};

use crate::icons;
use crate::pictures::{Picture, PictureSize, picture};
use crate::reader_app::ReaderApp;

/// The shape of the room a picture on the reading page is given while it loads: wide, as most
/// illustrations are.
const LOADING_SHAPE: f64 = 16.0 / 9.0;

/// The tallest a picture on the reading page is drawn, in logical points, so that a tall one
/// leaves the text around it in sight.
const TALLEST: f64 = 440.0;

/// How a picture takes its room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// Cut to fill the room its parent gives it, for a list's row.
    Filling,
    /// Whole, as wide as its parent allows and no larger than its own pixels, for the reading
    /// page.
    Whole,
}

#[derive(Debug)]
pub(crate) struct PictureView {
    pub(crate) reader: ReaderApp,
    pub(crate) address: String,
    pub(crate) shape: Shape,
    /// What the picture is of, in words, said where it cannot be shown whole.
    pub(crate) description: String,
}

impl StatefulWidget for PictureView {
    type State = PictureViewState;

    fn create_state(&self) -> PictureViewState {
        PictureViewState {
            state: StateData::new(),
            picture: None,
        }
    }
}

pub(crate) struct PictureViewState {
    state: StateData<PictureView>,
    /// The picture asked for, as far as it has come.
    picture: Option<Entity<Picture>>,
}

impl PictureViewState {
    /// Asks for the picture the widget names, at the size its shape needs.
    fn ask(self: Handle<Self>, app: &mut App) {
        let widget = self.widget(app);
        let size = match widget.shape {
            Shape::Filling => PictureSize::Thumbnail,
            Shape::Whole => PictureSize::Full,
        };
        let (reader, address) = (widget.reader.clone(), widget.address.clone());
        let asked = picture(app, &reader, &address, size);
        app.get_mut(self).picture = Some(asked);
    }
}

impl State for PictureViewState {
    type Widget = PictureView;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        self.ask(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, old_widget: &PictureView) {
        if old_widget.address != self.widget(app).address {
            self.ask(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let shape = self.widget(app).shape;
        let picture = app.get(self).picture.clone();
        let picture = picture.map_or(Picture::Loading, |picture| picture.read(app).clone());
        match (picture, shape) {
            (Picture::Ready(image), Shape::Filling) => filling(image),
            (Picture::Ready(image), Shape::Whole) => whole(image),
            (Picture::Loading, Shape::Filling) => grey(app, context, None),
            (Picture::Loading, Shape::Whole) => loading(grey(app, context, None)),
            (Picture::Missing, Shape::Filling) => grey(app, context, Some(20.0)),
            (Picture::Missing, Shape::Whole) => {
                let description = self.widget(app).description.clone();
                described(app, context, description)
            }
        }
    }
}

/// `image`, cut to fill its room.
fn filling(image: Image) -> WidgetRef {
    RawImage::new(Some(ImageInfo::new(image)))
        .fit(BoxFit::Cover)
        .into_widget()
}

/// `image` whole, at the left of its room, as wide as the room, no taller than [`TALLEST`], and
/// no larger than a logical point to each of its pixels, the size a page on the web shows it at.
fn whole(image: Image) -> WidgetRef {
    let [width, height] = image.size();
    let shape = f64::from(width) / f64::from(height.max(1));
    let no_larger = BoxConstraints::new()
        .max_width(f64::from(width))
        .max_height(TALLEST.min(f64::from(height)));
    let drawn = RawImage::new(Some(ImageInfo::new(image))).fit(BoxFit::Contain);
    at_the_left(
        ConstrainedBox::new(no_larger)
            .child(AspectRatio::new(shape).child(drawn))
            .into_widget(),
    )
}

fn at_the_left(picture: WidgetRef) -> WidgetRef {
    Align::new()
        .alignment(AlignmentGeometry::TOP_LEFT)
        .child(picture)
        .into_widget()
}

/// `grey`, in the room a picture of the reading page is given while it loads.
fn loading(grey: WidgetRef) -> WidgetRef {
    let room = BoxConstraints::new().max_height(TALLEST);
    at_the_left(
        ConstrainedBox::new(room)
            .child(AspectRatio::new(LOADING_SHAPE).child(grey))
            .into_widget(),
    )
}

/// The quiet grey a picture's room shows without it, with the mark of a picture that could not be
/// shown at `mark` logical points where given.
fn grey(app: &mut App, context: BuildContext, mark: Option<f64>) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let room = ColoredBox::new(resources.subtle_fill_color_secondary);
    match mark {
        Some(size) => {
            let mark = icons::no_picture(size).foreground(resources.text_fill_color_tertiary);
            room.child(Center::new().child(mark)).into_widget()
        }
        None => room.child(SizedBox::expand()).into_widget(),
    }
}

/// What a picture that cannot be shown was of, in words; nothing where nothing says.
fn described(app: &mut App, context: BuildContext, description: String) -> WidgetRef {
    if description.is_empty() {
        return SizedBox::shrink().into_widget();
    }
    let resources = ThemeResources::of(app, context);
    let style = TextBlockStyle::Caption.text_style(&resources, resources.text_fill_color_tertiary);
    Text::new(format!("Picture: {description}"))
        .style(style)
        .into_widget()
}
