//! The platform's pointer events as the framework's [`PointerReport`]s.
//!
//! The `views` interface speaks Flutter's `PointerData` vocabulary, and a report holds only what
//! the system said, so every field of an event is carried over as it stands. What the framework
//! wants beyond it, the identity of each press and the movement since the device's last event,
//! the client adds, as Flutter's engine does (Waft's `PointerDataPacketConverter`); nothing is
//! kept here.

use std::time::Duration;

use waft_embedder::{Contact, PointerDeviceKind, PointerReport, ReportedChange, ViewId};
use wasdk::views::{self, PointerEvent};

/// `event`, which arrived on the view `view_id`, as the report the client takes.
pub(crate) fn pointer_report(view_id: ViewId, event: &PointerEvent) -> PointerReport {
    PointerReport {
        view_id,
        time_stamp: Duration::from_nanos(event.time),
        device: event.device,
        kind: device_kind(event.kind),
        position: [event.x, event.y],
        change: reported_change(event),
        contact: contact(event),
    }
}

/// What the device did. A signal happens where the pointer is without changing its state, so it
/// is the report's change whatever movement the event names beside it.
fn reported_change(event: &PointerEvent) -> ReportedChange {
    match event.signal {
        views::PointerSignalKind::Scroll => ReportedChange::Scroll {
            delta: [event.scroll_delta_x, event.scroll_delta_y],
        },
        views::PointerSignalKind::ScrollInertiaCancel => ReportedChange::ScrollInertiaCancel,
        views::PointerSignalKind::Scale => ReportedChange::Scale { scale: event.scale },
        views::PointerSignalKind::None => movement(event),
    }
}

/// How the device moved or changed, with the buttons it holds.
fn movement(event: &PointerEvent) -> ReportedChange {
    let buttons = i64::from(event.buttons.bits());
    match event.change {
        views::PointerChange::Cancel => ReportedChange::Cancel { buttons },
        views::PointerChange::Add => ReportedChange::Add,
        views::PointerChange::Remove => ReportedChange::Remove,
        views::PointerChange::Hover => ReportedChange::Hover { buttons },
        views::PointerChange::Down => ReportedChange::Down { buttons },
        views::PointerChange::Move => ReportedChange::Move { buttons },
        views::PointerChange::Up => ReportedChange::Up { buttons },
        views::PointerChange::PanZoomStart => ReportedChange::PanZoomStart,
        views::PointerChange::PanZoomUpdate => ReportedChange::PanZoomUpdate {
            pan: [event.pan_x, event.pan_y],
            scale: event.scale,
            rotation: event.rotation,
        },
        views::PointerChange::PanZoomEnd => ReportedChange::PanZoomEnd,
    }
}

/// How a finger or a pen meets the surface, field for field.
fn contact(event: &PointerEvent) -> Contact {
    Contact {
        pressure: event.pressure,
        pressure_min: event.pressure_min,
        pressure_max: event.pressure_max,
        distance: event.distance,
        distance_max: event.distance_max,
        size: event.size,
        radius_major: event.radius_major,
        radius_minor: event.radius_minor,
        radius_min: event.radius_min,
        radius_max: event.radius_max,
        orientation: event.orientation,
        tilt: event.tilt,
    }
}

fn device_kind(kind: views::PointerDeviceKind) -> PointerDeviceKind {
    match kind {
        views::PointerDeviceKind::Touch => PointerDeviceKind::Touch,
        views::PointerDeviceKind::Mouse => PointerDeviceKind::Mouse,
        views::PointerDeviceKind::Stylus => PointerDeviceKind::Stylus,
        views::PointerDeviceKind::InvertedStylus => PointerDeviceKind::InvertedStylus,
        views::PointerDeviceKind::Trackpad => PointerDeviceKind::Trackpad,
    }
}

#[cfg(test)]
mod tests {
    use wasdk::views::{PointerButtons, PointerSignalKind as Signal};

    use super::*;

    const VIEW: ViewId = ViewId(3);

