//! A video's thumbnail: the still picture the system read of it, or decoded from where the library
//! kept it, an image drawn as any other, cropped to fill its room as a library's artwork is; a
//! film's outline on grey until the picture is read, and where there is none.

use waft_embedder::{Color, Image};
use waft_widgets::{
    BoxFit, Center, ClipRect, ColoredBox, ImageInfo, IntoWidget, RawImage, WidgetRef,
};

use crate::icons::{self, icon};

/// What shows where there is no picture, and the outline on it.
const NO_PICTURE: Color = Color::from_argb(255, 44, 44, 46);
const OUTLINE: Color = Color::from_argb(255, 142, 142, 147);

/// `image` filling its room, cut to it; the placeholder while it is none.
pub(crate) fn thumbnail(image: Option<Image>) -> WidgetRef {
    match image {
        Some(image) => ClipRect::new()
            .child(RawImage::new(Some(ImageInfo::new(image))).fit(BoxFit::Cover))
            .into_widget(),
        None => placeholder(),
    }
}

fn placeholder() -> WidgetRef {
    let film = icon(icons::VIDEO, 32.0).foreground(OUTLINE);
    ColoredBox::new(NO_PICTURE)
        .child(Center::new().child(film))
        .into_widget()
}
