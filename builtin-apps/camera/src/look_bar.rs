//! The looks to choose among, by name, in a Fluent selector bar under the picture: the one chosen
//! is the one the live picture is drawn through and the next photo is taken in.

use waft::{App, BuildContext, Center, IntoWidget, StatelessWidget, WidgetRef};
use waft_winui::{SelectorBar, SelectorBarItem};

use crate::camera_app::CameraApp;
use crate::looks::Look;

#[derive(Debug)]
pub(crate) struct LookBar {
    pub(crate) camera: CameraApp,
}

impl StatelessWidget for LookBar {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let chosen = self.camera.viewfinder.read(app).look();
        let items = Look::ALL
            .into_iter()
            .map(|look| SelectorBarItem::new(look.name(), look.name()))
            .collect();
        let selected = Look::ALL.iter().position(|look| *look == chosen);
        let viewfinder = self.camera.viewfinder.clone();
        let bar = SelectorBar::new(items, selected, move |app, asked| {
            let Some(look) = asked.and_then(|index| Look::ALL.get(index)) else {
                return;
            };
            viewfinder.update(app, |viewfinder, cx| {
                viewfinder.set_look(*look);
                cx.notify();
            });
        });
        Center::new().child(bar).into_widget()
    }
}
