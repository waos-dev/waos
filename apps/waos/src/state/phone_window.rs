//! The deck in a window of a phone's size, as `waos phone` opens it: the way to see on a desktop
//! the layout a phone's screen takes, its programs in their tabs among it. The window is an
//! ordinary one, asked for here and arriving as every view does, carrying no initial data, built
//! as WaOS's own window is (`ui::every_window`); closing it asks WaOS to quit, which asks the
//! programs first, as the phone's own app ends with its screen and as an app's last window does.

use std::rc::Rc;

use waft::ui::{AppExitType, ViewRef};
use waft::{App, WidgetRef, WindowConfig};
use waft_services::ServicesBinding;

use crate::ui::every_window::LibraryWindow;

/// An iPhone's screen held upright, in points.
pub(crate) const PHONE_SCREEN: [f64; 2] = [390.0, 844.0];

/// Asks the host for the window; where the host makes none, says so.
fn ask_for_phone_window(app: &mut App) {
    let config = WindowConfig {
        title: "WaOS".to_owned(),
        size: PHONE_SCREEN,
        ..WindowConfig::default()
    };
    if let Err(error) = app.platform().request_view(config, None) {
        tracing::warn!(%error, "no window of a phone's size");
    }
}

/// `content` as WaOS's own window: the screen the platform gives, as a phone gives its app the
/// screen, or else a window of a phone's size, asked for now; closing it asks WaOS to quit.
pub(crate) fn in_phone_window(app: &mut App, content: WidgetRef) -> LibraryWindow {
    if app.platform().views().is_empty() {
        ask_for_phone_window(app);
    }
    LibraryWindow {
        content,
        close_requested: Rc::new(ask_to_quit),
        initial_data: None,
    }
}

/// Asks WaOS to quit, as Cmd-Q does, which asks the programs first.
fn ask_to_quit(app: &mut App, _view: &ViewRef) {
    let services = ServicesBinding::instance(app);
    let _answer = services.exit_application(app, AppExitType::Cancelable, 0);
}
