//! How the app looks in light and in dark: egui's style for the panel, and the colours the
//! scene is staged in.

use egui::{CornerRadius, FontId, TextStyle, Theme};

/// The colours the scene is staged in, as displayed.
pub struct StageColours {
    /// The backdrop at the top and at the bottom.
    pub backdrop: [[f32; 3]; 2],
    /// The ground's lines.
    pub ground_ink: [f32; 3],
}

pub fn stage_colours(theme: Theme) -> StageColours {
    match theme {
        Theme::Dark => StageColours {
            backdrop: [[0.2, 0.205, 0.22], [0.105, 0.108, 0.118]],
            ground_ink: [0.5, 0.52, 0.56],
        },
        Theme::Light => StageColours {
            backdrop: [[0.95, 0.953, 0.96], [0.8, 0.808, 0.825]],
            ground_ink: [0.36, 0.38, 0.42],
        },
    }
}

/// Gives both of egui's themes the panel's type sizes, spacing and corners.
pub fn apply(context: &egui::Context) {
    context.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(18.0));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(11.5));
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        let corners = CornerRadius::same(6);
        let widgets = &mut style.visuals.widgets;
        for widget in [
            &mut widgets.noninteractive,
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            widget.corner_radius = corners;
        }
    });
}
