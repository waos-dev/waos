//! Text committed into what a view edits, as the platform's input methods commit theirs, as Waft's
//! host commits an input method's text (its `apply_commit`, after Flutter's macOS
//! `insertText:replacementRange:`): in place of the text being composed, or else of the selection,
//! or else at the start; the caret after it, and nothing composed. Offsets count UTF-16 code
//! units, as both sides of a text input do.

use crate::bindings::wasdk::platform::views::{
    TextAffinity, TextEditingValue, TextRange, TextSelection,
};

/// `value` with `text` committed into it.
pub(super) fn committed(value: &TextEditingValue, text: &str) -> TextEditingValue {
    let mut units: Vec<u16> = value.text.encode_utf16().collect();
    let (start, end) = replaced_range(value, units.len());
    let inserted: Vec<u16> = text.encode_utf16().collect();
    let caret = start + inserted.len();
    units.splice(start..end, inserted);
    TextEditingValue {
        text: String::from_utf16_lossy(&units),
        selection: Some(TextSelection {
            base_offset: offset(caret),
            extent_offset: offset(caret),
            affinity: TextAffinity::Downstream,
            is_directional: false,
        }),
        composing: None,
    }
}

/// What a commit replaces, within a text of `len` code units: what is composed, or else what is
/// selected, or else nothing, at the start.
fn replaced_range(value: &TextEditingValue, len: usize) -> (usize, usize) {
    let range = match (value.composing, value.selection) {
        (Some(TextRange { start, end }), _) => (start, end),
        (None, Some(selection)) => (
            selection.base_offset.min(selection.extent_offset),
            selection.base_offset.max(selection.extent_offset),
        ),
        (None, None) => (0, 0),
    };
    let within = |offset: u32| (offset as usize).min(len);
    (within(range.0), within(range.1))
}

/// A count of code units as a text input's offset.
fn offset(units: usize) -> u32 {
    u32::try_from(units).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str, selection: (u32, u32), composing: Option<(u32, u32)>) -> TextEditingValue {
        TextEditingValue {
            text: text.to_owned(),
            selection: Some(TextSelection {
                base_offset: selection.0,
                extent_offset: selection.1,
                affinity: TextAffinity::Upstream,
                is_directional: true,
            }),
            composing: composing.map(|(start, end)| TextRange { start, end }),
        }
    }

    fn caret(value: &TextEditingValue) -> (u32, u32) {
        let selection = value.selection.expect("a caret");
        (selection.base_offset, selection.extent_offset)
    }

    #[test]
    fn text_goes_in_at_the_caret_which_moves_after_it() {
        let after = committed(&value("milk eggs", (5, 5), None), "and ");
        assert_eq!(after.text, "milk and eggs");
        assert_eq!(caret(&after), (9, 9));
        assert!(after.composing.is_none());
        let selection = after.selection.unwrap();
        assert_eq!(selection.affinity, TextAffinity::Downstream);
        assert!(!selection.is_directional);
    }

    #[test]
    fn text_takes_the_place_of_a_selection_made_either_way() {
        let after = committed(&value("milk eggs", (9, 5), None), "bread");
        assert_eq!(after.text, "milk bread");
        assert_eq!(caret(&after), (10, 10));
    }

    #[test]
    fn text_takes_the_place_of_what_is_composed() {
        let after = committed(&value("milk にほ", (7, 7), Some((5, 7))), "日本");
        assert_eq!(after.text, "milk 日本");
        assert_eq!(caret(&after), (7, 7));
    }

    #[test]
    fn offsets_count_utf16_code_units() {
        let after = committed(&value("😀", (2, 2), None), "é");
        assert_eq!(after.text, "😀é");
        assert_eq!(caret(&after), (3, 3));
    }

    #[test]
    fn with_neither_a_caret_nor_a_selection_text_goes_in_at_the_start() {
        let mut empty = value("eggs", (0, 0), None);
        empty.selection = None;
        let after = committed(&empty, "milk ");
        assert_eq!(after.text, "milk eggs");
        assert_eq!(caret(&after), (5, 5));
    }

    #[test]
    fn offsets_past_the_end_are_the_end() {
        let after = committed(&value("milk", (40, 40), None), "!");
        assert_eq!(after.text, "milk!");
        assert_eq!(caret(&after), (5, 5));
    }
}
