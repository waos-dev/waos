//! A program's icon: its PNG clipped to a rounded square, as WaOS's library shows it; WaOS's
//! default icon until it is read, and for a program whose package has none.
//!
//! The invariant: the icon takes exactly `side` by `side` points whatever it shows, so the rows
//! line up whether or not a picture came.

use std::sync::Arc;

use waft::{
    App, BorderRadiusGeometry, BoxFit, BuildContext, ClipRRect, Image, IntoWidget, SizedBox,
    StatelessWidget, WidgetRef,
};

/// How round an icon's corners are, as a share of its side: iOS's home screen's.
const CORNER_SHARE: f64 = 0.225;

/// The icon a program with no picture of its own shows, WaOS's default.
const DEFAULT_PNG: &[u8] = include_bytes!("../assets/program-default.png");

#[derive(Clone, Debug)]
pub(crate) struct ProgramIcon {
    /// Its picture, a PNG; none for the default icon.
    pub(crate) png: Option<Arc<[u8]>>,
    pub(crate) side: f64,
}

impl StatelessWidget for ProgramIcon {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
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
