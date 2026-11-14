//! `wasdk:media/caption-settings`: how the user set captions to look, where the system keeps
//! caption settings.
//!
//! One [`CaptionSettings`] serves every program, written by the shell, and every clone is the same
//! one; the engine has one only where the shell's system keeps caption settings, as the shell says
//! when the engine starts. The interface is served to a program by [`host`].

pub(crate) mod host;

use std::sync::Arc;

use futures::stream::BoxStream;
use tokio::sync::watch;

use crate::bindings::wasdk::media::caption_settings::CaptionStyle;
use crate::newest::changes_of;

/// The user's caption style, as every program reads it: held by the engine for the shell to
/// report changes into.
#[derive(Clone)]
pub struct CaptionSettings(Arc<watch::Sender<CaptionStyle>>);

impl CaptionSettings {
    /// What the user has set at the start.
    pub(crate) fn new(style: CaptionStyle) -> CaptionSettings {
        CaptionSettings(Arc::new(watch::Sender::new(style)))
    }

    /// The style the user set, as the shell reported it last.
    pub(crate) fn style(&self) -> CaptionStyle {
        self.0.borrow().clone()
    }

    /// Each style the shell reports from now on, on a stream of its own: one read late hears the
    /// newest.
    pub(crate) fn style_changes(&self) -> BoxStream<'static, CaptionStyle> {
        changes_of(self.0.subscribe())
    }

    /// The user's style is `style` from now on: what `style` answers, and what every stream of its
    /// changes hears next.
    pub(crate) fn style_changed(&self, style: CaptionStyle) {
        self.0.send_replace(style);
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use futures::executor::block_on;

    use super::*;
    use crate::bindings::wasdk::media::caption_settings::{CaptionBehavior, NumberSetting};

    fn scaled(scale: f64) -> CaptionStyle {
        CaptionStyle {
            foreground_color: None,
            foreground_opacity: None,
            background_color: None,
            background_opacity: None,
            window_color: None,
            window_opacity: None,
            window_corner_radius: None,
            edge: None,
            edge_color: None,
            typeface: None,
            text_scale: Some(NumberSetting {
                value: scale,
                behavior: CaptionBehavior::Always,
            }),
        }
    }

    fn scale_of(style: &CaptionStyle) -> Option<f64> {
        style.text_scale.as_ref().map(|setting| setting.value)
    }

    #[test]
    fn a_style_the_shell_reports_is_read_and_every_stream_hears_the_newest() {
        let settings = CaptionSettings::new(scaled(1.0));
        let mut changes = settings.style_changes();
        settings.style_changed(scaled(1.5));
        settings.style_changed(scaled(2.0));
        assert_eq!(scale_of(&settings.style()), Some(2.0));
        let heard = block_on(changes.next()).unwrap();
        assert_eq!(
            scale_of(&heard),
            Some(2.0),
            "a stream read late hears the newest"
        );
    }
}
