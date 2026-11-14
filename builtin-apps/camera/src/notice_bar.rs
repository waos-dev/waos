//! What the window says of a command, in a Fluent info bar the user closes; nothing while there
//! is nothing to say.

use waft::{
    App, BuildContext, EdgeInsetsGeometry, IntoWidget, Padding, SizedBox, StatelessWidget,
    WidgetRef,
};
use waft_winui::InfoBar;

use crate::camera_app::CameraApp;

#[derive(Debug)]
pub(crate) struct NoticeBar {
    pub(crate) camera: CameraApp,
}

impl StatelessWidget for NoticeBar {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let Some(notice) = self.camera.notice.read(app).clone() else {
            return SizedBox::shrink().into_widget();
        };
        let camera = self.camera.clone();
        let bar = InfoBar::new(true, move |app, open| {
            if !open {
                camera.dismiss_notice(app);
            }
        })
        .severity(notice.severity)
        .message(notice.said);
        Padding::new(EdgeInsetsGeometry::all(12.0))
            .child(bar)
            .into_widget()
    }
}
