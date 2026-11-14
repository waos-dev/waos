//! The text a program edits, between the program's terms and Waft's: the value it holds and
//! the configuration it starts editing with, on their way to Waft's text input, and what an
//! input method did, on its way back.
//!
//! Both sides are Flutter's text input channel field for field, and both count offsets in
//! UTF-16 code units, so offsets are copied, never recounted. The difference is how "none" is
//! said: the program leaves a selection or a composing range out, Waft gives it offsets of -1.

use waft::ui::{
    TextAffinity as WaftAffinity, TextEditingValue as WaftEditingValue,
    TextInputAction as WaftAction, TextInputConfiguration as WaftConfiguration,
    TextInputType as WaftInputType, TextRange as WaftRange, TextSelection as WaftSelection, ViewId,
};
use waft_services::TextCapitalization as WaftCapitalization;
use waos::bindings::wasdk::platform::views::{
    TextAffinity, TextCapitalization, TextEditingValue, TextInputAction, TextInputConfiguration,
    TextInputType, TextRange, TextSelection,
};

/// The value the program holds, as Waft's text input takes it.
pub(crate) fn waft_editing_value(value: &TextEditingValue) -> WaftEditingValue {
    WaftEditingValue::new()
        .text(value.text.clone())
        .selection(
            value
                .selection
                .map_or(WaftEditingValue::EMPTY.selection, waft_selection),
        )
        .composing(value.composing.map_or(WaftRange::EMPTY, waft_range))
}

/// A value an input method edited, as the program reads it.
pub(crate) fn program_editing_value(value: &WaftEditingValue) -> TextEditingValue {
    TextEditingValue {
        text: value.text.clone(),
        selection: value
            .selection
            .is_valid()
            .then(|| program_selection(value.selection)),
        composing: value
            .composing
            .is_valid()
            .then(|| program_range(value.composing)),
    }
}

/// What the program's text is for, as Waft's text input takes it, for the Waft view `view`
/// the program's canvas is drawn in. Waft names a view by a 32-bit id, which every view its
/// hosts make fits.
pub(crate) fn waft_configuration(
    configuration: &TextInputConfiguration,
    view: ViewId,
) -> WaftConfiguration {
    let waft = WaftConfiguration::builder()
        .input_type(waft_input_type(configuration.input_type))
        .input_action(waft_action(configuration.input_action))
        .obscure_text(configuration.obscure_text)
        .autocorrect(configuration.autocorrect)
        .enable_suggestions(configuration.enable_suggestions)
        .text_capitalization(waft_capitalization(configuration.text_capitalization))
        .read_only(configuration.read_only);
    match i32::try_from(view.0) {
        Ok(view_id) => waft.view_id(view_id).build(),
        Err(_) => waft.build(),
    }
}

/// The action the user asked for, as the program names it.
pub(crate) fn program_action(action: WaftAction) -> TextInputAction {
    match action {
        WaftAction::None => TextInputAction::None,
        WaftAction::Unspecified => TextInputAction::Unspecified,
        WaftAction::Done => TextInputAction::Done,
        WaftAction::Go => TextInputAction::Go,
        WaftAction::Search => TextInputAction::Search,
        WaftAction::Send => TextInputAction::Send,
        WaftAction::Next => TextInputAction::Next,
        WaftAction::Previous => TextInputAction::Previous,
        WaftAction::ContinueAction => TextInputAction::ContinueAction,
        WaftAction::Join => TextInputAction::Join,
        WaftAction::Route => TextInputAction::Route,
        WaftAction::EmergencyCall => TextInputAction::EmergencyCall,
        WaftAction::Newline => TextInputAction::Newline,
    }
}

