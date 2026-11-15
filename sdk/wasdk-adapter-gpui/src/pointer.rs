//! A view's pointer events, in Flutter's `PointerData` vocabulary, as GPUI's mouse, scroll and
//! pinch input.
//!
//! wasdk's events carry the buttons held, not the one that changed, no modifiers and no click
//! count, in physical pixels. So this keeps the buttons seen last, to name the one pressed or
//! released, counts clicks itself, and takes the modifiers from the keys (`keys`). A trackpad's pan arrives as the
//! gesture's total so far, so this keeps the last total to give GPUI each step.

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent,
    NavigationDirection, PinchEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollEvent,
    ScrollSource, TouchPhase, point, px,
};
use wasdk::views::{PointerButtons, PointerChange, PointerEvent, PointerSignalKind};

/// Two presses this close in time and place are one click more: `gpui_web`'s numbers, since the
/// platform does not say the system's double-click interval.
const DOUBLE_CLICK_NANOSECONDS: u64 = 400_000_000;
const DOUBLE_CLICK_DISTANCE: f32 = 5.0;

pub(crate) struct PointerState {
    buttons: PointerButtons,
    position: Point<Pixels>,
    last_click: Option<(u64, Point<Pixels>)>,
    click_count: usize,
    pan: Point<f64>,
    scale: f64,
}

impl Default for PointerState {
    fn default() -> Self {
        PointerState {
            buttons: PointerButtons::empty(),
            position: Point::default(),
            last_click: None,
            click_count: 0,
            pan: point(0.0, 0.0),
            scale: 1.0,
        }
    }
}

/// What a pointer event means for the window beside its input: whether the pointer entered or left.
pub(crate) enum Hover {
    Entered,
    Left,
    Unchanged,
}

impl PointerState {
    pub(crate) fn position(&self) -> Point<Pixels> {
        self.position
    }

    /// The input `event` is to GPUI, at `scale` physical pixels to a logical one, with the
    /// `modifiers` held.
    pub(crate) fn input(
        &mut self,
        event: &PointerEvent,
        scale: f32,
        modifiers: Modifiers,
    ) -> (Option<PlatformInput>, Hover) {
        let position = point(px(event.x as f32 / scale), px(event.y as f32 / scale));
        self.position = position;
        if event.signal == PointerSignalKind::Scroll {
            let delta = ScrollDelta::Pixels(point(
                px(-event.scroll_delta_x as f32 / scale),
                px(-event.scroll_delta_y as f32 / scale),
            ));
            return (
                Some(scroll(position, delta, modifiers, TouchPhase::Moved)),
                Hover::Unchanged,
            );
        }
        let previous = std::mem::replace(&mut self.buttons, event.buttons);
        let input = match event.change {
            PointerChange::Down => {
                let button = changed_button(event.buttons - previous);
                let click_count = self.count_click(event.time, position);
                Some(PlatformInput::MouseDown(MouseDownEvent {
                    button,
                    position,
                    modifiers,
                    click_count,
                    first_mouse: false,
                }))
            }
            PointerChange::Up => Some(PlatformInput::MouseUp(MouseUpEvent {
                button: changed_button(previous - event.buttons),
                position,
                modifiers,
                click_count: self.click_count,
            })),
            PointerChange::Move | PointerChange::Hover => {
                Some(PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: pressed_button(event.buttons),
                    modifiers,
                }))
            }
            PointerChange::Remove => {
                return (
                    Some(PlatformInput::MouseExited(MouseExitEvent {
                        position,
                        pressed_button: pressed_button(event.buttons),
                        modifiers,
                    })),
                    Hover::Left,
                );
            }
            PointerChange::Add => return (None, Hover::Entered),
            PointerChange::PanZoomStart => {
                self.pan = point(0.0, 0.0);
                self.scale = 1.0;
                Some(scroll(
                    position,
                    ScrollDelta::Pixels(point(px(0.0), px(0.0))),
                    modifiers,
                    TouchPhase::Started,
                ))
            }
            PointerChange::PanZoomUpdate => {
                Some(self.pan_zoom_step(event, position, scale, modifiers))
            }
            PointerChange::PanZoomEnd => Some(scroll(
                position,
                ScrollDelta::Pixels(point(px(0.0), px(0.0))),
                modifiers,
                TouchPhase::Ended,
            )),
            PointerChange::Cancel => None,
        };
        (input, Hover::Unchanged)
    }

    /// A trackpad gesture's step: a pinch when its scale changed, else a scroll by the pan's step.
    fn pan_zoom_step(
        &mut self,
        event: &PointerEvent,
        position: Point<Pixels>,
        scale: f32,
        modifiers: Modifiers,
    ) -> PlatformInput {
        let previous_scale = std::mem::replace(&mut self.scale, event.scale);
        if (event.scale - previous_scale).abs() > f64::EPSILON {
            return PlatformInput::Pinch(PinchEvent {
                position,
                delta: (event.scale / previous_scale.max(f64::EPSILON) - 1.0) as f32,
                modifiers,
                phase: TouchPhase::Moved,
            });
        }
        let previous_pan = std::mem::replace(&mut self.pan, point(event.pan_x, event.pan_y));
        let delta = ScrollDelta::Pixels(point(
            px(((event.pan_x - previous_pan.x) as f32) / scale),
            px(((event.pan_y - previous_pan.y) as f32) / scale),
        ));
        scroll(position, delta, modifiers, TouchPhase::Moved)
    }

    fn count_click(&mut self, time: u64, position: Point<Pixels>) -> usize {
        let repeats = self.last_click.is_some_and(|(last_time, last_position)| {
            let distance = (position - last_position).magnitude() as f32;
            time.saturating_sub(last_time) < DOUBLE_CLICK_NANOSECONDS
                && distance < DOUBLE_CLICK_DISTANCE
        });
        self.click_count = if repeats { self.click_count + 1 } else { 1 };
        self.last_click = Some((time, position));
        self.click_count
    }
}

fn scroll(
    position: Point<Pixels>,
    delta: ScrollDelta,
    modifiers: Modifiers,
    touch_phase: TouchPhase,
) -> PlatformInput {
    PlatformInput::Scroll(ScrollEvent {
        position,
        delta,
        modifiers,
        touch_phase,
        source: ScrollSource::Pointer,
    })
}

fn changed_button(buttons: PointerButtons) -> MouseButton {
    pressed_button(buttons).unwrap_or(MouseButton::Left)
}

fn pressed_button(buttons: PointerButtons) -> Option<MouseButton> {
    [
        (PointerButtons::PRIMARY, MouseButton::Left),
        (PointerButtons::SECONDARY, MouseButton::Right),
        (PointerButtons::TERTIARY, MouseButton::Middle),
        (
            PointerButtons::BACK,
            MouseButton::Navigate(NavigationDirection::Back),
        ),
        (
            PointerButtons::FORWARD,
            MouseButton::Navigate(NavigationDirection::Forward),
        ),
    ]
    .into_iter()
    .find(|(flag, _)| buttons.contains(*flag))
    .map(|(_, button)| button)
}
