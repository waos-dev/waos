//! The user's caption settings, where the system keeps them: Waft's `CaptionSettings` over the
//! WIT's `caption-settings`, its style the WIT's setting for setting, and `changed` resolving at
//! the next style the WIT's `style-changes` hears, or as the stream ends.

use waft_embedder::{
    CaptionBehavior, CaptionSetting, CaptionSettings, CaptionStyle, CaptionTypeface, Color,
    GenericCaptionFont, MediaChangeFuture,
};
use wasdk::caption_settings as platform;

use crate::cues::framework_edge;

/// The user's caption settings.
pub(crate) struct WasdkCaptionSettings(platform::CaptionSettings);

impl WasdkCaptionSettings {
    /// The user's caption settings, where the system keeps them.
    pub(crate) fn get() -> Option<WasdkCaptionSettings> {
        platform::get_caption_settings().map(WasdkCaptionSettings)
    }
}

impl CaptionSettings for WasdkCaptionSettings {
    fn style(&self) -> CaptionStyle {
        framework_style(self.0.style())
    }

    fn changed(&self) -> MediaChangeFuture {
        let mut changes = self.0.style_changes();
        Box::pin(async move {
            changes.next().await;
        })
    }
}

/// The WIT's style, in Waft's terms.
fn framework_style(style: platform::CaptionStyle) -> CaptionStyle {
    CaptionStyle {
        foreground_color: style.foreground_color.map(color_setting),
        foreground_opacity: style.foreground_opacity.map(number_setting),
        background_color: style.background_color.map(color_setting),
        background_opacity: style.background_opacity.map(number_setting),
        window_color: style.window_color.map(color_setting),
        window_opacity: style.window_opacity.map(number_setting),
        window_corner_radius: style.window_corner_radius.map(number_setting),
        edge: style.edge.map(|setting| CaptionSetting {
            value: framework_edge(setting.value),
            behavior: framework_behavior(setting.behavior),
        }),
        edge_color: style.edge_color.map(color_setting),
        typeface: style.typeface.map(|setting| CaptionSetting {
            value: framework_typeface(setting.value),
            behavior: framework_behavior(setting.behavior),
        }),
        text_scale: style.text_scale.map(number_setting),
    }
}

fn color_setting(setting: platform::ColorSetting) -> CaptionSetting<Color> {
    CaptionSetting {
        value: Color::new(setting.value),
        behavior: framework_behavior(setting.behavior),
    }
}

fn number_setting(setting: platform::NumberSetting) -> CaptionSetting<f64> {
    CaptionSetting {
        value: setting.value,
        behavior: framework_behavior(setting.behavior),
    }
}

fn framework_behavior(behavior: platform::CaptionBehavior) -> CaptionBehavior {
    match behavior {
        platform::CaptionBehavior::Always => CaptionBehavior::Always,
        platform::CaptionBehavior::WhereMediaSaysNothing => CaptionBehavior::WhereMediaSaysNothing,
    }
}

fn framework_typeface(typeface: platform::CaptionTypeface) -> CaptionTypeface {
    match typeface {
        platform::CaptionTypeface::Family(family) => CaptionTypeface::Family(family),
        platform::CaptionTypeface::Generic(font) => CaptionTypeface::Generic(match font {
            platform::GenericCaptionFont::MonospacedWithSerifs => {
                GenericCaptionFont::MonospacedWithSerifs
            }
            platform::GenericCaptionFont::ProportionalWithSerifs => {
                GenericCaptionFont::ProportionalWithSerifs
            }
            platform::GenericCaptionFont::MonospacedWithoutSerifs => {
                GenericCaptionFont::MonospacedWithoutSerifs
            }
            platform::GenericCaptionFont::ProportionalWithoutSerifs => {
                GenericCaptionFont::ProportionalWithoutSerifs
            }
            platform::GenericCaptionFont::Casual => GenericCaptionFont::Casual,
            platform::GenericCaptionFont::Cursive => GenericCaptionFont::Cursive,
            platform::GenericCaptionFont::SmallCapitals => GenericCaptionFont::SmallCapitals,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wits_style_is_wafts_setting_for_setting() {
        let style = framework_style(platform::CaptionStyle {
            foreground_color: Some(platform::ColorSetting {
                value: 0xFFFF_FF00,
                behavior: platform::CaptionBehavior::Always,
            }),
            foreground_opacity: None,
            background_color: None,
            background_opacity: None,
            window_color: None,
            window_opacity: None,
            window_corner_radius: None,
            edge: Some(platform::EdgeSetting {
                value: wasdk::media_players::CueTextEdge::DropShadow,
                behavior: platform::CaptionBehavior::WhereMediaSaysNothing,
            }),
            edge_color: None,
            typeface: Some(platform::TypefaceSetting {
                value: platform::CaptionTypeface::Family("Menlo".to_owned()),
                behavior: platform::CaptionBehavior::Always,
            }),
            text_scale: None,
        });
        assert_eq!(
            style,
            CaptionStyle {
                foreground_color: Some(CaptionSetting::always(Color::new(0xFFFF_FF00))),
                edge: Some(CaptionSetting::where_media_says_nothing(
                    waft_embedder::CueTextEdge::DropShadow
                )),
                typeface: Some(CaptionSetting::always(CaptionTypeface::Family(
                    "Menlo".to_owned()
                ))),
                ..CaptionStyle::default()
            }
        );
    }
}
