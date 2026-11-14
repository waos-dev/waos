//! The window's layout, in egui: the panel beside the stage, or under it in a narrow window.

pub mod panel;
pub mod stage;
pub mod theme;

use egui::{CentralPanel, Frame, Margin, Panel, ScrollArea};

use self::panel::{Action, Contents};
use crate::camera::Camera;

/// A window narrower than this, in points, has the panel under the stage.
const NARROW: f32 = 620.0;
const PANEL_WIDTH: f32 = 272.0;

/// What a pass of the window's layout answers.
pub struct Layout {
    /// Where the scene is drawn, in points.
    pub stage: egui::Rect,
    pub action: Option<Action>,
}

/// Lays out the panel and the stage in `ui`, the whole window.
pub fn show(ui: &mut egui::Ui, contents: Contents<'_>, camera: &mut Camera) -> Layout {
    let window = ui.available_size();
    let frame = Frame::side_top_panel(ui.style()).inner_margin(Margin::same(14));
    let panel = if window.x < NARROW {
        Panel::bottom("panel").exact_size((window.y * 0.46).min(330.0))
    } else {
        Panel::right("panel").exact_size(PANEL_WIDTH)
    };
    let scrolled = |ui: &mut egui::Ui| {
        let scroll = ScrollArea::vertical().auto_shrink(false);
        scroll.show(ui, |ui| panel::show(ui, contents)).inner
    };
    let action = panel.resizable(false).frame(frame).show(ui, scrolled).inner;
    let stage = CentralPanel::no_frame()
        .show(ui, |ui| stage::show(ui, camera))
        .inner;
    Layout { stage, action }
}
