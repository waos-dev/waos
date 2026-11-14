//! The mouse as a terminal reports it to a command that asked: a press, a release, a drag or a
//! turn of the wheel at a cell, in xterm's encodings.

use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};

use crate::keys::Modifiers;
use crate::screen::MouseReporting;

/// What the mouse did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    /// A move with `MouseButton` held.
    Drag(MouseButton),
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    fn code(self) -> u8 {
        match self {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }
    }
}

/// The bytes that report `action` at the cell at `column` and `row`, both from 0, to a command
/// that asked as `reporting` says; none for an action it did not ask to hear of, or a cell the
/// encoding cannot name.
pub(crate) fn mouse_report(
    action: MouseAction,
    column: u16,
    row: u16,
    modifiers: Modifiers,
    reporting: MouseReporting,
) -> Option<Vec<u8>> {
    if !is_asked_for(action, reporting.mode) {
        return None;
    }
    let code = action_code(action) + modifier_bits(modifiers);
    let released = matches!(action, MouseAction::Release(_));
    match reporting.encoding {
        Encoding::Sgr => {
            let ending = if released { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{};{}{ending}", column + 1, row + 1).into_bytes())
        }
        Encoding::Default => {
            let code = if released {
                3 + modifier_bits(modifiers)
            } else {
                code
            };
            let mut report = b"\x1b[M".to_vec();
            report.push(32 + code);
            report.push(single_byte_coordinate(column)?);
            report.push(single_byte_coordinate(row)?);
            Some(report)
        }
        Encoding::Utf8 => {
            let code = if released {
                3 + modifier_bits(modifiers)
            } else {
                code
            };
            let mut report = b"\x1b[M".to_vec();
            report.push(32 + code);
            let mut utf8 = [0; 4];
            for coordinate in [column, row] {
                let character = char::from_u32(u32::from(coordinate) + 33)?;
                report.extend_from_slice(character.encode_utf8(&mut utf8).as_bytes());
            }
            Some(report)
        }
    }
}

/// Whether a command that asked for the mouse in `mode` hears of `action`.
fn is_asked_for(action: MouseAction, mode: Mode) -> bool {
    match (mode, action) {
        (Mode::None, _) => false,
        (Mode::Press, MouseAction::Press(_)) => true,
        (Mode::Press, _) => false,
        (Mode::PressRelease, MouseAction::Drag(_)) => false,
        (Mode::PressRelease | Mode::ButtonMotion | Mode::AnyMotion, _) => true,
    }
}

fn action_code(action: MouseAction) -> u8 {
    match action {
        MouseAction::Press(button) | MouseAction::Release(button) => button.code(),
        MouseAction::Drag(button) => 32 + button.code(),
        MouseAction::WheelUp => 64,
        MouseAction::WheelDown => 65,
    }
}

fn modifier_bits(modifiers: Modifiers) -> u8 {
    4 * u8::from(modifiers.shift) + 8 * u8::from(modifiers.alt) + 16 * u8::from(modifiers.control)
}

/// A coordinate as the oldest encoding sends it, one byte from 33 up; none past what a byte holds.
fn single_byte_coordinate(coordinate: u16) -> Option<u8> {
    u8::try_from(coordinate + 33).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reporting(mode: Mode, encoding: Encoding) -> MouseReporting {
        MouseReporting { mode, encoding }
    }

    #[test]
    fn nothing_is_reported_to_a_command_that_did_not_ask() {
        let off = reporting(Mode::None, Encoding::Sgr);
        let press = MouseAction::Press(MouseButton::Left);
        assert_eq!(mouse_report(press, 0, 0, Modifiers::default(), off), None);
    }

    #[test]
    fn sgr_reports_name_the_cell_from_one() {
        let sgr = reporting(Mode::ButtonMotion, Encoding::Sgr);
        let none = Modifiers::default();
        let press = MouseAction::Press(MouseButton::Left);
        assert_eq!(
            mouse_report(press, 4, 2, none, sgr),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        let release = MouseAction::Release(MouseButton::Left);
        assert_eq!(
            mouse_report(release, 4, 2, none, sgr),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
        assert_eq!(
            mouse_report(MouseAction::WheelDown, 0, 0, none, sgr),
            Some(b"\x1b[<65;1;1M".to_vec())
        );
    }

    #[test]
    fn the_oldest_encoding_sends_bytes_and_stops_where_they_end() {
        let old = reporting(Mode::PressRelease, Encoding::Default);
        let none = Modifiers::default();
        let press = MouseAction::Press(MouseButton::Right);
        assert_eq!(
            mouse_report(press, 0, 0, none, old),
            Some(b"\x1b[M\x22!!".to_vec())
        );
        assert_eq!(mouse_report(press, 300, 0, none, old), None);
    }

    #[test]
    fn a_drag_is_reported_only_where_motion_was_asked_for() {
        let drag = MouseAction::Drag(MouseButton::Left);
        let none = Modifiers::default();
        let presses = reporting(Mode::PressRelease, Encoding::Sgr);
        assert_eq!(mouse_report(drag, 1, 1, none, presses), None);
        let motion = reporting(Mode::ButtonMotion, Encoding::Sgr);
        assert_eq!(
            mouse_report(drag, 1, 1, none, motion),
            Some(b"\x1b[<32;2;2M".to_vec())
        );
    }
}