    fn event(change: views::PointerChange, buttons: PointerButtons) -> PointerEvent {
        PointerEvent {
            time: 5_000_000,
            change,
            kind: views::PointerDeviceKind::Mouse,
            signal: Signal::None,
            device: 7,
            x: 10.0,
            y: 20.0,
            buttons,
            pressure: 1.0,
            pressure_min: 1.0,
            pressure_max: 1.0,
            distance: 0.0,
            distance_max: 0.0,
            size: 0.0,
            radius_major: 0.0,
            radius_minor: 0.0,
            radius_min: 0.0,
            radius_max: 0.0,
            orientation: 0.0,
            tilt: 0.0,
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
            pan_x: 0.0,
            pan_y: 0.0,
            scale: 1.0,
            rotation: 0.0,
        }
    }

    fn buttons(buttons: PointerButtons) -> i64 {
        i64::from(buttons.bits())
    }

    #[test]
    fn a_press_is_reported_as_the_platform_saw_it() {
        let report = pointer_report(
            VIEW,
            &event(views::PointerChange::Down, PointerButtons::PRIMARY),
        );
        assert_eq!(report.view_id, VIEW);
        assert_eq!(report.time_stamp, Duration::from_millis(5));
        assert_eq!(report.device, 7);
        assert_eq!(report.kind, PointerDeviceKind::Mouse);
        assert_eq!(report.position, [10.0, 20.0]);
        assert_eq!(
            report.change,
            ReportedChange::Down {
                buttons: buttons(PointerButtons::PRIMARY)
            }
        );
        assert_eq!(
            report.contact,
            Contact::default(),
            "a mouse detects no contact"
        );
    }

    #[test]
    fn a_pen_keeps_how_it_meets_the_surface() {
        let pen = PointerEvent {
            kind: views::PointerDeviceKind::Stylus,
            pressure: 0.4,
            pressure_min: 0.0,
            orientation: 0.5,
            tilt: 0.25,
            ..event(views::PointerChange::Move, PointerButtons::PRIMARY)
        };
        let report = pointer_report(VIEW, &pen);
        assert_eq!(report.kind, PointerDeviceKind::Stylus);
        assert_eq!(report.contact.pressure, 0.4);
        assert_eq!(report.contact.pressure_min, 0.0);
        assert_eq!(report.contact.orientation, 0.5);
        assert_eq!(report.contact.tilt, 0.25);
    }

    #[test]
    fn a_hover_keeps_the_buttons_held() {
        let barrel = PointerButtons::SECONDARY;
        assert_eq!(
            pointer_report(VIEW, &event(views::PointerChange::Hover, barrel)).change,
            ReportedChange::Hover {
                buttons: buttons(barrel)
            }
        );
    }

    #[test]
    fn a_signal_is_reported_whatever_movement_it_interrupts() {
        let scroll = PointerEvent {
            signal: Signal::Scroll,
            scroll_delta_y: 40.0,
            ..event(views::PointerChange::Move, PointerButtons::PRIMARY)
        };
        assert_eq!(
            pointer_report(VIEW, &scroll).change,
            ReportedChange::Scroll { delta: [0.0, 40.0] }
        );
        let zoom = PointerEvent {
            signal: Signal::Scale,
            scale: 1.5,
            ..event(views::PointerChange::Hover, PointerButtons::empty())
        };
        assert_eq!(
            pointer_report(VIEW, &zoom).change,
            ReportedChange::Scale { scale: 1.5 }
        );
    }

    #[test]
    fn a_pan_zoom_update_carries_its_totals() {
        let update = PointerEvent {
            kind: views::PointerDeviceKind::Trackpad,
            pan_x: 3.0,
            pan_y: 4.0,
            scale: 1.5,
            rotation: 0.25,
            ..event(views::PointerChange::PanZoomUpdate, PointerButtons::empty())
        };
        assert_eq!(
            pointer_report(VIEW, &update).change,
            ReportedChange::PanZoomUpdate {
                pan: [3.0, 4.0],
                scale: 1.5,
                rotation: 0.25,
            }
        );
    }
}
