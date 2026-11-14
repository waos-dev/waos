//! The platform's key events as dart:ui `KeyData`.
//!
//! The `views` interface speaks Flutter's `KeyData` vocabulary, so this is a rename and
//! nothing else: no key table, no layout, no state to keep. What a platform must do to
//! reach this shape it has already done.

use std::time::Duration;

use waft_embedder::{KeyData, KeyEventDeviceType, KeyEventType};
use wasdk::views::{KeyDevice, KeyEvent, KeyKind};

/// The platform's key event as `KeyData`: the same vocabulary, field for field.
pub(crate) fn key_data(event: KeyEvent) -> KeyData {
    KeyData {
        time_stamp: Duration::from_nanos(event.time),
        event_type: match event.kind {
            KeyKind::Down => KeyEventType::Down,
            KeyKind::Up => KeyEventType::Up,
            KeyKind::Repeat => KeyEventType::Repeat,
        },
        device_type: match event.device {
            KeyDevice::Keyboard => KeyEventDeviceType::Keyboard,
            KeyDevice::DirectionalPad => KeyEventDeviceType::DirectionalPad,
            KeyDevice::Gamepad => KeyEventDeviceType::Gamepad,
            KeyDevice::Joystick => KeyEventDeviceType::Joystick,
            KeyDevice::Hdmi => KeyEventDeviceType::Hdmi,
        },
        physical: event.physical,
        logical: event.logical,
        character: event.character,
        synthesized: event.synthesized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(character: Option<&str>) -> KeyEvent {
        KeyEvent {
            time: 1_500_000,
            kind: KeyKind::Down,
            device: KeyDevice::Keyboard,
            physical: 0x0007_0004,
            logical: u64::from('a'),
            character: character.map(str::to_owned),
            synthesized: false,
        }
    }

    #[test]
    fn a_key_keeps_every_field_it_came_with() {
        let data = key_data(pressed(Some("a")));
        assert_eq!(data.time_stamp, Duration::from_micros(1_500));
        assert_eq!(data.event_type, KeyEventType::Down);
        assert_eq!(data.device_type, KeyEventDeviceType::Keyboard);
        assert_eq!(data.physical, 0x0007_0004);
        assert_eq!(data.logical, u64::from('a'));
        assert_eq!(data.character.as_deref(), Some("a"));
        assert!(!data.synthesized);
    }

    /// A modifier the platform synthesized before a pointer event stays one, so the framework
    /// knows no one pressed it just now.
    #[test]
    fn a_synthesized_key_says_so() {
        let event = KeyEvent {
            synthesized: true,
            ..pressed(None)
        };
        assert!(key_data(event).synthesized);
    }

    #[test]
    fn a_release_carries_no_character() {
        let event = KeyEvent {
            kind: KeyKind::Up,
            ..pressed(None)
        };
        let data = key_data(event);
        assert_eq!(data.event_type, KeyEventType::Up);
        assert_eq!(data.character, None);
    }
}
