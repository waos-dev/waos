//! Typing a key the framework did not handle into the active text input: the key half of
//! Flutter's macOS `FlutterTextInputPlugin.handleKeyEvent`, as Waft's winit host ports it. The
//! key's character goes in at the selection, and Enter acts.
//!
//! Backspace, Delete and the arrows type nothing here: this platform turns no key into an edit,
//! so the framework's own key bindings take them, and they reach this module only when no field
//! wanted them. A key pressed while Control or Command is held is a chord for the menus, as
//! `NSTextInputContext` hands one to the menu and never to `insertText:`, so it types nothing
//! whatever character it produced; which of those are held is kept from the keys themselves,
//! since the platform sends a modifier that changed unseen as a synthesized key.

use std::collections::BTreeSet;

use waft_embedder::{
    KeyData, KeyEventType, TextAffinity, TextEditingValue, TextInputAction, TextInputConfiguration,
    TextInputType, TextRange, TextSelection,
};

/// Flutter's `LogicalKeyboardKey.enter` and `numpadEnter`: either one acts as Enter, as the
/// winit key both come from is `NamedKey::Enter`.
const ENTER: u64 = 0x0001_0000_000d;
const NUMPAD_ENTER: u64 = 0x0002_0000_020d;

/// Flutter's `LogicalKeyboardKey.controlLeft`, `controlRight`, `metaLeft` and `metaRight`: the
/// keys that make a key a chord. Meta is Command on Apple's keyboards and the Windows key on
/// others.
const CHORD_KEYS: [u64; 4] = [
    0x0002_0000_0100,
    0x0002_0000_0101,
    0x0002_0000_0106,
    0x0002_0000_0107,
];

/// The text input a view was started with, kept until it stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveTextInput {
    multiline: bool,
    input_action: TextInputAction,
}

impl ActiveTextInput {
    pub(crate) fn new(configuration: &TextInputConfiguration) -> ActiveTextInput {
        ActiveTextInput {
            multiline: configuration.input_type.index == TextInputType::MULTILINE.index,
            input_action: configuration.input_action,
        }
    }

    /// What `FlutterTextInputPlugin` does with `key`, which the framework did not handle, while
    /// this input is active; `chord_held` says Control or Command is down.
    pub(crate) fn key_effect(&self, key: &KeyData, chord_held: bool) -> TextInputKeyEffect {
        if !is_users_press(key) || chord_held {
            return TextInputKeyEffect::None;
        }
        if key.logical == ENTER || key.logical == NUMPAD_ENTER {
            return self.newline_effect();
        }
        match printable_character(key) {
            Some(text) => TextInputKeyEffect::Insert(text),
            None => TextInputKeyEffect::None,
        }
    }

