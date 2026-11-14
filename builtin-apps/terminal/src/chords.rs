//! The keys that are the terminal's own and never reach the shell: Control and Shift with a key,
//! since Control with a letter alone is the shell's, and Shift with Page Up and Page Down. On a
//! Mac the menus offer the same commands with Command; these are for every keyboard.

use waft_services::LogicalKeyboardKey as Key;

use crate::keys::Modifiers;
use crate::text_size::TextSizeStep;

/// What one of the terminal's own keys does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCommand {
    Copy,
    Paste,
    Clear,
    NewWindow,
    CloseWindow,
    SizeText(TextSizeStep),
    /// Scrolls a screen's height up into the scrollback.
    PageUp,
    /// Scrolls a screen's height down towards the screen.
    PageDown,
}

/// The command `key` asks for with `modifiers` held; none for a key that is the shell's.
pub(crate) fn command_for_key(key: Key, modifiers: Modifiers) -> Option<TerminalCommand> {
    if modifiers.meta || modifiers.alt || !modifiers.shift {
        return None;
    }
    if !modifiers.control {
        return match key {
            Key::PAGE_UP => Some(TerminalCommand::PageUp),
            Key::PAGE_DOWN => Some(TerminalCommand::PageDown),
            _ => None,
        };
    }
    let command = match key {
        Key::KEY_C => TerminalCommand::Copy,
        Key::KEY_V => TerminalCommand::Paste,
        Key::KEY_K => TerminalCommand::Clear,
        Key::KEY_N => TerminalCommand::NewWindow,
        Key::KEY_W => TerminalCommand::CloseWindow,
        Key::EQUAL | Key::ADD => TerminalCommand::SizeText(TextSizeStep::Bigger),
        Key::MINUS | Key::UNDERSCORE => TerminalCommand::SizeText(TextSizeStep::Smaller),
        Key::DIGIT0 | Key::PARENTHESIS_RIGHT => TerminalCommand::SizeText(TextSizeStep::Default),
        _ => return None,
    };
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(control: bool, shift: bool) -> Modifiers {
        Modifiers {
            control,
            shift,
            ..Modifiers::default()
        }
    }

    #[test]
    fn control_with_a_letter_alone_stays_the_shells() {
        assert_eq!(command_for_key(Key::KEY_C, held(true, false)), None);
        assert_eq!(command_for_key(Key::KEY_C, held(false, true)), None);
    }

    #[test]
    fn control_and_shift_are_the_terminals_own() {
        assert_eq!(
            command_for_key(Key::KEY_C, held(true, true)),
            Some(TerminalCommand::Copy)
        );
        assert_eq!(
            command_for_key(Key::KEY_N, held(true, true)),
            Some(TerminalCommand::NewWindow)
        );
        assert_eq!(
            command_for_key(Key::EQUAL, held(true, true)),
            Some(TerminalCommand::SizeText(TextSizeStep::Bigger))
        );
    }

    #[test]
    fn shift_with_the_page_keys_scrolls() {
        assert_eq!(
            command_for_key(Key::PAGE_UP, held(false, true)),
            Some(TerminalCommand::PageUp)
        );
        assert_eq!(command_for_key(Key::PAGE_UP, held(false, false)), None);
    }
}
