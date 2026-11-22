//! Events as the platform reports them, made by hand, for a test of how the engine hands them on:
//! a view's geometry, and a mouse's change at a set time on the program's clock. A test that only
//! drives a program as its user would gives its input through the program's `Mouse` and the
//! `Keyboard` instead.

use waos::bindings::wasdk::platform::views::{
    Insets, Metrics, PointerButtons, PointerChange, PointerDeviceKind, PointerEvent,
    PointerSignalKind,
};

/// A view `width` by `height` physical pixels at the test shell's 2x, with nothing around it.
pub fn metrics(width: u32, height: u32) -> Metrics {
    let none = Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    Metrics {
        width,
        height,
        device_pixel_ratio: 2.0,
        padding: none,
        view_padding: none,
        view_insets: none,
    }
}

/// A mouse's `change` at `(x, y)` in a view's physical pixels, at `time` on the program's clock,
/// with `buttons` held.
pub fn mouse(
    change: PointerChange,
    buttons: PointerButtons,
    (x, y): (f64, f64),
    time: u64,
) -> PointerEvent {
    PointerEvent {
        time,
        change,
        kind: PointerDeviceKind::Mouse,
        signal: PointerSignalKind::None,
        device: 1,
        x,
        y,
        buttons,
        pressure: 0.0,
        pressure_min: 0.0,
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
