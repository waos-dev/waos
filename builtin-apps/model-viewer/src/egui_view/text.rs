//! egui's text fields and the platform's input methods.
//!
//! The two models differ. A view's text input is Flutter's: the program tells the platform the
//! whole text and selection of the field being edited, and an input method answers with the whole
//! new value. egui tells an integration only that a field wants text and where its caret is, and
//! takes an input method's work as steps: text being composed, text committed. So the platform is
//! told an empty field, and each value that comes back is the step itself: what is composed while
//! it carries a composing range, what is committed once it does not, after which the field is
//! told empty again. What an input method does with the text around the caret, a soft keyboard's
//! backspace among it, has nothing to act on.

use egui::{Event, ImeEvent};
use wasdk::views::{
    PixelRect, TextAffinity, TextCapitalization, TextEditingValue, TextInput, TextInputAction,
    TextInputConfiguration, TextInputType, TextSelection, View,
};

/// The view's text input, started while a field of egui's wants text.
pub(super) struct TextSession {
    input: Option<TextInput>,
    editing: bool,
    composing: bool,
}

impl TextSession {
    pub(super) fn new(view: &View) -> TextSession {
        TextSession {
            input: view.text_input(),
            editing: false,
            composing: false,
        }
    }

    /// Starts or stops editing as egui's pass asked, and says where the caret is.
    pub(super) fn follow(
        &mut self,
        wanted: Option<egui::output::IMEOutput>,
        pixels_per_point: f32,
    ) {
        let Some(input) = &self.input else { return };
        match wanted {
            Some(ime) => {
                if !self.editing {
                    input.set_client(configuration());
                    input.show();
                    input.set_editing_state(&empty_value());
                    self.editing = true;
                }
                input.set_caret_rect(pixel_rect(ime.cursor_rect, pixels_per_point));
            }
            None if self.editing => {
                input.clear_client();
                input.hide();
                self.editing = false;
                self.composing = false;
            }
            None => {}
        }
    }

    /// What an input method's new value is to egui: text composed, or text committed.
    pub(super) fn value_changed(&mut self, value: TextEditingValue) -> Option<Event> {
        if value.composing.is_some() {
            self.composing = true;
            return Some(Event::Ime(ImeEvent::Preedit {
                text: value.text,
                active_range_chars: None,
            }));
        }
        let was_composing = std::mem::take(&mut self.composing);
        if value.text.is_empty() {
            return was_composing.then(|| Event::Ime(ImeEvent::Commit(String::new())));
        }
        if let Some(input) = &self.input {
            input.set_editing_state(&empty_value());
        }
        Some(Event::Ime(ImeEvent::Commit(value.text)))
    }

    /// The platform ended editing.
    pub(super) fn closed(&mut self) {
        self.editing = false;
        self.composing = false;
    }
}

fn configuration() -> TextInputConfiguration {
    TextInputConfiguration {
        input_type: TextInputType::Text,
        input_action: TextInputAction::Done,
        obscure_text: false,
        autocorrect: false,
        enable_suggestions: false,
        text_capitalization: TextCapitalization::None,
        read_only: false,
    }
}

fn empty_value() -> TextEditingValue {
    TextEditingValue {
        text: String::new(),
        selection: Some(TextSelection {
            base_offset: 0,
            extent_offset: 0,
            affinity: TextAffinity::Downstream,
            is_directional: false,
        }),
        composing: None,
    }
}

fn pixel_rect(rect: egui::Rect, pixels_per_point: f32) -> PixelRect {
    PixelRect {
        x: f64::from(rect.min.x * pixels_per_point),
        y: f64::from(rect.min.y * pixels_per_point),
        width: f64::from(rect.width() * pixels_per_point),
        height: f64::from(rect.height() * pixels_per_point),
    }
}
