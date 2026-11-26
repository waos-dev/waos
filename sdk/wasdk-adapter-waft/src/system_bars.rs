//! The system's bars over a view, as the framework reaches them: the platform's `system-bars`,
//! which takes the style the framework asks for as `overlay-style`, Flutter's
//! `SystemUiOverlayStyle` without the bars' colours, which the interface does not carry, and
//! hides and shows them.

use waft_embedder::{Brightness, SystemBars, SystemBarsBehavior, SystemUiOverlayStyle};
use wasdk::system;
use wasdk::views::{self, OverlayStyle};

/// The platform's bars over one view.
pub(crate) struct WasdkSystemBars(pub(crate) views::SystemBars);

impl SystemBars for WasdkSystemBars {
    fn set_overlay_style(&self, style: &SystemUiOverlayStyle) {
        self.0.set_overlay_style(platform_overlay_style(style));
    }

    fn hide(&self, behavior: SystemBarsBehavior) {
        self.0.hide(platform_system_bars_behavior(behavior));
    }

    fn show(&self) {
        self.0.show();
    }
}

fn platform_system_bars_behavior(behavior: SystemBarsBehavior) -> views::SystemBarsBehavior {
    match behavior {
        SystemBarsBehavior::ShowBarsBySwipe => views::SystemBarsBehavior::ShowBarsBySwipe,
        SystemBarsBehavior::ShowTransientBarsBySwipe => {
            views::SystemBarsBehavior::ShowTransientBarsBySwipe
        }
    }
}

/// `style` as the platform takes it. The status bar's icons are Android's brightness of them, or
/// else the opposite of iOS's brightness of the bar, which is what Flutter's iOS embedding reads.
pub(crate) fn platform_overlay_style(style: &SystemUiOverlayStyle) -> OverlayStyle {
    let status_bar_icons = style
        .status_bar_icon_brightness
        .or(style.status_bar_brightness.map(opposite));
    OverlayStyle {
        status_bar_icons: status_bar_icons.map(platform_brightness),
        navigation_bar_icons: style
            .system_navigation_bar_icon_brightness
            .map(platform_brightness),
        status_bar_contrast_enforced: style.system_status_bar_contrast_enforced,
        navigation_bar_contrast_enforced: style.system_navigation_bar_contrast_enforced,
    }
}

fn platform_brightness(brightness: Brightness) -> system::Brightness {
    match brightness {
        Brightness::Light => system::Brightness::Light,
        Brightness::Dark => system::Brightness::Dark,
    }
}

fn opposite(brightness: Brightness) -> Brightness {
    match brightness {
        Brightness::Light => Brightness::Dark,
        Brightness::Dark => Brightness::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_bar_icons_are_androids_brightness_of_them() {
        let style = platform_overlay_style(&SystemUiOverlayStyle {
            status_bar_icon_brightness: Some(Brightness::Dark),
            status_bar_brightness: Some(Brightness::Dark),
            system_navigation_bar_icon_brightness: Some(Brightness::Light),
            system_status_bar_contrast_enforced: Some(false),
            ..SystemUiOverlayStyle::new()
        });
        assert_eq!(style.status_bar_icons, Some(system::Brightness::Dark));
        assert_eq!(style.navigation_bar_icons, Some(system::Brightness::Light));
        assert_eq!(style.status_bar_contrast_enforced, Some(false));
        assert_eq!(style.navigation_bar_contrast_enforced, None);
    }

    #[test]
    fn a_light_bar_on_ios_has_dark_icons() {
        let style = platform_overlay_style(&SystemUiOverlayStyle {
            status_bar_brightness: Some(Brightness::Light),
            ..SystemUiOverlayStyle::new()
        });
        assert_eq!(style.status_bar_icons, Some(system::Brightness::Dark));
    }

    #[test]
    fn nothing_said_is_nothing_asked() {
        let style = platform_overlay_style(&SystemUiOverlayStyle::new());
        assert_eq!(style.status_bar_icons, None);
        assert_eq!(style.navigation_bar_icons, None);
    }
}
