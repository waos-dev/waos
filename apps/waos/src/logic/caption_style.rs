//! How the user set captions to look, as Waft's caption settings report it, in the engine's
//! terms: setting for setting, each with when it applies.

use waft::Platform;
use waft::ui;
use waos::bindings::wasdk::media::caption_settings::{
    CaptionBehavior, CaptionStyle, CaptionTypeface, ColorSetting, EdgeSetting, GenericCaptionFont,
    NumberSetting, TypefaceSetting,
};

use super::cues::program_edge;

/// The user's caption style as `platform`'s caption settings answer it now; none where the system
/// keeps no caption settings.
pub(crate) fn caption_style(platform: &dyn Platform) -> Option<CaptionStyle> {
    Some(program_style(platform.caption_settings()?.style()))
}

fn program_style(style: ui::CaptionStyle) -> CaptionStyle {
    CaptionStyle {
        foreground_color: style.foreground_color.map(color_setting),
        foreground_opacity: style.foreground_opacity.map(number_setting),
        background_color: style.background_color.map(color_setting),
        background_opacity: style.background_opacity.map(number_setting),
        window_color: style.window_color.map(color_setting),
        window_opacity: style.window_opacity.map(number_setting),
        window_corner_radius: style.window_corner_radius.map(number_setting),
        edge: style.edge.map(|setting| EdgeSetting {
            value: program_edge(setting.value),
            behavior: program_behavior(setting.behavior),
        }),
        edge_color: style.edge_color.map(color_setting),
        typeface: style.typeface.map(|setting| TypefaceSetting {
            value: program_typeface(setting.value),
            behavior: program_behavior(setting.behavior),
        }),
        text_scale: style.text_scale.map(number_setting),
    }
}

fn color_setting(setting: ui::CaptionSetting<ui::Color>) -> ColorSetting {
    ColorSetting {
        value: setting.value.to_argb32(),
        behavior: program_behavior(setting.behavior),
    }
}

fn number_setting(setting: ui::CaptionSetting<f64>) -> NumberSetting {
    NumberSetting {
        value: setting.value,
        behavior: program_behavior(setting.behavior),
    }
}

fn program_behavior(behavior: ui::CaptionBehavior) -> CaptionBehavior {
    match behavior {
        ui::CaptionBehavior::Always => CaptionBehavior::Always,
        ui::CaptionBehavior::WhereMediaSaysNothing => CaptionBehavior::WhereMediaSaysNothing,
    }
}

fn program_typeface(typeface: ui::CaptionTypeface) -> CaptionTypeface {
    match typeface {
        ui::CaptionTypeface::Family(family) => CaptionTypeface::Family(family),
        ui::CaptionTypeface::Generic(font) => CaptionTypeface::Generic(match font {
            ui::GenericCaptionFont::MonospacedWithSerifs => {
                GenericCaptionFont::MonospacedWithSerifs
            }
            ui::GenericCaptionFont::ProportionalWithSerifs => {
                GenericCaptionFont::ProportionalWithSerifs
            }
            ui::GenericCaptionFont::MonospacedWithoutSerifs => {
                GenericCaptionFont::MonospacedWithoutSerifs
            }
            ui::GenericCaptionFont::ProportionalWithoutSerifs => {
                GenericCaptionFont::ProportionalWithoutSerifs
            }
            ui::GenericCaptionFont::Casual => GenericCaptionFont::Casual,
            ui::GenericCaptionFont::Cursive => GenericCaptionFont::Cursive,
            ui::GenericCaptionFont::SmallCapitals => GenericCaptionFont::SmallCapitals,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_users_style_is_the_programs_setting_for_setting() {
        let style = program_style(ui::CaptionStyle {
            foreground_color: Some(ui::CaptionSetting::always(ui::Color::new(0xFFFF_FF00))),
            text_scale: Some(ui::CaptionSetting::where_media_says_nothing(1.5)),
            typeface: Some(ui::CaptionSetting::always(ui::CaptionTypeface::Generic(
                ui::GenericCaptionFont::Casual,
            ))),
            ..ui::CaptionStyle::default()
        });
        let color = style.foreground_color.unwrap();
        assert_eq!(color.value, 0xFFFF_FF00);
        assert!(matches!(color.behavior, CaptionBehavior::Always));
        let scale = style.text_scale.unwrap();
        assert_eq!(scale.value, 1.5);
        assert!(matches!(
            scale.behavior,
            CaptionBehavior::WhereMediaSaysNothing
        ));
        assert!(matches!(
            style.typeface.unwrap().value,
            CaptionTypeface::Generic(GenericCaptionFont::Casual)
        ));
        assert!(style.edge.is_none(), "the system gave no edge");
    }
}
