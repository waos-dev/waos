//! A view's pointer events as egui's.
//!
//! A view speaks Flutter's `PointerData`: one record for a mouse, a finger and a trackpad's
//! gesture, with the buttons held as bits and a gesture's pan, scale and rotation as totals since
//! it began. egui wants a button named as it goes down or up, a touch as a touch and also as the
//! one pointer it stands for, and a gesture as steps. What that takes is kept here: the buttons
//! last held, the finger that is the pointer, and the gesture's last totals.

use egui::{Event, Modifiers, PointerButton, Pos2, TouchDeviceId, TouchId, TouchPhase, Vec2};
use wasdk::views::{
    PointerButtons, PointerChange, PointerDeviceKind, PointerEvent, PointerSignalKind,
};

const BUTTONS: [(PointerButtons, PointerButton); 5] = [
    (PointerButtons::PRIMARY, PointerButton::Primary),
    (PointerButtons::SECONDARY, PointerButton::Secondary),
    (PointerButtons::TERTIARY, PointerButton::Middle),
    (PointerButtons::BACK, PointerButton::Extra1),
    (PointerButtons::FORWARD, PointerButton::Extra2),
];

/// What the view's pointers were last heard doing.
pub(super) struct Pointers {
    buttons: PointerButtons,
    primary_touch: Option<i64>,
    gesture: Gesture,
}

/// A trackpad gesture's totals, as its last update gave them.
#[derive(Clone, Copy)]
struct Gesture {
    pan: Vec2,
    scale: f32,
    rotation: f32,
}

impl Gesture {
    const START: Gesture = Gesture {
        pan: Vec2::ZERO,
        scale: 1.0,
        rotation: 0.0,
    };
}

impl Pointers {
    pub(super) fn new() -> Pointers {
        Pointers {
            buttons: PointerButtons::empty(),
            primary_touch: None,
            gesture: Gesture::START,
        }
    }

    /// `event` as egui's events, with `pixels_per_point` physical pixels to one of egui's points.
    pub(super) fn translate(
        &mut self,
        event: &PointerEvent,
        pixels_per_point: f32,
        modifiers: Modifiers,
    ) -> Vec<Event> {
        let position = Pos2::new(event.x as f32, event.y as f32) / pixels_per_point;
        match event.kind {
            PointerDeviceKind::Touch => self.touch(event, position, modifiers),
            _ => self.cursor(event, position, pixels_per_point, modifiers),
        }
    }

    /// A finger: a touch, and for the first finger down the pointer too, until it lifts.
    fn touch(&mut self, event: &PointerEvent, position: Pos2, modifiers: Modifiers) -> Vec<Event> {
        let Some(phase) = touch_phase(event.change) else {
            return Vec::new();
        };
        let mut events = vec![Event::Touch {
            device_id: TouchDeviceId(0),
            id: TouchId(event.device as u64),
            phase,
            pos: position,
            force: (event.pressure_max > event.pressure_min).then_some(event.pressure as f32),
        }];
        if phase == TouchPhase::Start && self.primary_touch.is_none() {
            self.primary_touch = Some(event.device);
            events.push(Event::PointerMoved(position));
            events.push(button(position, PointerButton::Primary, true, modifiers));
        } else if self.primary_touch == Some(event.device) {
            events.push(Event::PointerMoved(position));
            if phase != TouchPhase::Move {
                self.primary_touch = None;
                events.push(button(position, PointerButton::Primary, false, modifiers));
                events.push(Event::PointerGone);
            }
        }
        events
    }

    /// A mouse, a stylus or a trackpad's cursor: its moves, its buttons, its wheel and gestures.
    fn cursor(
        &mut self,
        event: &PointerEvent,
        position: Pos2,
        pixels_per_point: f32,
        modifiers: Modifiers,
    ) -> Vec<Event> {
        match event.change {
            PointerChange::Remove => vec![Event::PointerGone],
            PointerChange::Cancel => {
                let mut events = self.button_changes(PointerButtons::empty(), position, modifiers);
                events.push(Event::PointerGone);
                events
            }
            PointerChange::PanZoomStart => {
                self.gesture = Gesture::START;
                Vec::new()
            }
            PointerChange::PanZoomUpdate => self.gesture_step(event, pixels_per_point, modifiers),
            PointerChange::PanZoomEnd => Vec::new(),
            PointerChange::Add
            | PointerChange::Hover
            | PointerChange::Down
            | PointerChange::Move
            | PointerChange::Up => {
                let mut events = vec![Event::PointerMoved(position)];
                events.extend(self.button_changes(event.buttons, position, modifiers));
                events.extend(signal(event, pixels_per_point, modifiers));
                events
            }
        }
    }

    /// A press or a release for each button held now and not before, or before and not now.
    fn button_changes(
        &mut self,
        held: PointerButtons,
        position: Pos2,
        modifiers: Modifiers,
    ) -> Vec<Event> {
        let before = std::mem::replace(&mut self.buttons, held);
        BUTTONS
            .into_iter()
            .filter(|(bit, _)| before.contains(*bit) != held.contains(*bit))
            .map(|(bit, name)| button(position, name, held.contains(bit), modifiers))
            .collect()
    }

    /// How far a trackpad's gesture went since its last update: a scroll for its pan, a zoom for
    /// its scale and a turn for its rotation.
    fn gesture_step(
        &mut self,
        event: &PointerEvent,
        pixels_per_point: f32,
        modifiers: Modifiers,
    ) -> Vec<Event> {
        let now = Gesture {
            pan: Vec2::new(event.pan_x as f32, event.pan_y as f32) / pixels_per_point,
            scale: event.scale as f32,
            rotation: event.rotation as f32,
        };
        let before = std::mem::replace(&mut self.gesture, now);
        let mut events = Vec::new();
        if now.pan != before.pan {
            events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: now.pan - before.pan,
                phase: TouchPhase::Move,
                modifiers,
            });
        }
        if now.scale != before.scale && before.scale > 0.0 {
            events.push(Event::Zoom(now.scale / before.scale));
        }
        if now.rotation != before.rotation {
            events.push(Event::Rotate(now.rotation - before.rotation));
        }
        events
    }
}

fn touch_phase(change: PointerChange) -> Option<TouchPhase> {
    match change {
        PointerChange::Down => Some(TouchPhase::Start),
        PointerChange::Move => Some(TouchPhase::Move),
        PointerChange::Up => Some(TouchPhase::End),
        PointerChange::Cancel => Some(TouchPhase::Cancel),
        _ => None,
    }
}

fn button(position: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton {
        pos: position,
        button,
        pressed,
        modifiers,
    }
}

/// A wheel's scroll or a pinch, which a view reports on the movement it interrupts. A view's
/// scroll says how far the content goes towards its end; egui's wheel, how far it comes back.
fn signal(event: &PointerEvent, pixels_per_point: f32, modifiers: Modifiers) -> Option<Event> {
    match event.signal {
        PointerSignalKind::Scroll => {
            let scroll = Vec2::new(event.scroll_delta_x as f32, event.scroll_delta_y as f32);
            Some(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: -scroll / pixels_per_point,
                phase: TouchPhase::Move,
                modifiers,
            })
        }
        PointerSignalKind::Scale => Some(Event::Zoom(event.scale as f32)),
        PointerSignalKind::None | PointerSignalKind::ScrollInertiaCancel => None,
    }
}
