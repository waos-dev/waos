//! The machine's facts as Waft reports them, in the engine's terms: the screens, light or dark,
//! the languages and the caption style, each whole, for `WaOS::new` and for the setters that keep
//! them current; with the time zone, which Waft does not report, as the system names it.
//!
//! A screen's size is its frame's, in points, at its scale, as Flutter's `Display.size` is, and
//! its refresh rate in millihertz; its id is Waft's, which it keeps while it stays connected; its
//! frame and visible frame are Waft's, in the coordinates of a window's frame on both sides.

use waft::Platform;
use waft::ui::Screen as WaftScreen;
use waos::bindings::wasdk::platform::screens::PixelSize;
use waos::bindings::wasdk::platform::system::{Brightness, Locale};
use waos::{PlatformState, Screen};
use waos_program_window::rect_of;

use crate::logic::caption_style::caption_style;
use crate::os;

/// What `platform` reports of the machine now.
pub(crate) fn platform_state(platform: &dyn Platform) -> PlatformState {
    PlatformState {
        screens: screens(platform),
        platform_brightness: brightness(platform),
        locales: locales(platform),
        caption_style: caption_style(platform),
        time_zone: os::time_zone(),
    }
}

/// The screens `platform` lists now, the main one first; none where it reports none.
pub(crate) fn screens(platform: &dyn Platform) -> Vec<Screen> {
    platform.screens().map_or_else(Vec::new, |screens| {
        screens.screens().iter().map(screen_of).collect()
    })
}

/// The user's light or dark preference, as `platform` reports it now.
pub(crate) fn brightness(platform: &dyn Platform) -> Brightness {
    match platform.platform_brightness() {
        waft::Brightness::Light => Brightness::Light,
        waft::Brightness::Dark => Brightness::Dark,
    }
}

/// The user's languages, most preferred first, as `platform` reports them now.
pub(crate) fn locales(platform: &dyn Platform) -> Vec<Locale> {
    platform
        .locales()
        .iter()
        .map(|locale| Locale {
            language: locale.language_code.clone(),
            script: locale.script_code.clone(),
            region: locale.country_code.clone(),
        })
        .collect()
}

fn screen_of(screen: &WaftScreen) -> Screen {
    Screen {
        id: screen.id.0,
        name: screen.name.clone(),
        size: PixelSize {
            width: physical(screen.frame.width(), screen.scale),
            height: physical(screen.frame.height(), screen.scale),
        },
        scale: screen.scale,
        refresh_rate: screen
            .refresh_rate
            .map_or(0, |hertz| (hertz * 1000.0).round() as u32),
        frame: rect_of(screen.frame),
        visible_frame: rect_of(screen.visible_frame),
    }
}

/// `points` at `scale`, in whole physical pixels.
fn physical(points: f64, scale: f64) -> u32 {
    (points * scale).round() as u32
}

#[cfg(test)]
mod tests {
    use waft::ui::{InertPlatform, ScreenId};

    use super::*;

    #[test]
    fn a_platform_that_reports_nothing_gives_no_screens_light_and_no_languages() {
        let state = platform_state(&InertPlatform);
        assert!(state.screens.is_empty());
        assert_eq!(state.platform_brightness, Brightness::Light);
        assert!(state.locales.is_empty());
    }

    #[test]
    fn a_screen_is_its_frame_at_its_scale_with_its_id_name_and_rate() {
        let frame = waft::Rect::from_ltwh(0.0, 0.0, 1512.0, 982.0);
        let screen = screen_of(&WaftScreen {
            id: ScreenId(3),
            name: "Built-in Retina Display".to_owned(),
            frame,
            visible_frame: frame,
            scale: 2.0,
            refresh_rate: Some(120.0),
        });
        assert_eq!(screen.id, 3);
        assert_eq!(screen.name, "Built-in Retina Display");
        assert_eq!((screen.size.width, screen.size.height), (3024, 1964));
        assert_eq!(screen.scale, 2.0);
        assert_eq!(screen.refresh_rate, 120_000);
    }

    #[test]
    fn a_screen_is_where_waft_places_it_with_what_its_bars_leave_free() {
        let screen = screen_of(&WaftScreen {
            id: ScreenId(5),
            name: "Studio Display".to_owned(),
            frame: waft::Rect::from_ltwh(-2560.0, -180.0, 2560.0, 1440.0),
            visible_frame: waft::Rect::from_ltwh(-2560.0, -155.0, 2560.0, 1345.0),
            scale: 2.0,
            refresh_rate: Some(60.0),
        });
        let (frame, visible) = (screen.frame, screen.visible_frame);
        assert_eq!((frame.origin.x, frame.origin.y), (-2560.0, -180.0));
        assert_eq!((frame.size.width, frame.size.height), (2560.0, 1440.0));
        assert_eq!((visible.origin.x, visible.origin.y), (-2560.0, -155.0));
        assert_eq!((visible.size.width, visible.size.height), (2560.0, 1345.0));
    }

    #[test]
    fn a_screen_whose_rate_the_system_does_not_say_refreshes_at_zero() {
        let frame = waft::Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let screen = screen_of(&WaftScreen {
            id: ScreenId(1),
            name: String::new(),
            frame,
            visible_frame: frame,
            scale: 1.0,
            refresh_rate: None,
        });
        assert_eq!(screen.refresh_rate, 0);
    }
}
