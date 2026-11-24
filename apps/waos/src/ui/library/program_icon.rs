//! A program's icon as the library shows it: its PNG, a full square as its package draws it,
//! given the shape, the shadow and the edge every icon has here, as iOS, Android and macOS draw
//! an app's artwork in the system's own shape rather than each app drawing its own; where there is no picture to show, as for a program whose
//! package has none or one a source lists before it is installed, WaOS's default icon, Apple's
//! icon-template lines on blueprint blue (`assets/program-default.png`, drawn by
//! `assets/draw_icons.py`), as a Mac shows its generic app icon. Plain Waft widgets.
//!
//! The invariant: the icon takes exactly `side` by `side` points whatever it shows, so a grid of
//! icons lines up whether or not a picture decoded.

use std::sync::Arc;

use waft::ui::Brightness;
use waft::{
    App, BlurStyle, BorderRadiusGeometry, BorderSide, BorderStyle, BoxFit, BoxShadow, BuildContext,
    ClipRSuperellipse, Color, DecoratedBox, DecorationPosition, Image, IntoWidget, MediaQuery,
    Offset, RoundedSuperellipseBorder, ShapeDecoration, SizedBox, StatelessWidget, WidgetRef,
};

/// How round an icon's corners are, as a share of its side: iOS's home screen's, 13.5 points on
/// a 60-point icon, in the continuous curve Apple's icons have.
const CORNER_SHARE: f64 = 0.225;

/// The shadow an icon casts, as shares of its side: soft and close under it, as macOS's Apps
/// shows its icons.
const SHADOW_COLOR: Color = Color::from_argb(46, 0, 0, 0);
const SHADOW_DROP_SHARE: f64 = 0.03;
const SHADOW_BLUR_SHARE: f64 = 0.09;

/// The hairline along an icon's edge, which keeps a light icon apart from a light background and
/// a dark one from a dark one.
const RIM_WIDTH: f64 = 0.5;
const RIM_ON_LIGHT: Color = Color::from_argb(26, 0, 0, 0);
const RIM_ON_DARK: Color = Color::from_argb(46, 255, 255, 255);

/// The icon a program with no picture of its own shows.
const DEFAULT_PNG: &[u8] = include_bytes!("../../../assets/program-default.png");

/// A program's icon.
#[derive(Clone, Debug)]
pub(crate) struct ProgramIcon {
    /// Its picture, a PNG; none to show the default icon.
    pub(crate) png: Option<Arc<[u8]>>,
    pub(crate) side: f64,
}

impl StatelessWidget for ProgramIcon {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let rim = match MediaQuery::platform_brightness_of(app, context) {
            Brightness::Dark => RIM_ON_DARK,
            Brightness::Light => RIM_ON_LIGHT,
        };
        self.rounded_square(rim)
    }
}

impl ProgramIcon {
    /// The picture, or the default icon, in the icon's shape: clipped to it, over a soft shadow,
    /// under a hairline along its edge.
    fn rounded_square(&self, rim: Color) -> WidgetRef {
        let png = self.png.clone().unwrap_or_else(|| Arc::from(DEFAULT_PNG));
        let picture = Image::memory(png)
            .width(self.side)
            .height(self.side)
            .fit(BoxFit::Cover);
        let corners = BorderRadiusGeometry::circular(self.side * CORNER_SHARE);
        let shape = |edge: BorderSide| RoundedSuperellipseBorder::new(edge, Some(corners));
        let shadow = BoxShadow::new(
            SHADOW_COLOR,
            Offset::new(0.0, self.side * SHADOW_DROP_SHARE),
            self.side * SHADOW_BLUR_SHARE,
            0.0,
            BlurStyle::Normal,
        );
        let edge = BorderSide::new(
            rim,
            RIM_WIDTH,
            BorderStyle::Solid,
            BorderSide::STROKE_ALIGN_INSIDE,
        );
        let clipped = ClipRSuperellipse::new()
            .border_radius(corners)
            .child(picture);
        let rimmed = DecoratedBox::new(ShapeDecoration::new(shape(edge)))
            .position(DecorationPosition::Foreground)
            .child(clipped);
        let shadowed =
            DecoratedBox::new(ShapeDecoration::new(shape(BorderSide::NONE)).shadows(vec![shadow]))
                .child(rimmed);
        SizedBox::new()
            .width(self.side)
            .height(self.side)
            .child(shadowed)
            .into_widget()
    }
}
