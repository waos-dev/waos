//! The launcher's window as WaOS's own on a desktop (`state::launcher`): one floating card with
//! rounded corners over whatever is on screen, the window around it see-through and casting the
//! system's shadow. In it, the library in Fluent's controls, its home the launcher's own
//! (`home`), a search row over the grid of programs, and its other pages, getting programs and
//! the settings, each with Back to the grid; what the library asks or says, as a dialog over the
//! card. Escape in it and the system's close command dismiss the launcher, and the window does
//! what the launcher orders (`window_orders`).
//!
//! The invariant: the card is all the window shows, so nothing but the card is drawn where the
//! window is see-through.

pub(crate) mod home;
#[cfg(test)]
pub(crate) mod tests;
mod window_orders;

use std::rc::Rc;

use waft::{
    App, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxDecoration, BuildContext,
    ClipRRect, DecoratedBox, Entity, IntoWidget, Listener, StatelessWidget, WidgetRef,
};
use waft_rendering::DecorationPosition;
use waft_winui::ThemeResources;

use crate::state::launcher::{LAUNCHER_VIEW, Launcher};
use crate::state::library::Library;
use crate::state::recordings::Recordings;
use crate::ui::every_window::LibraryWindow;
use crate::ui::library::fluent::FluentLibrary;
use home::LauncherHome;
use window_orders::WindowOrders;

/// How round the card's corners are.
const CORNER_RADIUS: f64 = 12.0;

/// The launcher's window: `library` in its card, with who records among `recordings`.
pub(crate) fn launcher_window(
    launcher: &Entity<Launcher>,
    library: &Entity<Library>,
    recordings: &Entity<Recordings>,
) -> LibraryWindow {
    let dismissing = launcher.clone();
    let dismissed = Listener::new(move |app| dismissing.update(app, Launcher::dismissed));
    let home = LauncherHome {
        launcher: launcher.clone(),
        library: library.clone(),
        recordings: recordings.clone(),
    };
    let library = FluentLibrary {
        library: library.clone(),
        recordings: recordings.clone(),
        dismissed: Some(dismissed),
        questions_here: true,
        home: Some(home.into_widget()),
    };
    let card = WindowOrders {
        launcher: launcher.clone(),
        child: Card {
            child: library.into_widget(),
        }
        .into_widget(),
    };
    let closing = launcher.clone();
    LibraryWindow {
        content: card.into_widget(),
        close_requested: Rc::new(move |app, _view| closing.update(app, Launcher::dismissed)),
        initial_data: Some(LAUNCHER_VIEW),
    }
}

/// `child` clipped to the card's rounded corners, inside the hairline border Fluent draws around a
/// flyout.
#[derive(Debug)]
struct Card {
    child: WidgetRef,
}

impl StatelessWidget for Card {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let radius = BorderRadiusGeometry::circular(CORNER_RADIUS);
        let hairline = Border::all(
            resources.surface_stroke_color_flyout,
            1.0,
            BorderStyle::Solid,
            BorderSide::STROKE_ALIGN_INSIDE,
        );
        let border = BoxDecoration {
            border: Some(hairline.into()),
            border_radius: Some(radius),
            ..BoxDecoration::default()
        };
        let clipped = ClipRRect::new()
            .border_radius(radius)
            .child(self.child.clone());
        DecoratedBox::new(border)
            .position(DecorationPosition::Foreground)
            .child(clipped)
            .into_widget()
    }
}
