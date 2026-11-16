//! A program's icon: its picture, decoded by the system's codecs, clipped to a rounded square, as
//! WaOS's library shows it; a quiet square until it is decoded.
//!
//! The invariant: the icon takes exactly `side` by `side` points whatever it shows, so the rows
//! line up whether or not a picture came.

use waft::ui::Image;
use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BoxFit, BuildContext, ClipRRect, Color, DecoratedBox,
    ImageInfo, IntoWidget, RawImage, SizedBox, StatelessWidget, WidgetRef,
};

/// How round an icon's corners are, as a share of its side: iOS's home screen's.
const CORNER_SHARE: f64 = 0.225;

/// What shows until the picture is decoded.
const WAITING: Color = Color::from_argb(24, 128, 128, 128);

#[derive(Clone, Debug)]
pub(crate) struct ProgramIcon {
    /// Its picture; none until it is decoded.
    pub(crate) image: Option<Image>,
    pub(crate) side: f64,
}

impl StatelessWidget for ProgramIcon {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let radius = BorderRadiusGeometry::circular(self.side * CORNER_SHARE);
        let square = SizedBox::new().width(self.side).height(self.side);
        match self.image.clone() {
            Some(image) => ClipRRect::new()
                .border_radius(radius)
                .child(square.child(RawImage::new(Some(ImageInfo::new(image))).fit(BoxFit::Cover)))
                .into_widget(),
            None => DecoratedBox::new(BoxDecoration::new().color(WAITING).border_radius(radius))
                .child(square)
                .into_widget(),
        }
    }
}
