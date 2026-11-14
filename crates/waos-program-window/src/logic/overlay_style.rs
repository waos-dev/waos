//! The style of the system's bars a program asks for over its view, as Waft's
//! `SystemUiOverlayStyle`, which an annotated region over the view's canvas carries to Waft's host.
//!
//! A program says only what it changes: a field it leaves out keeps what it said before, so each
//! call is laid over the style said until then (`said_over`).

use waft::Brightness;
use waft::ui::SystemUiOverlayStyle;
use waos::bindings::wasdk::platform::system::Brightness as ProgramBrightness;
use waos::bindings::wasdk::platform::views::OverlayStyle;

/// `style` laid over `before`: each field `style` says, and `before`'s where it says nothing.
pub(crate) fn said_over(before: OverlayStyle, style: OverlayStyle) -> OverlayStyle {
    OverlayStyle {
        status_bar_icons: style.status_bar_icons.or(before.status_bar_icons),
        navigation_bar_icons: style.navigation_bar_icons.or(before.navigation_bar_icons),
        status_bar_contrast_enforced: style
            .status_bar_contrast_enforced
            .or(before.status_bar_contrast_enforced),
        navigation_bar_contrast_enforced: style
            .navigation_bar_contrast_enforced
            .or(before.navigation_bar_contrast_enforced),
    }
}

/// `style` in Waft's terms, with iOS's brightness of the status bar, the opposite of its icons',
/// beside Android's brightness of the icons, as Flutter's own styles carry both.
pub(crate) fn waft_overlay_style(style: &OverlayStyle) -> SystemUiOverlayStyle {
    let status_bar_icons = style.status_bar_icons.map(waft_brightness);
    SystemUiOverlayStyle {
        status_bar_icon_brightness: status_bar_icons,
        status_bar_brightness: status_bar_icons.map(opposite),
        system_navigation_bar_icon_brightness: style.navigation_bar_icons.map(waft_brightness),
        system_status_bar_contrast_enforced: style.status_bar_contrast_enforced,
        system_navigation_bar_contrast_enforced: style.navigation_bar_contrast_enforced,
        ..SystemUiOverlayStyle::new()
    }
}

fn waft_brightness(brightness: ProgramBrightness) -> Brightness {
    match brightness {
        ProgramBrightness::Light => Brightness::Light,
        ProgramBrightness::Dark => Brightness::Dark,
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

    const NOTHING: OverlayStyle = OverlayStyle {
        status_bar_icons: None,
        navigation_bar_icons: None,
        status_bar_contrast_enforced: None,
        navigation_bar_contrast_enforced: None,
    };

    #[test]
    fn a_field_left_out_keeps_what_was_said_before() {
        let before = OverlayStyle {
            status_bar_icons: Some(ProgramBrightness::Dark),
            status_bar_contrast_enforced: Some(false),
            ..NOTHING
        };
        let style = OverlayStyle {
            status_bar_icons: Some(ProgramBrightness::Light),
            navigation_bar_icons: Some(ProgramBrightness::Dark),
            ..NOTHING
        };
        let said = said_over(before, style);
        assert_eq!(said.status_bar_icons, Some(ProgramBrightness::Light));
        assert_eq!(said.navigation_bar_icons, Some(ProgramBrightness::Dark));
        assert_eq!(said.status_bar_contrast_enforced, Some(false));
        assert_eq!(said.navigation_bar_contrast_enforced, None);
    }

    #[test]
    fn dark_icons_are_a_light_status_bar_on_ios() {
        let style = waft_overlay_style(&OverlayStyle {
            status_bar_icons: Some(ProgramBrightness::Dark),
            navigation_bar_contrast_enforced: Some(true),
            ..NOTHING
        });
        assert_eq!(style.status_bar_icon_brightness, Some(Brightness::Dark));
        assert_eq!(style.status_bar_brightness, Some(Brightness::Light));
        assert_eq!(style.system_navigation_bar_icon_brightness, None);
        assert_eq!(style.system_navigation_bar_contrast_enforced, Some(true));
        assert_eq!(style.status_bar_color, None);
    }
}
