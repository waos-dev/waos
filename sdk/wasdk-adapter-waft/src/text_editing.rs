//! The text input vocabulary between the framework and the platform: editing values, the
//! configuration a text input starts with, and actions.
//!
//! Both sides are Flutter's, and both count offsets in UTF-16 code units, so an offset crosses
//! as it is. What differs is how "none" is said: the framework's selection and composing range
//! are -1 when there are none, the platform's are absent. The platform's configuration carries
//! seven of the framework's fields; the rest are hints no platform here reads.

use waft_embedder::{
    TextAffinity, TextCapitalization, TextEditingValue, TextInputAction, TextInputConfiguration,
    TextInputType, TextRange, TextSelection,
};
use wasdk::views::{self, NumberFormat};

/// The platform's editing value as the framework holds it.
pub(crate) fn framework_editing_value(value: views::TextEditingValue) -> TextEditingValue {
    TextEditingValue {
        text: value.text,
        selection: value
            .selection
            .map_or(TextEditingValue::EMPTY.selection, framework_selection),
        composing: value.composing.map_or(TextRange::EMPTY, |composing| {
            TextRange::new(
                framework_offset(composing.start),
                framework_offset(composing.end),
            )
        }),
    }
}

/// The framework's editing value as the platform takes it.
pub(crate) fn platform_editing_value(value: &TextEditingValue) -> views::TextEditingValue {
    views::TextEditingValue {
        text: value.text.clone(),
        selection: platform_selection(value.selection),
        composing: platform_offset(value.composing.start)
            .zip(platform_offset(value.composing.end))
            .map(|(start, end)| views::TextRange { start, end }),
    }
}

fn framework_selection(selection: views::TextSelection) -> TextSelection {
    TextSelection {
        base_offset: framework_offset(selection.base_offset),
        extent_offset: framework_offset(selection.extent_offset),
        affinity: match selection.affinity {
            views::TextAffinity::Upstream => TextAffinity::Upstream,
            views::TextAffinity::Downstream => TextAffinity::Downstream,
        },
        is_directional: selection.is_directional,
    }
}

/// The selection, or none where the framework has neither a caret nor a selection.
fn platform_selection(selection: TextSelection) -> Option<views::TextSelection> {
    let (base_offset, extent_offset) =
        platform_offset(selection.base_offset).zip(platform_offset(selection.extent_offset))?;
    Some(views::TextSelection {
        base_offset,
        extent_offset,
        affinity: match selection.affinity {
            TextAffinity::Upstream => views::TextAffinity::Upstream,
            TextAffinity::Downstream => views::TextAffinity::Downstream,
        },
        is_directional: selection.is_directional,
    })
}

/// An offset in the framework's signed count, which holds any text shorter than 2^31 code
/// units; a longer one's offsets stop at the largest it can hold.
fn framework_offset(offset: u32) -> i32 {
    i32::try_from(offset).unwrap_or(i32::MAX)
}

/// An offset as the platform counts it; none for the framework's -1.
fn platform_offset(offset: i32) -> Option<u32> {
    u32::try_from(offset).ok()
}

/// The framework's configuration, less what the platform's record leaves out: the view, which
/// the text input already belongs to, autofill, smart dashes and quotes, the action's label,
/// the keyboard's appearance, hint locales, MIME types, the delta model, inline prediction,
/// personalised learning, interactive selection, and a number field's password flag.
pub(crate) fn platform_configuration(
    configuration: &TextInputConfiguration,
) -> views::TextInputConfiguration {
    views::TextInputConfiguration {
        input_type: platform_input_type(configuration.input_type),
        input_action: platform_action(configuration.input_action),
        obscure_text: configuration.obscure_text,
        autocorrect: configuration.autocorrect,
        enable_suggestions: configuration.enable_suggestions,
        text_capitalization: match configuration.text_capitalization {
            TextCapitalization::Words => views::TextCapitalization::Words,
            TextCapitalization::Sentences => views::TextCapitalization::Sentences,
            TextCapitalization::Characters => views::TextCapitalization::Characters,
            TextCapitalization::None => views::TextCapitalization::None,
        },
        read_only: configuration.read_only,
    }
}

