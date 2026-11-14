//! The controls under the picture: the newest photo, which opens the photos kept; the shutter in
//! the middle; and, where the system has more than one camera, the button that switches to the
//! next.

use waft::{
    Align, AlignmentGeometry, App, BuildContext, EdgeInsetsGeometry, Expanded, GestureDetector,
    IntoWidget, Listener, Padding, Row, SizedBox, StatelessWidget, WidgetRef,
};
use waft_winui::{Button, ButtonStyle, ToolTip, ToolTipService};

use crate::camera_app::CameraApp;
use crate::icons::{self, icon};
use crate::photo_tile::photo_tile;
use crate::photos_page;
use crate::shooting::take_photo;
use crate::shutter_button::ShutterButton;
use crate::viewfinder::switch_camera;

/// The newest photo's tile, across, in logical points.
const NEWEST_SIDE: f64 = 52.0;

#[derive(Debug)]
pub(crate) struct Controls {
    pub(crate) camera: CameraApp,
}

impl StatelessWidget for Controls {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let row = Row::new().children([
            beside(
                AlignmentGeometry::CENTER_LEFT,
                self.newest_photo(app, context),
            ),
            self.shutter(app),
            beside(AlignmentGeometry::CENTER_RIGHT, self.camera_switch(app)),
        ]);
        Padding::new(EdgeInsetsGeometry::from_ltrb(24.0, 8.0, 24.0, 20.0))
            .child(row)
            .into_widget()
    }
}

impl Controls {
    /// The shutter, which takes a photo while the camera runs and none is being taken.
    fn shutter(&self, app: &App) -> WidgetRef {
        let viewfinder = self.camera.viewfinder.read(app);
        let ready = viewfinder.running().is_some() && !viewfinder.is_taking_photo();
        let camera = self.camera.clone();
        let press = ready.then(|| Listener::new(move |app: &mut App| take_photo(app, &camera)));
        ShutterButton { press }.into_widget()
    }

    /// The newest photo, which opens the photos kept at a tap; room for it while none is kept.
    fn newest_photo(&self, app: &App, context: BuildContext) -> WidgetRef {
        let Some(newest) = self.camera.roll.read(app).newest() else {
            return SizedBox::new()
                .width(NEWEST_SIDE)
                .height(NEWEST_SIDE)
                .into_widget();
        };
        let tile = photo_tile(newest.thumbnail.clone(), NEWEST_SIDE, None);
        let (camera, path) = (self.camera.clone(), newest.path.clone());
        let open = Listener::new(move |app: &mut App| {
            photos_page::open(app, context, &camera, path.clone());
        });
        let button = GestureDetector::new().on_tap(open).child(tile);
        ToolTipService::new(button, ToolTip::text("Photos")).into_widget()
    }

    /// The button that runs the next camera, where the system lists more than one.
    fn camera_switch(&self, app: &App) -> WidgetRef {
        if !self.camera.viewfinder.read(app).has_another_camera() {
            return SizedBox::shrink().into_widget();
        }
        let viewfinder = self.camera.viewfinder.clone();
        let switch = Listener::new(move |app: &mut App| switch_camera(app, &viewfinder));
        let button =
            Button::new(icon(icons::CAMERA_SWITCH, 22.0), switch).style(ButtonStyle::Subtle);
        ToolTipService::new(button, ToolTip::text("Switch camera")).into_widget()
    }
}

/// `control` in the room beside the shutter, held to the side `alignment` names.
fn beside(alignment: AlignmentGeometry, control: WidgetRef) -> WidgetRef {
    Expanded::new(Align::new().alignment(alignment).child(control)).into_widget()
}
