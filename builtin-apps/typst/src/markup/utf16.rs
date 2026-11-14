//! Places in a text counted both ways: in UTF-16 code units, as the editor's selection and
//! Typst's problems count them here, and in bytes, as Rust's strings are cut.

/// The byte at which the UTF-16 code unit `unit` of `text` starts; the text's end for a unit
/// past it.
pub(crate) fn byte_of(text: &str, unit: usize) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= unit {
            return byte;
        }
        units += character.len_utf16();
    }
    text.len()
}

/// The line and column, both from 1, of the UTF-16 code unit `unit` of `text`, a column
/// counting characters.
pub(crate) fn line_and_column(text: &str, unit: usize) -> (usize, usize) {
    let before = &text[..byte_of(text, unit)];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    (line, before[line_start..].chars().count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_past_two_unit_characters_are_found() {
        let text = "a😀b";
        assert_eq!(byte_of(text, 0), 0);
        assert_eq!(byte_of(text, 1), 1);
        assert_eq!(byte_of(text, 3), 5);
        assert_eq!(byte_of(text, 9), text.len());
    }

    #[test]
    fn a_caret_is_at_a_line_and_a_column() {
        let text = "one\ntwo\n";
        assert_eq!(line_and_column(text, 0), (1, 1));
        assert_eq!(line_and_column(text, 5), (2, 2));
        assert_eq!(line_and_column(text, 8), (3, 1));
    }
}
