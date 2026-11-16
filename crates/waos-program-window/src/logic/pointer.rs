//! Waft's pointer events, as a widget receives them, as the program's: both are Flutter's
//! shapes, field for field, with positions and lengths in the view's physical pixels on the
//! program's side and logical ones on Waft's.
//!
//! A widget hears a pointer enter and leave its region, but no event says that a pointer is new;
//! a program, like any client of Flutter's engine, expects a pointer to be added before its first
//! event and removed when it goes. So the devices the program has heard of are kept here, and a
//! pointer it has not is added before its event.

use std::collections::HashSet;

use waft::ui::PointerDeviceKind as HostKind;
use waft_gestures::PointerEvent as HostEvent;
use waos::bindings::wasdk::platform::views::{
    PointerButtons, PointerChange, PointerDeviceKind, PointerEvent, PointerSignalKind,
};

use super::event_time::event_time;

/// The pointers the program has been told about.
#[derive(Default)]
pub(crate) struct KnownPointers(HashSet<i64>);

impl KnownPointers {
    /// `event` as the program's, `ratio` physical pixels to a logical one, at the time the system
    /// stamped it: a pointer the program has not heard of is added first, and one it has is added
    /// or removed only once.
    pub(crate) fn translate(&mut self, event: &HostEvent, ratio: f64) -> Vec<PointerEvent> {
        let change = change_of(event);
        let device = event.device();
        let known = self.0.contains(&device);
        match change {
            PointerChange::Add if known => return Vec::new(),
            PointerChange::Remove if !known => return Vec::new(),
            PointerChange::Remove => {
                self.0.remove(&device);
            }
            _ => {
                self.0.insert(device);
            }
        }
        let mut events = Vec::new();
        if !known && change != PointerChange::Add {
            events.push(translated(event, PointerChange::Add, ratio));
        }
        events.push(translated(event, change, ratio));
        events
    }
}

/// What the pointer did, as the program names it. A signal, such as a scroll, happens where the
/// pointer is without moving it, so it is reported as the movement it interrupts.
fn change_of(event: &HostEvent) -> PointerChange {
    match event {
        HostEvent::Added(_) | HostEvent::Enter(_) => PointerChange::Add,
        HostEvent::Removed(_) | HostEvent::Exit(_) => PointerChange::Remove,
        HostEvent::Down(_) => PointerChange::Down,
        HostEvent::Move(_) => PointerChange::Move,
        HostEvent::Up(_) => PointerChange::Up,
        HostEvent::Cancel(_) => PointerChange::Cancel,
        HostEvent::PanZoomStart(_) => PointerChange::PanZoomStart,
        HostEvent::PanZoomUpdate(_) => PointerChange::PanZoomUpdate,
        HostEvent::PanZoomEnd(_) => PointerChange::PanZoomEnd,
        HostEvent::Hover(_)
        | HostEvent::Scroll(_)
        | HostEvent::ScrollInertiaCancel(_)
        | HostEvent::Scale(_) => {
            if event.buttons() == 0 {
                PointerChange::Hover
            } else {
                PointerChange::Move
            }
        }
    }
}

fn signal_of(event: &HostEvent) -> PointerSignalKind {
    match event {
        HostEvent::Scroll(_) => PointerSignalKind::Scroll,
        HostEvent::ScrollInertiaCancel(_) => PointerSignalKind::ScrollInertiaCancel,
        HostEvent::Scale(_) => PointerSignalKind::Scale,
        _ => PointerSignalKind::None,
    }
}

/// The kind of device, as the program names it. Waft has one more name than the interface: a
/// pointer of no known kind is a touch, as Flutter's engine takes one.
fn kind_of(kind: HostKind) -> PointerDeviceKind {
    match kind {
        HostKind::Mouse => PointerDeviceKind::Mouse,
        HostKind::Stylus => PointerDeviceKind::Stylus,
        HostKind::InvertedStylus => PointerDeviceKind::InvertedStylus,
        HostKind::Trackpad => PointerDeviceKind::Trackpad,
        HostKind::Touch | HostKind::Unknown => PointerDeviceKind::Touch,
    }
}

