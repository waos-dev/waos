//! Waft's pointer events, as the canvas of a prompt's dialog receives them, as the reports Waft's
//! host gives of a view (`PointerReport`), naming the dialog's view, which WaOS hands its own view
//! of the prompt: positions and lengths in the
//! view's physical pixels, where the canvas has them in logical ones, and each change as the host
//! names it.
//!
//! A widget hears a pointer enter and leave its region, but no event says that a pointer is new,
//! while Waft's converter expects a device to be added before its first report and removed once,
//! as a host reports it. So the devices WaOS has heard of are kept here: one it has not is added
//! before its event, and one it has is added and removed only once. A touch is a device from landing
//! to lifting, so it is removed as it lifts.

use std::collections::HashSet;

use waft::ui::{Contact, PointerDeviceKind, PointerReport, ReportedChange};
use waft_gestures::PointerEvent;

/// The pointers WaOS has been told about.
#[derive(Default)]
pub(crate) struct PromptPointers(HashSet<i64>);

impl PromptPointers {
    /// `event` as the reports WaOS takes, `ratio` physical pixels to a logical one.
    pub(crate) fn translate(&mut self, event: &PointerEvent, ratio: f64) -> Vec<PointerReport> {
        let change = change_of(event, ratio);
        let device = event.device();
        let known = self.0.contains(&device);
        match change {
            ReportedChange::Add if known => return Vec::new(),
            ReportedChange::Remove if !known => return Vec::new(),
            ReportedChange::Remove => {
                self.0.remove(&device);
            }
            _ => {
                self.0.insert(device);
            }
        }
        let mut reports = Vec::new();
        if !known && change != ReportedChange::Add {
            reports.push(report(event, ReportedChange::Add, ratio));
        }
        reports.push(report(event, change, ratio));
        if lifts_a_touch(event, change) {
            self.0.remove(&device);
            reports.push(report(event, ReportedChange::Remove, ratio));
        }
        reports
    }
}

/// What the device did, as a host reports it, its lengths in physical pixels.
fn change_of(event: &PointerEvent, ratio: f64) -> ReportedChange {
    let buttons = event.buttons();
    match event {
        PointerEvent::Added(_) | PointerEvent::Enter(_) => ReportedChange::Add,
        PointerEvent::Removed(_) | PointerEvent::Exit(_) => ReportedChange::Remove,
        PointerEvent::Hover(_) => ReportedChange::Hover { buttons },
        PointerEvent::Down(_) => ReportedChange::Down { buttons },
        PointerEvent::Move(_) => ReportedChange::Move { buttons },
        PointerEvent::Up(_) => ReportedChange::Up { buttons },
        PointerEvent::Cancel(_) => ReportedChange::Cancel { buttons },
        PointerEvent::Scroll(scroll) => ReportedChange::Scroll {
            delta: [
                scroll.scroll_delta.dx() * ratio,
                scroll.scroll_delta.dy() * ratio,
            ],
        },
        PointerEvent::ScrollInertiaCancel(_) => ReportedChange::ScrollInertiaCancel,
        PointerEvent::Scale(scale) => ReportedChange::Scale { scale: scale.scale },
        PointerEvent::PanZoomStart(_) => ReportedChange::PanZoomStart,
        PointerEvent::PanZoomUpdate(gesture) => {
            let pan = gesture.local_pan();
            ReportedChange::PanZoomUpdate {
                pan: [pan.dx() * ratio, pan.dy() * ratio],
                scale: gesture.scale,
                rotation: gesture.rotation,
            }
        }
        PointerEvent::PanZoomEnd(_) => ReportedChange::PanZoomEnd,
    }
}

/// Whether `change` is the end of a touch, which lifts or is taken away.
fn lifts_a_touch(event: &PointerEvent, change: ReportedChange) -> bool {
    event.kind() == PointerDeviceKind::Touch
        && matches!(
            change,
            ReportedChange::Up { .. } | ReportedChange::Cancel { .. }
        )
}

/// `$event`'s contact, whichever kind of event it is, its radii in physical pixels.
macro_rules! contact_of {
    ($event:expr, $ratio:expr, $($variant:ident),+) => {
        match $event {
            $(PointerEvent::$variant(e) => Contact {
                pressure: e.pressure,
                pressure_min: e.pressure_min,
                pressure_max: e.pressure_max,
                distance: e.distance,
                distance_max: e.distance_max,
                size: e.size,
                radius_major: e.radius_major * $ratio,
                radius_minor: e.radius_minor * $ratio,
                radius_min: e.radius_min * $ratio,
                radius_max: e.radius_max * $ratio,
                orientation: e.orientation,
                tilt: e.tilt,
            },)+
        }
    };
}

