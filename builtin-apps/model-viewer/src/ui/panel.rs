//! The panel: what model is shown, its animations, and how it is seen.

use egui::{Button, ComboBox, RichText, Slider};

use crate::model::Model;
use crate::render::globals::Shading;
use crate::settings::ViewSettings;
use crate::shown::Playback;

/// What the panel says about opening a model.
pub enum Status {
    Ready,
    /// The name of the file being read.
    Opening(String),
    /// Why the last file was not shown.
    Failed(String),
}

/// What the user asked for in the panel that the panel cannot do itself.
#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Open,
    ShowSample,
    FrameModel,
}

/// What the panel shows and sets.
pub struct Contents<'a> {
    pub model: &'a Model,
    pub playback: &'a mut Playback,
    pub settings: &'a mut ViewSettings,
    pub status: &'a Status,
    /// Whether the platform has a dialog to open a file with.
    pub can_open: bool,
}

/// Lays the panel out in `ui`, and answers what the user asked for.
pub fn show(ui: &mut egui::Ui, contents: Contents<'_>) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        let open = ui.add_enabled(contents.can_open, Button::new("Open…"));
        if open
            .on_disabled_hover_text("This system has no dialog to open a file with.")
            .clicked()
        {
            action = Some(Action::Open);
        }
        if ui.button("Sample").clicked() {
            action = Some(Action::ShowSample);
        }
    });
    ui.add_space(6.0);
    model_summary(ui, contents.model, contents.status);
    if !contents.model.animations.is_empty() {
        section(ui, "Animation");
        animation_controls(ui, contents.model, contents.playback);
    }
    section(ui, "View");
    view_controls(ui, contents.settings);
    ui.add_space(10.0);
    if ui.button("Frame model").clicked() {
        action = Some(Action::FrameModel);
    }
    ui.add_space(6.0);
    hint(
        ui,
        "Drag to orbit. Shift-drag or right-drag to pan. Scroll or pinch to zoom.",
    );
    action
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(2.0);
    ui.label(RichText::new(title).small().strong());
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

fn model_summary(ui: &mut egui::Ui, model: &Model, status: &Status) {
    ui.label(RichText::new(&model.name).heading());
    let counts = [
        counted(model.triangle_count(), "triangle"),
        counted(model.materials.len(), "material"),
        counted(model.images.len(), "texture"),
    ];
    hint(ui, &counts.join(" · "));
    match status {
        Status::Ready => {}
        Status::Opening(name) => {
            ui.label(format!("Opening {name}…"));
        }
        Status::Failed(reason) => {
            ui.colored_label(ui.visuals().warn_fg_color, reason);
        }
    }
}

/// `count` of a thing, as it is read: "1 material", "12 480 triangles".
fn counted(count: usize, thing: &str) -> String {
    let digits = count.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push('\u{2009}');
        }
        grouped.push(digit);
    }
    let plural = if count == 1 { "" } else { "s" };
    format!("{grouped} {thing}{plural}")
}

fn animation_controls(ui: &mut egui::Ui, model: &Model, playback: &mut Playback) {
    const AT_REST: &str = "At rest";
    let name_of =
        |animation: Option<usize>| match animation.and_then(|index| model.animations.get(index)) {
            Some(animation) => animation.name.as_str(),
            None => AT_REST,
        };
    let before = playback.animation;
    ComboBox::from_id_salt("animation")
        .width(ui.available_width())
        .selected_text(name_of(playback.animation))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut playback.animation, None, AT_REST);
            for index in 0..model.animations.len() {
                ui.selectable_value(&mut playback.animation, Some(index), name_of(Some(index)));
            }
        });
    if playback.animation != before {
        playback.time = 0.0;
    }
    let Some(animation) = playback
        .animation
        .and_then(|index| model.animations.get(index))
    else {
        return;
    };
    ui.horizontal(|ui| {
        let label = if playback.playing { "Pause" } else { "Play" };
        if ui
            .add_sized([58.0, ui.spacing().interact_size.y], Button::new(label))
            .clicked()
        {
            playback.playing = !playback.playing;
        }
        let duration = animation.duration();
        ui.spacing_mut().slider_width = ui.available_width() - 52.0;
        ui.add(Slider::new(&mut playback.time, 0.0..=duration).show_value(false));
        hint(ui, &format!("{:.1} s", playback.time));
    });
}

fn view_controls(ui: &mut egui::Ui, settings: &mut ViewSettings) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut settings.shading, Shading::Shaded, "Shaded");
        ui.selectable_value(&mut settings.shading, Shading::Wireframe, "Wireframe");
        ui.selectable_value(&mut settings.shading, Shading::Normals, "Normals");
    });
    ui.add_space(2.0);
    ui.checkbox(&mut settings.ground, "Ground");
    ui.checkbox(&mut settings.light, "Light");
    egui::Grid::new("view sliders")
        .num_columns(2)
        .show(ui, |ui| {
            ui.spacing_mut().slider_width = 120.0;
            ui.add_enabled(settings.light, egui::Label::new("Direction"));
            let direction = Slider::new(&mut settings.light_turn, 0.0..=360.0)
                .suffix("°")
                .fixed_decimals(0);
            ui.add_enabled(settings.light, direction);
            ui.end_row();
            ui.label("Exposure");
            ui.add(Slider::new(&mut settings.exposure, -3.0..=3.0).fixed_decimals(1));
            ui.end_row();
        });
}