/// The buttons held, in the order both sides count them: the primary button is the low bit.
fn buttons_of(bits: i64) -> PointerButtons {
    let order = [
        PointerButtons::PRIMARY,
        PointerButtons::SECONDARY,
        PointerButtons::TERTIARY,
        PointerButtons::BACK,
        PointerButtons::FORWARD,
    ];
    let mut buttons = PointerButtons::empty();
    for (index, button) in order.into_iter().enumerate() {
        if bits & (1 << index) != 0 {
            buttons |= button;
        }
    }
    buttons
}

/// What a contact measures, which every kind of event carries: Flutter's `PointerEvent` fields
/// that the enum has no accessor for.
struct Contact {
    distance: f64,
    distance_max: f64,
    size: f64,
    /// In logical pixels.
    radius_major: f64,
    radius_minor: f64,
    radius_min: f64,
    radius_max: f64,
    orientation: f64,
    tilt: f64,
}

/// `$event`'s contact, whichever kind of event it is.
macro_rules! contact_of {
    ($event:expr, $($variant:ident),+) => {
        match $event {
            $(HostEvent::$variant(e) => Contact {
                distance: e.distance,
                distance_max: e.distance_max,
                size: e.size,
                radius_major: e.radius_major,
                radius_minor: e.radius_minor,
                radius_min: e.radius_min,
                radius_max: e.radius_max,
                orientation: e.orientation,
                tilt: e.tilt,
            },)+
        }
    };
}

fn contact(event: &HostEvent) -> Contact {
    contact_of!(
        event,
        Added,
        Removed,
        Hover,
        Enter,
        Exit,
        Down,
        Move,
        Up,
        Cancel,
        Scroll,
        ScrollInertiaCancel,
        Scale,
        PanZoomStart,
        PanZoomUpdate,
        PanZoomEnd
    )
}

/// How far a scroll asks the content to move, in logical pixels.
fn scroll_of(event: &HostEvent) -> [f64; 2] {
    match event {
        HostEvent::Scroll(scroll) => [scroll.scroll_delta.dx(), scroll.scroll_delta.dy()],
        _ => [0.0, 0.0],
    }
}

/// A pan and zoom so far: the pan in logical pixels, the scale with one as its start, and the
/// rotation in radians.
fn gesture_of(event: &HostEvent) -> ([f64; 2], f64, f64) {
    match event {
        HostEvent::PanZoomUpdate(gesture) => {
            let pan = gesture.local_pan();
            ([pan.dx(), pan.dy()], gesture.scale, gesture.rotation)
        }
        HostEvent::Scale(scale) => ([0.0, 0.0], scale.scale, 0.0),
        _ => ([0.0, 0.0], 1.0, 0.0),
    }
}

