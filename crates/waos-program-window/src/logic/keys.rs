//! Waft's key events as the program's: both are Flutter's `KeyData`, field for field (the
//! physical key's USB HID usage, the logical key's id, the character it produced), so this is a
//! change of names only.

use waft::ui::KeyEventDeviceType;
use waft_services::KeyEvent as HostEvent;
use waos::bindings::wasdk::platform::views::{KeyDevice, KeyEvent, KeyKind};

use super::event_time::event_time;

/// `event` as the program reads it, at the time the system stamped it.
pub(crate) fn translated(event: &HostEvent) -> KeyEvent {
    KeyEvent {
        time: event_time(event.time_stamp()),
        kind: match event {
            HostEvent::Down(_) => KeyKind::Down,
            HostEvent::Up(_) => KeyKind::Up,
            HostEvent::Repeat(_) => KeyKind::Repeat,
        },
        device: device_of(event.device_type()),
        physical: event.physical_key().usb_hid_usage,
        logical: event.logical_key().key_id,
        character: event.character().map(str::to_owned),
        synthesized: event.synthesized(),
    }
}

fn device_of(device: KeyEventDeviceType) -> KeyDevice {
    match device {
        KeyEventDeviceType::Keyboard => KeyDevice::Keyboard,
        KeyEventDeviceType::DirectionalPad => KeyDevice::DirectionalPad,
        KeyEventDeviceType::Gamepad => KeyDevice::Gamepad,
        KeyEventDeviceType::Joystick => KeyDevice::Joystick,
        KeyEventDeviceType::Hdmi => KeyDevice::Hdmi,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use waft_services::{KeyDownEvent, KeyUpEvent, LogicalKeyboardKey, PhysicalKeyboardKey};

    use super::*;

    #[test]
    fn a_press_keeps_its_keys_its_character_and_its_time() {
        let down = KeyDownEvent::new(
            PhysicalKeyboardKey::KEY_A,
            LogicalKeyboardKey::KEY_A,
            Duration::from_millis(5),
        )
        .character("a");
        let event = translated(&HostEvent::Down(down));
        assert_eq!(event.time, 5_000_000, "the system's stamp, in nanoseconds");
        assert_eq!(event.kind, KeyKind::Down);
        assert_eq!(event.device, KeyDevice::Keyboard);
        assert_eq!(event.physical, 0x0007_0004);
        assert_eq!(event.logical, 0x61);
        assert_eq!(event.character.as_deref(), Some("a"));
        assert!(!event.synthesized);
    }

    #[test]
    fn a_synthesized_release_says_so_and_produces_nothing() {
        let up = KeyUpEvent::new(
            PhysicalKeyboardKey::KEY_A,
            LogicalKeyboardKey::KEY_A,
            Duration::ZERO,
        )
        .synthesized(true)
        .device_type(KeyEventDeviceType::Gamepad);
        let event = translated(&HostEvent::Up(up));
        assert_eq!(event.kind, KeyKind::Up);
        assert_eq!(event.device, KeyDevice::Gamepad);
        assert_eq!(event.character, None);
        assert!(event.synthesized);
    }
}
