//! Waft's desktop host, which the app reaches beyond Waft's `Platform` for what only macOS
//! offers: hearing WaOS asked to show itself again, which summons its launcher, hiding WaOS, and
//! keeping macOS from throttling work its user is waiting on. On another system, or a host of
//! another kind, there is nothing to reach.

use waft::App;
use waft::ui::PlatformRef;
use waft_embedder_winit::{MacOs, WinitPlatform};

/// What `act` answers with what only macOS offers, when the app runs on Waft's desktop host on
/// macOS.
pub(crate) fn with_macos<T>(app: &App, act: impl FnOnce(&MacOs) -> T) -> Option<T> {
    with_macos_platform(&app.platform(), act)
}

/// [`with_macos`], over an app's platform kept rather than the app it came from.
pub(crate) fn with_macos_platform<T>(
    platform: &PlatformRef,
    act: impl FnOnce(&MacOs) -> T,
) -> Option<T> {
    platform.downcast_ref::<WinitPlatform>()?.macos().map(act)
}