fn waft_action(action: TextInputAction) -> WaftAction {
    match action {
        TextInputAction::None => WaftAction::None,
        TextInputAction::Unspecified => WaftAction::Unspecified,
        TextInputAction::Done => WaftAction::Done,
        TextInputAction::Go => WaftAction::Go,
        TextInputAction::Search => WaftAction::Search,
        TextInputAction::Send => WaftAction::Send,
        TextInputAction::Next => WaftAction::Next,
        TextInputAction::Previous => WaftAction::Previous,
        TextInputAction::ContinueAction => WaftAction::ContinueAction,
        TextInputAction::Join => WaftAction::Join,
        TextInputAction::Route => WaftAction::Route,
        TextInputAction::EmergencyCall => WaftAction::EmergencyCall,
        TextInputAction::Newline => WaftAction::Newline,
    }
}

fn waft_selection(selection: TextSelection) -> WaftSelection {
    WaftSelection::new(
        waft_offset(selection.base_offset),
        waft_offset(selection.extent_offset),
    )
    .affinity(waft_affinity(selection.affinity))
    .is_directional(selection.is_directional)
}

/// A selection Waft holds, which has offsets of its own: `is_valid` has been checked.
fn program_selection(selection: WaftSelection) -> TextSelection {
    TextSelection {
        base_offset: program_offset(selection.base_offset),
        extent_offset: program_offset(selection.extent_offset),
        affinity: program_affinity(selection.affinity),
        is_directional: selection.is_directional,
    }
}

fn waft_range(range: TextRange) -> WaftRange {
    WaftRange::new(waft_offset(range.start), waft_offset(range.end))
}

/// A range Waft holds, which has offsets of its own: `is_valid` has been checked.
fn program_range(range: WaftRange) -> TextRange {
    TextRange {
        start: program_offset(range.start),
        end: program_offset(range.end),
    }
}

/// An offset Waft can hold. None is past `i32::MAX`: that is a text of 4 GiB.
fn waft_offset(offset: u32) -> i32 {
    i32::try_from(offset).unwrap_or(i32::MAX)
}

fn program_offset(offset: i32) -> u32 {
    u32::try_from(offset).unwrap_or(0)
}

fn waft_affinity(affinity: TextAffinity) -> WaftAffinity {
    match affinity {
        TextAffinity::Upstream => WaftAffinity::Upstream,
        TextAffinity::Downstream => WaftAffinity::Downstream,
    }
}

fn program_affinity(affinity: WaftAffinity) -> TextAffinity {
    match affinity {
        WaftAffinity::Upstream => TextAffinity::Upstream,
        WaftAffinity::Downstream => TextAffinity::Downstream,
    }
}

fn waft_input_type(input_type: TextInputType) -> WaftInputType {
    match input_type {
        TextInputType::Text => WaftInputType::TEXT,
        TextInputType::Multiline => WaftInputType::MULTILINE,
        TextInputType::Number(format) => WaftInputType::number_with_options()
            .signed(format.signed)
            .decimal(format.decimal),
        TextInputType::Phone => WaftInputType::PHONE,
        TextInputType::Datetime => WaftInputType::DATETIME,
        TextInputType::EmailAddress => WaftInputType::EMAIL_ADDRESS,
        TextInputType::Url => WaftInputType::URL,
        TextInputType::VisiblePassword => WaftInputType::VISIBLE_PASSWORD,
        TextInputType::Name => WaftInputType::NAME,
        TextInputType::StreetAddress => WaftInputType::STREET_ADDRESS,
        TextInputType::None => WaftInputType::NONE,
        TextInputType::WebSearch => WaftInputType::WEB_SEARCH,
        TextInputType::Twitter => WaftInputType::TWITTER,
    }
}

fn waft_capitalization(capitalization: TextCapitalization) -> WaftCapitalization {
    match capitalization {
        TextCapitalization::Words => WaftCapitalization::Words,
        TextCapitalization::Sentences => WaftCapitalization::Sentences,
        TextCapitalization::Characters => WaftCapitalization::Characters,
        TextCapitalization::None => WaftCapitalization::None,
    }
}

#[cfg(test)]
mod tests {
    use waos::bindings::wasdk::platform::views::NumberFormat;

    use super::*;