/// The input type by its index in Flutter's `TextInputType.values`; an index Flutter has no
/// name for is plain text, as its engines read one.
fn platform_input_type(input_type: TextInputType) -> views::TextInputType {
    match input_type.index {
        1 => views::TextInputType::Multiline,
        2 => views::TextInputType::Number(NumberFormat {
            signed: input_type.signed.unwrap_or(false),
            decimal: input_type.decimal.unwrap_or(false),
        }),
        3 => views::TextInputType::Phone,
        4 => views::TextInputType::Datetime,
        5 => views::TextInputType::EmailAddress,
        6 => views::TextInputType::Url,
        7 => views::TextInputType::VisiblePassword,
        8 => views::TextInputType::Name,
        9 => views::TextInputType::StreetAddress,
        10 => views::TextInputType::None,
        11 => views::TextInputType::WebSearch,
        12 => views::TextInputType::Twitter,
        _ => views::TextInputType::Text,
    }
}

fn platform_action(action: TextInputAction) -> views::TextInputAction {
    match action {
        TextInputAction::None => views::TextInputAction::None,
        TextInputAction::Unspecified => views::TextInputAction::Unspecified,
        TextInputAction::Done => views::TextInputAction::Done,
        TextInputAction::Go => views::TextInputAction::Go,
        TextInputAction::Search => views::TextInputAction::Search,
        TextInputAction::Send => views::TextInputAction::Send,
        TextInputAction::Next => views::TextInputAction::Next,
        TextInputAction::Previous => views::TextInputAction::Previous,
        TextInputAction::ContinueAction => views::TextInputAction::ContinueAction,
        TextInputAction::Join => views::TextInputAction::Join,
        TextInputAction::Route => views::TextInputAction::Route,
        TextInputAction::EmergencyCall => views::TextInputAction::EmergencyCall,
        TextInputAction::Newline => views::TextInputAction::Newline,
    }
}

