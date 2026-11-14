//! A photo as a small square: its thumbnail cropped to fill a rounded tile, ringed in the accent
//! colour where it is the one selected; a picture's outline on grey until the thumbnail is
//! decoded.

use waft::ui::Image;
use waft::{
    Border, BorderRadiusGeometry, BorderStyle, BoxDecoration, BoxFit, Center, ClipRRect, Color,
    ColoredBox, DecoratedBox, EdgeInsetsGeometry, ImageInfo, IntoWidget, Padding, RawImage,
    SizedBox, WidgetRef,
};

use crate::icons::{self, icon};

/// What shows where there is no thumbnail yet, and the outline on it.
const NO_PICTURE: Color = Color::from_argb(255, 44, 44, 46);
const OUTLINE: Color = Color::from_argb(255, 142, 142, 147);

/// The room between the ring and the picture, and the ring's own width, in logical points.
const RING_GAP: f64 = 2.0;
const RING_WIDTH: f64 = 2.0;

/// The tile's corner radius, in logical points.
const CORNER: f64 = 6.0;

/// A tile `side` points across showing `thumbnail`, ringed in `ring` where given.
pub(crate) fn photo_tile(thumbnail: Option<Image>, side: f64, ring: Option<Color>) -> WidgetRef {
    let picture: WidgetRef = match thumbnail {
        Some(image) => RawImage::new(Some(ImageInfo::new(image)))
            .fit(BoxFit::Cover)
            .into_widget(),
        None => ColoredBox::new(NO_PICTURE)
            .child(Center::new().child(icon(icons::IMAGE, side * 0.4).foreground(OUTLINE)))
            .into_widget(),
    };
    let rounded = ClipRRect::new()
        .border_radius(BorderRadiusGeometry::circular(CORNER - RING_GAP))
        .child(SizedBox::expand().child(picture));
    let ring_color = ring.unwrap_or(Color::from_argb(0, 0, 0, 0));
    let ringed = DecoratedBox::new(
        BoxDecoration::new()
            .border(Border::all(
                ring_color,
                RING_WIDTH,
                BorderStyle::Solid,
                -1.0,
            ))
            .border_radius(BorderRadiusGeometry::circular(CORNER + RING_WIDTH)),
    )
    .child(Padding::new(EdgeInsetsGeometry::all(RING_GAP + RING_WIDTH)).child(rounded));
    SizedBox::new()
        .width(side)
        .height(side)
        .child(ringed)
        .into_widget()
}