    /// The generated types compare nothing whole: their `Debug` says every field.
    fn assert_same(value: &TextEditingValue, expected: &TextEditingValue) {
        assert_eq!(format!("{value:?}"), format!("{expected:?}"));
    }

    #[test]
    fn a_value_survives_the_trip_through_waft() {
        let value = TextEditingValue {
            // Offsets count UTF-16 code units: the emoji is two of them, so 3 is after the "a".
            text: "😀a你好".to_owned(),
            selection: Some(TextSelection {
                base_offset: 5,
                extent_offset: 3,
                affinity: TextAffinity::Upstream,
                is_directional: true,
            }),
            composing: Some(TextRange { start: 3, end: 5 }),
        };
        let waft = waft_editing_value(&value);
        assert_eq!(
            (waft.selection.base_offset, waft.selection.extent_offset),
            (5, 3)
        );
        assert_eq!(waft.composing, WaftRange::new(3, 5));
        assert_same(&program_editing_value(&waft), &value);
    }

    #[test]
    fn no_selection_and_nothing_composed_are_waft_offsets_of_minus_one() {
        let value = TextEditingValue {
            text: "draft".to_owned(),
            selection: None,
            composing: None,
        };
        let waft = waft_editing_value(&value);
        assert!(!waft.selection.is_valid());
        assert_eq!(waft.composing, WaftRange::EMPTY);
        assert_same(&program_editing_value(&waft), &value);
    }

    #[test]
    fn a_caret_is_a_selection_of_nothing() {
        let caret = TextSelection {
            base_offset: 2,
            extent_offset: 2,
            affinity: TextAffinity::Downstream,
            is_directional: false,
        };
        let value = TextEditingValue {
            text: "hi".to_owned(),
            selection: Some(caret),
            composing: None,
        };
        assert!(waft_editing_value(&value).selection.is_collapsed());
        assert_same(&program_editing_value(&waft_editing_value(&value)), &value);
    }

    #[test]
    fn a_configuration_carries_every_hint_and_the_view_it_is_for() {
        let configuration = TextInputConfiguration {
            input_type: TextInputType::Number(NumberFormat {
                signed: true,
                decimal: false,
            }),
            input_action: TextInputAction::Search,
            obscure_text: true,
            autocorrect: false,
            enable_suggestions: false,
            text_capitalization: TextCapitalization::Words,
            read_only: true,
        };
        let waft = waft_configuration(&configuration, ViewId(7));
        assert_eq!(waft.view_id, Some(7));
        assert_eq!(
            waft.input_type,
            WaftInputType::number_with_options()
                .signed(true)
                .decimal(false)
        );
        assert_eq!(waft.input_action, WaftAction::Search);
        assert!(waft.obscure_text && waft.read_only);
        assert!(!waft.autocorrect && !waft.enable_suggestions);
        assert_eq!(waft.text_capitalization, WaftCapitalization::Words);
    }

    #[test]
    fn a_multiline_field_asks_for_a_new_line() {
        let configuration = TextInputConfiguration {
            input_type: TextInputType::Multiline,
            input_action: TextInputAction::Newline,
            obscure_text: false,
            autocorrect: true,
            enable_suggestions: true,
            text_capitalization: TextCapitalization::Sentences,
            read_only: false,
        };
        let waft = waft_configuration(&configuration, ViewId(0));
        assert_eq!(waft.input_type, WaftInputType::MULTILINE);
        assert_eq!(waft.input_action, WaftAction::Newline);
    }

    #[test]
    fn every_action_reads_back_as_itself() {
        let actions = [
            TextInputAction::None,
            TextInputAction::Unspecified,
            TextInputAction::Done,
            TextInputAction::Go,
            TextInputAction::Search,
            TextInputAction::Send,
            TextInputAction::Next,
            TextInputAction::Previous,
            TextInputAction::ContinueAction,
            TextInputAction::Join,
            TextInputAction::Route,
            TextInputAction::EmergencyCall,
            TextInputAction::Newline,
        ];
        for action in actions {
            assert_eq!(program_action(waft_action(action)), action);
        }
    }
}
