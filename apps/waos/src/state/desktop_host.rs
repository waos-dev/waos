//! Waft's desktop host, which the app reaches beyond Waft's `Platform` for what only macOS
//! offers: hearing WaOS asked to show itself again, which summons its launcher, and hiding WaOS.
//! On another system, or a host of another kind, there is nothing to reach.

use waft::App;
use waft_embedder_winit::{MacOs, WinitPlatform};

/// What `act` answers with what only macOS offers, when the app runs on Waft's desktop host on
/// macOS.
pub(crate) fn with_macos<T>(app: &App, act: impl FnOnce(&MacOs) -> T) -> Option<T> {
    let platform = app.platform();
    platform.downcast_ref::<WinitPlatform>()?.macos().map(act)
}