    /// `FlutterTextInputPlugin.insertNewline:`: a multiline field with the newline action gets
    /// the "\n"; every field gets `performAction`.
    fn newline_effect(&self) -> TextInputKeyEffect {
        let inserts = self.multiline && self.input_action == TextInputAction::Newline;
        TextInputKeyEffect::Enter {
            insert: inserts.then(|| "\n".to_owned()),
            action: self.input_action,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TextInputKeyEffect {
    None,
    Insert(String),
    Enter {
        insert: Option<String>,
        action: TextInputAction,
    },
}

/// A press or a repeat the user made. A release types nothing, and neither does a key the
/// platform synthesized to keep the framework's picture of the keyboard right, which no one
/// pressed.
fn is_users_press(key: &KeyData) -> bool {
    matches!(key.event_type, KeyEventType::Down | KeyEventType::Repeat) && !key.synthesized
}

/// The key's character, unless it is a control character, which Flutter's embedders keep out
/// of `KeyData.character` and a platform may not have.
fn printable_character(key: &KeyData) -> Option<String> {
    key.character
        .as_deref()
        .filter(|character| !is_control_character(character))
        .map(str::to_owned)
}

/// Flutter's `LogicalKeyboardKey.isControlCharacter`.
fn is_control_character(label: &str) -> bool {
    let mut characters = label.chars();
    let (Some(character), None) = (characters.next(), characters.next()) else {
        return false;
    };
    let code_point = u32::from(character);
    code_point <= 0x1f || (0x7f..=0x9f).contains(&code_point)
}

/// The Control and Command keys held down, by logical key, as Flutter's `HardwareKeyboard`
/// keeps them.
#[derive(Default)]
pub(crate) struct ChordModifiers {
    held: BTreeSet<u64>,
}

impl ChordModifiers {
    /// Follows `key`, if it is one of the chord keys, down and up.
    pub(crate) fn record(&mut self, key: &KeyData) {
        if !CHORD_KEYS.contains(&key.logical) {
            return;
        }
        match key.event_type {
            KeyEventType::Down => {
                self.held.insert(key.logical);
            }
            KeyEventType::Up => {
                self.held.remove(&key.logical);
            }
            KeyEventType::Repeat => {}
        }
    }

    pub(crate) fn any_held(&self) -> bool {
        !self.held.is_empty()
    }
}

/// `value` with `text` in place of what is being composed, or else of the selection, and the
/// caret after it: an input method's commit, as Waft's winit host applies one.
pub(crate) fn inserted(value: &TextEditingValue, text: &str) -> TextEditingValue {
    let range = insertion_range(value);
    value
        .replaced(range, text)
        .composing(TextRange::EMPTY)
        .selection(TextSelection::collapsed(
            range.start + utf16_len(text),
            TextAffinity::Downstream,
        ))
}

/// What an insertion overwrites: the composing range if there is one, else the selection, else
/// the start of the text.
fn insertion_range(value: &TextEditingValue) -> TextRange {
    if value.composing.is_valid() {
        return value.composing;
    }
    let selection = value.selection.range();
    if selection.is_valid() {
        selection
    } else {
        TextRange::collapsed(0)
    }
}

/// Dart's `String.length`: UTF-16 code units.
fn utf16_len(text: &str) -> i32 {
    i32::try_from(text.encode_utf16().count()).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use waft_embedder::KeyEventDeviceType;

    use super::*;

    const KEY_A: u64 = 0x61;
    const BACKSPACE: u64 = 0x0001_0000_0008;
    const SHIFT_LEFT: u64 = 0x0002_0000_0102;
    const META_LEFT: u64 = 0x0002_0000_0106;
    const CONTROL_RIGHT: u64 = 0x0002_0000_0101;

    fn single_line() -> ActiveTextInput {
        ActiveTextInput {
            multiline: false,
            input_action: TextInputAction::Done,
        }
    }

    fn multiline() -> ActiveTextInput {
        ActiveTextInput {
            multiline: true,
            input_action: TextInputAction::Newline,
        }
    }

    fn key(event_type: KeyEventType, logical: u64, character: Option<&str>) -> KeyData {
        KeyData {
            time_stamp: Duration::ZERO,
            event_type,
            device_type: KeyEventDeviceType::Keyboard,
            physical: 0,
            logical,
            character: character.map(str::to_owned),
            synthesized: false,
        }
    }

    fn pressed(logical: u64, character: Option<&str>) -> KeyData {
        key(KeyEventType::Down, logical, character)
    }

    fn caret_at(text: &str, offset: i32) -> TextEditingValue {
        TextEditingValue::new()
            .text(text)
            .selection(TextSelection::collapsed(offset, TextAffinity::Downstream))
    }

    #[test]
    fn a_character_key_inserts_its_character() {
        let effect = single_line().key_effect(&pressed(KEY_A, Some("a")), false);
        assert_eq!(effect, TextInputKeyEffect::Insert("a".into()));
    }

    #[test]
    fn a_held_key_types_again_with_each_repeat() {
        let repeat = key(KeyEventType::Repeat, KEY_A, Some("a"));
        assert_eq!(
            single_line().key_effect(&repeat, false),
            TextInputKeyEffect::Insert("a".into())
        );
    }

    #[test]
    fn shift_and_option_still_type() {
        let effect = single_line().key_effect(&pressed(0xe5, Some("å")), false);
        assert_eq!(effect, TextInputKeyEffect::Insert("å".into()));
    }

    #[test]
    fn command_and_control_chords_do_not_type() {
        let effect = single_line().key_effect(&pressed(0x63, Some("c")), true);
        assert_eq!(effect, TextInputKeyEffect::None);
    }

    #[test]
    fn control_characters_do_not_type() {
        let effect = single_line().key_effect(&pressed(BACKSPACE, Some("\u{8}")), false);
        assert_eq!(effect, TextInputKeyEffect::None);
    }

    #[test]
    fn a_release_and_a_synthesized_key_do_not_type() {
        let released = key(KeyEventType::Up, KEY_A, Some("a"));
        assert_eq!(
            single_line().key_effect(&released, false),
            TextInputKeyEffect::None
        );
        let synthesized = KeyData {
            synthesized: true,
            ..pressed(KEY_A, Some("a"))
        };
        assert_eq!(
            single_line().key_effect(&synthesized, false),
            TextInputKeyEffect::None
        );
    }

    #[test]
    fn enter_performs_the_action_and_only_a_multiline_field_gets_the_newline() {
        let enter = pressed(ENTER, None);
        assert_eq!(
            single_line().key_effect(&enter, false),
            TextInputKeyEffect::Enter {
                insert: None,
                action: TextInputAction::Done,
            }
        );
        assert_eq!(
            multiline().key_effect(&enter, false),
            TextInputKeyEffect::Enter {
                insert: Some("\n".into()),
                action: TextInputAction::Newline,
            }
        );
    }

    #[test]
    fn the_numpad_enter_is_enter() {
        assert_eq!(
            single_line().key_effect(&pressed(NUMPAD_ENTER, None), false),
            TextInputKeyEffect::Enter {
                insert: None,
                action: TextInputAction::Done,
            }
        );
    }

    #[test]
    fn a_multiline_field_is_known_by_its_input_type() {
        let configuration = TextInputConfiguration::builder()
            .input_type(TextInputType::MULTILINE)
            .input_action(TextInputAction::Newline)
            .build();
        assert_eq!(ActiveTextInput::new(&configuration), multiline());
    }

    #[test]
    fn a_chord_lasts_while_control_or_command_is_down() {
        let mut modifiers = ChordModifiers::default();
        modifiers.record(&pressed(META_LEFT, None));
        modifiers.record(&pressed(CONTROL_RIGHT, None));
        assert!(modifiers.any_held());
        modifiers.record(&key(KeyEventType::Up, META_LEFT, None));
        assert!(modifiers.any_held());
        modifiers.record(&key(KeyEventType::Up, CONTROL_RIGHT, None));
        assert!(!modifiers.any_held());
    }

    #[test]
    fn shift_makes_no_chord() {
        let mut modifiers = ChordModifiers::default();
        modifiers.record(&pressed(SHIFT_LEFT, None));
        assert!(!modifiers.any_held());
    }

    #[test]
    fn an_insertion_goes_in_at_the_caret() {
        let value = inserted(&caret_at("", 0), "hi");
        assert_eq!(value, caret_at("hi", 2).composing(TextRange::EMPTY));
    }

    #[test]
    fn an_insertion_replaces_the_selection() {
        let selected = TextEditingValue::new()
            .text("hello")
            .selection(TextSelection::new(4, 1));
        assert_eq!(inserted(&selected, "a"), caret_at("hao", 2));
    }

    #[test]
    fn an_insertion_replaces_what_is_being_composed() {
        let composing = TextEditingValue::new()
            .text("ni")
            .selection(TextSelection::new(0, 2))
            .composing(TextRange::new(0, 2));
        assert_eq!(inserted(&composing, "你"), caret_at("你", 1));
    }

    #[test]
    fn an_insertion_counts_utf16_code_units() {
        let value = inserted(&caret_at("a😀", 3), "b");
        assert_eq!(value, caret_at("a😀b", 4));
    }

    #[test]
    fn with_no_selection_an_insertion_goes_at_the_start() {
        let value = inserted(&TextEditingValue::new().text("b"), "a");
        assert_eq!(value.text, "ab");
        assert_eq!(value.selection.base_offset, 1);
        assert_eq!(value.selection.extent_offset, 1);
    }
}
