//! The icons of the system's bars over the program's windows, light or dark, as `gpui_ios`'s
//! `set_status_bar_style` has an app say them: GPUI has no word for them, and only the app knows
//! whether what it draws under the bars is light or dark. A phone's status bar and Android's
//! navigation bar take the style; a desktop, which draws no bars over a window, ignores it.
//!
//! The invariant: every window shows the style said last, whether its view arrived before it
//! was said or after.

use std::cell::Cell;

use wasdk::system::Brightness;
use wasdk::views::OverlayStyle;

use crate::platform::WasdkPlatform;

/// The icons of the system's bars: `gpui_ios`'s `StatusBarContentStyle`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusBarContentStyle {
    /// Light icons, over a dark background.
    Light,
    /// Dark icons, over a light background.
    #[default]
    Dark,
}

thread_local! {
    /// The style the app said last; none until it says one, when each view keeps the platform's.
    static SAID: Cell<Option<StatusBarContentStyle>> = const { Cell::new(None) };
}

/// Shows the system's bars' icons in `style` over every window of the program, now and later.
pub fn set_status_bar_style(style: StatusBarContentStyle) {
    SAID.set(Some(style));
    if let Some(platform) = WasdkPlatform::current() {
        for window in platform.shown_windows() {
            window.set_overlay_style(overlay_style(style));
        }
    }
}

/// The style a window shows as its view arrives: the one said last, if any.
pub(crate) fn said() -> Option<OverlayStyle> {
    SAID.get().map(overlay_style)
}

/// `style` as WaOS's overlay style: the same icons on the status and the navigation bar, the
/// scrims left as the platform has them.
fn overlay_style(style: StatusBarContentStyle) -> OverlayStyle {
    let icons = match style {
        StatusBarContentStyle::Light => Brightness::Light,
        StatusBarContentStyle::Dark => Brightness::Dark,
    };
    OverlayStyle {
        status_bar_icons: Some(icons),
        navigation_bar_icons: Some(icons),
        status_bar_contrast_enforced: None,
        navigation_bar_contrast_enforced: None,
    }
}
