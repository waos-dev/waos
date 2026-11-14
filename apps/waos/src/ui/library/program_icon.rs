//! A program's icon as the library shows it: its PNG clipped to a rounded square, as its package
//! draws it, with nothing behind it; where there is no picture to show, as for a program whose
//! package has none or one a source lists before it is installed, WaOS's default icon, Apple's
//! icon-template lines on blueprint blue (`assets/program-default.png`, drawn by
//! `assets/draw_icons.py`), as a Mac shows its generic app icon; and a badge overhanging its
//! trailing top corner where one is given. Plain Waft widgets: the badge itself is the layout's.
//!
//! The invariant: the icon takes exactly `side` by `side` points whatever it shows, so a grid of
//! icons lines up whether or not a picture decoded, and the badge takes no room of its own.

use std::sync::Arc;

use waft::{
    App, BorderRadiusGeometry, BoxFit, BuildContext, Clip, ClipRRect, Image, IntoWidget,
    Positioned, SizedBox, Stack, StatelessWidget, WidgetRef,
};

/// How round an icon's corners are, as a share of its side: iOS's home screen's, 13.5 points on
/// a 60-point icon.
const CORNER_SHARE: f64 = 0.225;

/// The icon a program with no picture of its own shows.
const DEFAULT_PNG: &[u8] = include_bytes!("../../../assets/program-default.png");

/// How far a badge overhangs the icon's corner, as a share of the badge's own side.
const BADGE_OVERHANG_SHARE: f64 = 0.35;

/// A program's icon.
#[derive(Clone, Debug)]
pub(crate) struct ProgramIcon {
    /// Its picture, a PNG; none to show the default icon.
    pub(crate) png: Option<Arc<[u8]>>,
    pub(crate) side: f64,
    /// What overhangs its trailing top corner, with that badge's side; none for nothing.
    pub(crate) badge: Option<(WidgetRef, f64)>,
}

impl StatelessWidget for ProgramIcon {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let icon = self.rounded_square();
        let Some((badge, badge_side)) = self.badge.clone() else {
            return icon;
        };
        let overhang = -badge_side * BADGE_OVERHANG_SHARE;
        let badge = Positioned {
            top: Some(overhang),
            right: Some(overhang),
            ..Positioned::new(badge)
        };
        SizedBox::new()
            .width(self.side)
            .height(self.side)
            .child(
                Stack::new()
                    .clip_behavior(Clip::None)
                    .children([icon, badge.into_widget()]),
            )
            .into_widget()
    }
}

impl ProgramIcon {
    /// The picture, or the default icon, clipped to the icon's rounded square.
    fn rounded_square(&self) -> WidgetRef {
        let png = self.png.clone().unwrap_or_else(|| Arc::from(DEFAULT_PNG));
        let picture = Image::memory(png)
            .width(self.side)
            .height(self.side)
            .fit(BoxFit::Cover);
        ClipRRect::new()
            .border_radius(BorderRadiusGeometry::circular(self.side * CORNER_SHARE))
            .child(
                SizedBox::new()
                    .width(self.side)
                    .height(self.side)
                    .child(picture),
            )
            .into_widget()
    }
}