fn contact(event: &PointerEvent, ratio: f64) -> Contact {
    contact_of!(
        event,
        ratio,
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

/// One report of `event`, as `change`.
fn report(event: &PointerEvent, change: ReportedChange, ratio: f64) -> PointerReport {
    let position = event.local_position();
    PointerReport {
        view_id: event.view_id(),
        time_stamp: event.time_stamp(),
        device: event.device(),
        kind: event.kind(),
        position: [position.dx() * ratio, position.dy() * ratio],
        change,
        contact: contact(event, ratio),
    }
}

#[cfg(test)]
mod tests {
    use waft::Offset;
    use waft_gestures::{
        PointerDownEvent, PointerExitEvent, PointerHoverEvent, PointerScrollEvent, PointerUpEvent,
    };

    use super::*;

    fn hover(device: i64, x: f64, y: f64) -> PointerEvent {
        PointerEvent::Hover(PointerHoverEvent {
            device,
            kind: PointerDeviceKind::Mouse,
            position: Offset::new(x, y),
            ..PointerHoverEvent::default()
        })
    }

    fn changes(reports: &[PointerReport]) -> Vec<ReportedChange> {
        reports.iter().map(|report| report.change).collect()
    }

    #[test]
    fn a_new_pointer_is_added_before_its_first_report_in_physical_pixels() {
        let mut pointers = PromptPointers::default();
        let reports = pointers.translate(&hover(1, 10.0, 20.0), 2.0);
        assert_eq!(
            changes(&reports),
            [ReportedChange::Add, ReportedChange::Hover { buttons: 0 }]
        );
        assert_eq!(reports[1].position, [20.0, 40.0]);
        assert_eq!(reports[1].device, 1);
        assert_eq!(reports[1].kind, PointerDeviceKind::Mouse);
    }

    #[test]
    fn a_pointer_is_removed_once_and_only_after_it_was_added() {
        let mut pointers = PromptPointers::default();
        let exit = PointerEvent::Exit(PointerExitEvent {
            device: 4,
            ..PointerExitEvent::default()
        });
        assert!(pointers.translate(&exit, 1.0).is_empty());
        pointers.translate(&hover(4, 0.0, 0.0), 1.0);
        assert_eq!(
            changes(&pointers.translate(&exit, 1.0)),
            [ReportedChange::Remove]
        );
        assert!(pointers.translate(&exit, 1.0).is_empty());
    }

    #[test]
    fn a_touch_is_removed_as_it_lifts() {
        let mut pointers = PromptPointers::default();
        let down = PointerEvent::Down(PointerDownEvent {
            device: 7,
            kind: PointerDeviceKind::Touch,
            buttons: 1,
            ..PointerDownEvent::default()
        });
        let up = PointerEvent::Up(PointerUpEvent {
            device: 7,
            kind: PointerDeviceKind::Touch,
            ..PointerUpEvent::default()
        });
        assert_eq!(
            changes(&pointers.translate(&down, 1.0)),
            [ReportedChange::Add, ReportedChange::Down { buttons: 1 }]
        );
        assert_eq!(
            changes(&pointers.translate(&up, 1.0)),
            [ReportedChange::Up { buttons: 0 }, ReportedChange::Remove]
        );
        assert_eq!(
            changes(&pointers.translate(&down, 1.0)),
            [ReportedChange::Add, ReportedChange::Down { buttons: 1 }],
            "a new touch"
        );
    }

    #[test]
    fn a_scroll_goes_by_physical_pixels() {
        let mut pointers = PromptPointers::default();
        pointers.translate(&hover(1, 0.0, 0.0), 2.0);
        let scroll = PointerEvent::Scroll(PointerScrollEvent {
            device: 1,
            kind: PointerDeviceKind::Mouse,
            scroll_delta: Offset::new(0.0, 15.0),
            ..PointerScrollEvent::default()
        });
        assert_eq!(
            changes(&pointers.translate(&scroll, 2.0)),
            [ReportedChange::Scroll { delta: [0.0, 30.0] }]
        );
    }
}
