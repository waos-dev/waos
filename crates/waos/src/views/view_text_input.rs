//! A view's text input, as the program's calls reach the shell's input methods and what they do
//! reaches the program: the shell's side, which every call of the program's goes on to, and the
//! engine's copy of what is edited, whether the program is editing and the value it set or an
//! input method changed last, which a tool typing into the view commits its text into (the
//! design's decision 60).
//!
//! The invariant a later editor will break: every change of the text passes through the copy, the
//! program's and the input methods', each of which hands over the whole value, so the copy is what
//! the input methods edit now.

use std::sync::{Arc, Mutex};

use crate::bindings::wasdk::platform::views::{TextEditingValue, TextInputConfiguration};
use crate::shell::ShellTextInput;
use crate::sync::locked;

/// One view's text input. Every clone is the same.
#[derive(Clone)]
pub(crate) struct ViewTextInput(Arc<TextInputState>);

struct TextInputState {
    shell: Arc<dyn ShellTextInput>,
    edited: Mutex<Edited>,
}

/// What is edited, as the engine keeps it.
struct Edited {
    /// From the program's `start` to its `stop`, or until the platform ends editing.
    editing: bool,
    value: TextEditingValue,
}

impl ViewTextInput {
    pub(crate) fn new(shell: Arc<dyn ShellTextInput>) -> ViewTextInput {
        ViewTextInput(Arc::new(TextInputState {
            shell,
            edited: Mutex::new(Edited {
                editing: false,
                value: TextEditingValue {
                    text: String::new(),
                    selection: None,
                    composing: None,
                },
            }),
        }))
    }

    /// The shell's side, which the program's calls that change nothing kept here go to.
    pub(crate) fn shell(&self) -> &dyn ShellTextInput {
        self.0.shell.as_ref()
    }

    /// The program starts editing with `configuration`.
    pub(crate) fn start(&self, configuration: TextInputConfiguration) {
        locked(&self.0.edited).editing = true;
        self.0.shell.start(configuration);
    }

    /// The program holds `value` now, which the input methods edit from here on.
    pub(crate) fn set_editing_state(&self, value: TextEditingValue) {
        locked(&self.0.edited).value = value.clone();
        self.0.shell.set_editing_state(value);
    }

    /// The program stops editing.
    pub(crate) fn stop(&self) {
        locked(&self.0.edited).editing = false;
        self.0.shell.stop();
    }

    /// An input method changed the text: `value` is the whole of it after the change.
    pub(crate) fn edited(&self, value: &TextEditingValue) {
        locked(&self.0.edited).value = value.clone();
    }

    /// The platform ended editing.
    pub(crate) fn closed(&self) {
        locked(&self.0.edited).editing = false;
    }

    /// What is edited now: the value the program set or an input method changed last; none while
    /// the program is not editing.
    pub(crate) fn editing_value(&self) -> Option<TextEditingValue> {
        let edited = locked(&self.0.edited);
        edited.editing.then(|| edited.value.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::bindings::wasdk::platform::views::{
        PixelRect, TextAffinity, TextCapitalization, TextInputAction, TextInputType, TextSelection,
    };

    use super::*;

    /// A shell's text input that keeps the values it was handed.
    #[derive(Default)]
    struct Kept(Mutex<Vec<String>>);

    impl ShellTextInput for Kept {
        fn start(&self, _configuration: TextInputConfiguration) {}
        fn set_editing_state(&self, value: TextEditingValue) {
            locked(&self.0).push(value.text);
        }
        fn set_caret_rect(&self, _rect: PixelRect) {}
        fn set_composing_rect(&self, _rect: PixelRect) {}
        fn show(&self) {}
        fn hide(&self) {}
        fn stop(&self) {}
    }

    fn configuration() -> TextInputConfiguration {
        TextInputConfiguration {
            input_type: TextInputType::Text,
            input_action: TextInputAction::Done,
            obscure_text: false,
            autocorrect: true,
            enable_suggestions: true,
            text_capitalization: TextCapitalization::None,
            read_only: false,
        }
    }

    fn value(text: &str) -> TextEditingValue {
        let end = text.encode_utf16().count() as u32;
        TextEditingValue {
            text: text.to_owned(),
            selection: Some(TextSelection {
                base_offset: end,
                extent_offset: end,
                affinity: TextAffinity::Downstream,
                is_directional: false,
            }),
            composing: None,
        }
    }

    #[test]
    fn nothing_is_edited_until_the_program_starts_and_once_it_stops() {
        let input = ViewTextInput::new(Arc::new(Kept::default()));
        input.set_editing_state(value("milk"));
        assert!(input.editing_value().is_none(), "not started");
        input.start(configuration());
        assert_eq!(input.editing_value().unwrap().text, "milk");
        input.stop();
        assert!(input.editing_value().is_none());
    }

    #[test]
    fn the_value_is_whichever_side_changed_it_last_and_the_programs_reaches_the_shell() {
        let shell = Arc::new(Kept::default());
        let input = ViewTextInput::new(Arc::clone(&shell) as Arc<dyn ShellTextInput>);
        input.start(configuration());
        input.set_editing_state(value("milk"));
        input.edited(&value("milk, eggs"));
        assert_eq!(input.editing_value().unwrap().text, "milk, eggs");
        input.set_editing_state(value("bread"));
        assert_eq!(input.editing_value().unwrap().text, "bread");
        assert_eq!(*locked(&shell.0), ["milk", "bread"]);
    }

    #[test]
    fn the_platform_ending_editing_ends_it() {
        let input = ViewTextInput::new(Arc::new(Kept::default()));
        input.start(configuration());
        input.closed();
        assert!(input.editing_value().is_none());
    }
}