/// The action the user asked for on the platform, as the framework performs it.
pub(crate) fn framework_action(action: views::TextInputAction) -> TextInputAction {
    match action {
        views::TextInputAction::None => TextInputAction::None,
        views::TextInputAction::Unspecified => TextInputAction::Unspecified,
        views::TextInputAction::Done => TextInputAction::Done,
        views::TextInputAction::Go => TextInputAction::Go,
        views::TextInputAction::Search => TextInputAction::Search,
        views::TextInputAction::Send => TextInputAction::Send,
        views::TextInputAction::Next => TextInputAction::Next,
        views::TextInputAction::Previous => TextInputAction::Previous,
        views::TextInputAction::ContinueAction => TextInputAction::ContinueAction,
        views::TextInputAction::Join => TextInputAction::Join,
        views::TextInputAction::Route => TextInputAction::Route,
        views::TextInputAction::EmergencyCall => TextInputAction::EmergencyCall,
        views::TextInputAction::Newline => TextInputAction::Newline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "a😀b" is four UTF-16 code units: the emoji is two. The caret after it is at 3.
    fn after_the_emoji() -> views::TextEditingValue {
        views::TextEditingValue {
            text: String::from("a😀b"),
            selection: Some(views::TextSelection {
                base_offset: 1,
                extent_offset: 3,
                affinity: views::TextAffinity::Upstream,
                is_directional: true,
            }),
            composing: Some(views::TextRange { start: 1, end: 3 }),
        }
    }

    #[test]
    fn offsets_cross_as_utf16_code_units_both_ways() {
        let framework = framework_editing_value(after_the_emoji());
        assert_eq!(framework.text, "a😀b");
        assert_eq!(framework.selection.base_offset, 1);
        assert_eq!(framework.selection.extent_offset, 3);
        assert_eq!(framework.selection.affinity, TextAffinity::Upstream);
        assert!(framework.selection.is_directional);
        assert_eq!(framework.composing, TextRange::new(1, 3));
        assert_eq!(framework.composing.text_inside(&framework.text), "😀");

        let platform = platform_editing_value(&framework);
        assert_eq!(platform.text, "a😀b");
        let selection = platform.selection.expect("a selection");
        assert_eq!((selection.base_offset, selection.extent_offset), (1, 3));
        assert_eq!(selection.affinity, views::TextAffinity::Upstream);
        assert!(selection.is_directional);
        let composing = platform.composing.expect("a composing range");
        assert_eq!((composing.start, composing.end), (1, 3));
    }

    #[test]
    fn no_selection_and_nothing_composing_are_absent_on_the_platform() {
        let platform = platform_editing_value(&TextEditingValue::EMPTY);
        assert!(platform.selection.is_none());
        assert!(platform.composing.is_none());

        let framework = framework_editing_value(views::TextEditingValue {
            text: String::new(),
            selection: None,
            composing: None,
        });
        assert_eq!(framework, TextEditingValue::EMPTY);
    }

    #[test]
    fn a_caret_is_a_collapsed_selection_on_both_sides() {
        let caret = TextEditingValue::new()
            .text("ab")
            .selection(TextSelection::collapsed(2, TextAffinity::Downstream));
        let platform = platform_editing_value(&caret);
        let selection = platform.selection.as_ref().expect("a caret");
        assert_eq!((selection.base_offset, selection.extent_offset), (2, 2));
        assert!(platform.composing.is_none());
        assert_eq!(framework_editing_value(platform), caret);
    }

    #[test]
    fn every_input_type_keeps_its_name() {
        use views::TextInputType as Platform;
        let named = |framework: TextInputType| platform_input_type(framework);
        assert!(matches!(named(TextInputType::TEXT), Platform::Text));
        assert!(matches!(
            named(TextInputType::MULTILINE),
            Platform::Multiline
        ));
        assert!(matches!(named(TextInputType::PHONE), Platform::Phone));
        assert!(matches!(named(TextInputType::DATETIME), Platform::Datetime));
        assert!(matches!(
            named(TextInputType::EMAIL_ADDRESS),
            Platform::EmailAddress
        ));
        assert!(matches!(named(TextInputType::URL), Platform::Url));
        assert!(matches!(
            named(TextInputType::VISIBLE_PASSWORD),
            Platform::VisiblePassword
        ));
        assert!(matches!(named(TextInputType::NAME), Platform::Name));
        assert!(matches!(
            named(TextInputType::STREET_ADDRESS),
            Platform::StreetAddress
        ));
        assert!(matches!(named(TextInputType::NONE), Platform::None));
        assert!(matches!(
            named(TextInputType::WEB_SEARCH),
            Platform::WebSearch
        ));
        assert!(matches!(named(TextInputType::TWITTER), Platform::Twitter));
    }

    #[test]
    fn a_number_keeps_its_sign_and_decimal_point() {
        let signed_decimal = TextInputType::number_with_options()
            .signed(true)
            .decimal(true);
        assert!(matches!(
            platform_input_type(signed_decimal),
            views::TextInputType::Number(NumberFormat {
                signed: true,
                decimal: true,
            })
        ));
        assert!(matches!(
            platform_input_type(TextInputType::NUMBER),
            views::TextInputType::Number(NumberFormat {
                signed: false,
                decimal: false,
            })
        ));
    }

    #[test]
    fn every_action_goes_out_and_comes_back_the_same() {
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
            assert_eq!(framework_action(platform_action(action)), action);
        }
    }

    #[test]
    fn a_configuration_keeps_the_fields_the_platform_has() {
        let configuration = TextInputConfiguration::builder()
            .input_type(TextInputType::MULTILINE)
            .input_action(TextInputAction::Newline)
            .obscure_text(true)
            .autocorrect(false)
            .enable_suggestions(false)
            .text_capitalization(TextCapitalization::Sentences)
            .read_only(true)
            .build();
        let platform = platform_configuration(&configuration);
        assert!(matches!(
            platform.input_type,
            views::TextInputType::Multiline
        ));
        assert_eq!(platform.input_action, views::TextInputAction::Newline);
        assert!(platform.obscure_text);
        assert!(!platform.autocorrect);
        assert!(!platform.enable_suggestions);
        assert_eq!(
            platform.text_capitalization,
            views::TextCapitalization::Sentences
        );
        assert!(platform.read_only);
    }
}