/// One event, as the program reads it.
fn translated(event: &HostEvent, change: PointerChange, ratio: f64) -> PointerEvent {
    let position = event.local_position();
    let contact = contact(event);
    let scroll = scroll_of(event);
    let (pan, scale, rotation) = gesture_of(event);
    PointerEvent {
        time: event_time(event.time_stamp()),
        change,
        kind: kind_of(event.kind()),
        signal: signal_of(event),
        device: event.device(),
        x: position.dx() * ratio,
        y: position.dy() * ratio,
        buttons: buttons_of(event.buttons()),
        pressure: event.pressure(),
        pressure_min: event.pressure_min(),
        pressure_max: event.pressure_max(),
        distance: contact.distance,
        distance_max: contact.distance_max,
        size: contact.size,
        radius_major: contact.radius_major * ratio,
        radius_minor: contact.radius_minor * ratio,
        radius_min: contact.radius_min * ratio,
        radius_max: contact.radius_max * ratio,
        orientation: contact.orientation,
        tilt: contact.tilt,
        scroll_delta_x: scroll[0] * ratio,
        scroll_delta_y: scroll[1] * ratio,
        pan_x: pan[0] * ratio,
        pan_y: pan[1] * ratio,
        scale,
        rotation,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use waft::Offset;
    use waft_gestures::{
        PointerDownEvent, PointerEnterEvent, PointerExitEvent, PointerHoverEvent, PointerMoveEvent,
        PointerPanZoomUpdateEvent, PointerScrollEvent, PointerUpEvent, VelocityTracker,
    };

    use super::*;

    /// How far apart a phone's touch reports come: 120 a second.
    const TOUCH_SPACING: Duration = Duration::from_micros(8_333);

    /// A finger's move to `y`, stamped `at` by the system.
    fn finger_move(y: f64, at: Duration) -> HostEvent {
        HostEvent::Move(PointerMoveEvent {
            device: 1,
            kind: HostKind::Touch,
            buttons: 1,
            position: Offset::new(100.0, y),
            time_stamp: at,
            ..PointerMoveEvent::default()
        })
    }

    #[test]
    fn moves_delivered_together_keep_the_spacing_they_happened_at() {
        let mut known = KnownPointers::default();
        let stamps = [0, 1, 2].map(|nth| Duration::from_millis(500) + TOUCH_SPACING * nth);
        let times: Vec<u64> = stamps
            .iter()
            .flat_map(|at| known.translate(&finger_move(300.0, *at), 1.0))
            .filter(|event| event.change == PointerChange::Move)
            .map(|event| event.time)
            .collect();
        let expected: Vec<u64> = stamps.iter().map(|at| at.as_nanos() as u64).collect();
        assert_eq!(times, expected, "each move at its own stamp");
    }

    /// A finger that goes up 200 points, turns and comes down 48, every move reported in one batch,
    /// as the program reads it: what the program's velocity tracker, Flutter's, makes of the
    /// fling as the finger lifts.
    #[test]
    fn a_turned_back_drag_delivered_at_once_flings_the_way_the_finger_went_last() {
        let mut known = KnownPointers::default();
        let mut tracker = VelocityTracker::with_kind(HostKind::Touch);
        let up = (0..20).map(|nth| 600.0 - 10.0 * f64::from(nth));
        let down = (1..=8).map(|nth| 410.0 + 6.0 * f64::from(nth));
        for (nth, y) in up.chain(down).enumerate() {
            let at = Duration::from_secs(3) + TOUCH_SPACING * nth as u32;
            for event in known.translate(&finger_move(y, at), 1.0) {
                let position = Offset::new(event.x, event.y);
                tracker.add_position(Duration::from_nanos(event.time), position);
            }
        }
        let fling = tracker.get_velocity().pixels_per_second;
        assert!(
            fling.dy() > 500.0,
            "the fling goes down, as the finger went last: {fling:?}"
        );
    }

    fn hover(device: i64, x: f64, y: f64) -> HostEvent {
        HostEvent::Hover(PointerHoverEvent {
            device,
            kind: HostKind::Mouse,
            position: Offset::new(x, y),
            ..PointerHoverEvent::default()
        })
    }

    #[test]
    fn a_new_pointer_is_added_before_its_first_event() {
        let mut known = KnownPointers::default();
        let events = known.translate(&hover(1, 10.0, 20.0), 2.0);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].change, PointerChange::Add);
        assert_eq!(events[1].change, PointerChange::Hover);
        assert_eq!((events[1].x, events[1].y), (20.0, 40.0), "physical pixels");
        assert_eq!(events[1].kind, PointerDeviceKind::Mouse);
    }

    #[test]
    fn a_known_pointer_is_not_added_again() {
        let mut known = KnownPointers::default();
        known.translate(&hover(1, 0.0, 0.0), 1.0);
        let events = known.translate(&hover(1, 5.0, 5.0), 1.0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change, PointerChange::Hover);
        let enter = HostEvent::Enter(PointerEnterEvent {
            device: 1,
            ..PointerEnterEvent::default()
        });
        assert!(known.translate(&enter, 1.0).is_empty());
    }

    #[test]
    fn a_pointer_is_removed_once_and_only_after_it_was_added() {
        let mut known = KnownPointers::default();
        let exit = HostEvent::Exit(PointerExitEvent {
            device: 4,
            ..PointerExitEvent::default()
        });
        assert!(known.translate(&exit, 1.0).is_empty());
        known.translate(&hover(4, 0.0, 0.0), 1.0);
        let events = known.translate(&exit, 1.0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change, PointerChange::Remove);
        assert!(known.translate(&exit, 1.0).is_empty());
    }

    #[test]
    fn buttons_are_counted_from_the_primary_one() {
        assert_eq!(buttons_of(0), PointerButtons::empty());
        assert_eq!(buttons_of(1), PointerButtons::PRIMARY);
        assert_eq!(
            buttons_of(0b10110),
            PointerButtons::SECONDARY | PointerButtons::TERTIARY | PointerButtons::FORWARD
        );
    }

    #[test]
    fn a_press_carries_its_buttons_and_its_contact_in_physical_pixels() {
        let mut known = KnownPointers::default();
        let down = HostEvent::Down(PointerDownEvent {
            device: 2,
            buttons: 1,
            pressure: 0.5,
            radius_major: 3.0,
            tilt: 0.25,
            ..PointerDownEvent::default()
        });
        let events = known.translate(&down, 2.0);
        let event = events.last().unwrap();
        assert_eq!(event.change, PointerChange::Down);
        assert_eq!(event.buttons, PointerButtons::PRIMARY);
        assert_eq!(event.pressure, 0.5);
        assert_eq!(event.radius_major, 6.0);
        assert_eq!(event.tilt, 0.25);
        let up = HostEvent::Up(PointerUpEvent {
            device: 2,
            ..PointerUpEvent::default()
        });
        let events = known.translate(&up, 2.0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change, PointerChange::Up);
    }

    #[test]
    fn a_scroll_is_a_signal_where_the_pointer_is() {
        let mut known = KnownPointers::default();
        let scroll = HostEvent::Scroll(PointerScrollEvent {
            device: 1,
            scroll_delta: Offset::new(3.0, -4.0),
            ..PointerScrollEvent::default()
        });
        let events = known.translate(&scroll, 2.0);
        let event = events.last().unwrap();
        assert_eq!(event.change, PointerChange::Hover);
        assert_eq!(event.signal, PointerSignalKind::Scroll);
        assert_eq!((event.scroll_delta_x, event.scroll_delta_y), (6.0, -8.0));
    }

    #[test]
    fn a_pan_and_zoom_carries_its_pan_scale_and_rotation() {
        let mut known = KnownPointers::default();
        let update = HostEvent::PanZoomUpdate(PointerPanZoomUpdateEvent {
            device: 3,
            pan: Offset::new(5.0, 1.0),
            scale: 1.5,
            rotation: 0.1,
            ..PointerPanZoomUpdateEvent::default()
        });
        let events = known.translate(&update, 2.0);
        let event = events.last().unwrap();
        assert_eq!(event.change, PointerChange::PanZoomUpdate);
        assert_eq!(event.kind, PointerDeviceKind::Trackpad);
        assert_eq!((event.pan_x, event.pan_y), (10.0, 2.0));
        assert_eq!((event.scale, event.rotation), (1.5, 0.1));
    }

    #[test]
    fn a_pointer_of_unknown_kind_is_a_touch() {
        assert_eq!(kind_of(HostKind::Unknown), PointerDeviceKind::Touch);
        assert_eq!(kind_of(HostKind::Trackpad), PointerDeviceKind::Trackpad);
    }
}
