//! One page in the preview: a white sheet of the page's size at the preview's scale, with a
//! faint shadow, showing the page's picture stretched over it. Building the sheet asks for the
//! picture at the screen's density where the one shown is of another text or another size, so
//! the old picture stays until the new one is drawn, and a sheet out of sight asks for nothing.

use waft::{
    App, BlurStyle, BoxDecoration, BoxFit, BoxShadow, BuildContext, Center, Color, DecoratedBox,
    EdgeInsetsGeometry, ImageInfo, IntoWidget, MediaQuery, Offset, Padding, RawImage, SizedBox,
    StatelessWidget, WidgetRef,
};

use crate::documents_app::DocumentsApp;
use crate::page_images::{densest, want_page};

/// Half the room between two pages, in logical points.
pub(crate) const HALF_GAP: f64 = 8.0;

const PAPER: Color = Color::from_argb(255, 255, 255, 255);
const SHADOW: Color = Color::from_argb(40, 0, 0, 0);

#[derive(Debug)]
pub(crate) struct PageSheet {
    pub(crate) documents: DocumentsApp,
    pub(crate) index: usize,
    /// Logical pixels to a point.
    pub(crate) scale: f64,
}

impl StatelessWidget for PageSheet {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let Some(size) = self.documents.pages.read(app).size(self.index) else {
            return SizedBox::new().into_widget();
        };
        let screen_density = MediaQuery::device_pixel_ratio_of(app, context);
        let density = (self.scale * screen_density).min(densest(size));
        want_page(app, &self.documents, self.index, density);
        let picture = self.documents.page_images.read(app).image(self.index);
        let sheet = SizedBox::new()
            .width(size[0] * self.scale)
            .height(size[1] * self.scale)
            .child(DecoratedBox::new(paper()).child(shown(picture)));
        Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, HALF_GAP, 0.0, HALF_GAP))
            .child(Center::new().child(sheet))
            .into_widget()
    }
}

/// The picture over the whole sheet; the bare sheet until there is one.
fn shown(picture: Option<waft::ui::Image>) -> WidgetRef {
    match picture {
        Some(picture) => RawImage::new(Some(ImageInfo::new(picture)))
            .fit(BoxFit::Fill)
            .into_widget(),
        None => SizedBox::new().into_widget(),
    }
}

fn paper() -> BoxDecoration {
    BoxDecoration::new()
        .color(PAPER)
        .box_shadow(vec![BoxShadow {
            color: SHADOW,
            offset: Offset::new(0.0, 1.0),
            blur_radius: 4.0,
            spread_radius: 0.0,
            blur_style: BlurStyle::Normal,
        }])
}
