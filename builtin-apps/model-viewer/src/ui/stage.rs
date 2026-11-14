//! The stage: the part of the window the model is seen in. What the pointer, the wheel, fingers
//! and the arrow keys do there moves the camera.

use egui::{CursorIcon, Key, Modifiers, PointerButton, Sense, Vec2};

use crate::camera::Camera;

/// The chords the stage answers while no control has the keyboard.
pub const KEYS: [(Modifiers, Key); 7] = [
    (Modifiers::NONE, Key::ArrowLeft),
    (Modifiers::NONE, Key::ArrowRight),
    (Modifiers::NONE, Key::ArrowUp),
    (Modifiers::NONE, Key::ArrowDown),
    (Modifiers::NONE, Key::Plus),
    (Modifiers::NONE, Key::Equals),
    (Modifiers::NONE, Key::Minus),
];

/// Points of drag an arrow key stands for.
const KEY_STEP: f32 = 24.0;
/// How much closer one press of the plus key comes, and how much a point of scroll.
const KEY_ZOOM: f32 = 1.15;
const ZOOM_PER_POINT: f32 = 0.004;

/// Fills what is left of `ui` with the stage, moves `camera` as its user asks, and answers the
/// rectangle the scene is drawn in, in points.
pub fn show(ui: &mut egui::Ui, camera: &mut Camera) -> egui::Rect {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let (fingers, shift) = ui.input(|input| (input.multi_touch(), input.modifiers.shift));
    let pans = response.dragged_by(PointerButton::Secondary)
        || response.dragged_by(PointerButton::Middle)
        || response.dragged_by(PointerButton::Primary) && shift;
    if let Some(fingers) = fingers.filter(|_| response.contains_pointer()) {
        camera.zoom(fingers.zoom_delta);
        camera.pan(drag(fingers.translation_delta), rect.height());
    } else if pans {
        camera.pan(drag(response.drag_delta()), rect.height());
    } else if response.dragged_by(PointerButton::Primary) {
        camera.orbit(drag(response.drag_delta()));
    }
    if response.hovered() {
        let (scroll, pinch) = ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
        camera.zoom(pinch * (scroll * ZOOM_PER_POINT).exp());
    }
    if ui.memory(|memory| memory.focused().is_none()) {
        answer_keys(ui, camera);
    }
    let cursor = if response.dragged() {
        CursorIcon::Grabbing
    } else {
        CursorIcon::Grab
    };
    response.on_hover_cursor(cursor);
    rect
}

/// A drag in egui's points, as the camera takes one.
fn drag(points: Vec2) -> glam::Vec2 {
    glam::Vec2::new(points.x, points.y)
}

fn answer_keys(ui: &egui::Ui, camera: &mut Camera) {
    let pressed = |key| ui.input_mut(|input| input.consume_key(Modifiers::NONE, key));
    let steps = [
        (Key::ArrowLeft, Vec2::new(KEY_STEP, 0.0)),
        (Key::ArrowRight, Vec2::new(-KEY_STEP, 0.0)),
        (Key::ArrowUp, Vec2::new(0.0, KEY_STEP)),
        (Key::ArrowDown, Vec2::new(0.0, -KEY_STEP)),
    ];
    for (key, step) in steps {
        if pressed(key) {
            camera.orbit(drag(step));
        }
    }
    if pressed(Key::Plus) || pressed(Key::Equals) {
        camera.zoom(KEY_ZOOM);
    }
    if pressed(Key::Minus) {
        camera.zoom(1.0 / KEY_ZOOM);
    }
}
