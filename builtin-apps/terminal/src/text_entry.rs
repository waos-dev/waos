//! Text typed into a session through its view's text input: what a key's character, an input
//! method or an on-screen keyboard enters. The terminal keeps no text of its own to edit, so the
//! value it shows the platform is empty; text an input method is composing is shown at the cursor
//! until it is committed, and committed text is typed into the shell and the value emptied again.
//!
//! The invariant a later editor will break: the platform holds a copy of the value, so each time
//! committed text is taken from it, the platform is told the value is empty again; otherwise the
//! next commit arrives with the last one's text before it.

use waft_embedder::{
    Matrix4, Rect, Size, TextAffinity, TextEditingValue, TextInputAction, TextInputConfiguration,
    TextInputType, TextSelection, ViewId,
};
use waft_foundation::{App, Entity, Handle, OptionRetained, Ref};
use waft_services::{TextInput, TextInputClient, TextInputConnection};

use crate::session::{Session, set_composing, type_bytes};

/// A session's side of its view's text input.
pub(crate) struct TextEntry {
    session: Entity<Session>,
    /// The connection to the platform's text input, while the session's view has the keyboard.
    connection: Option<Ref<TextInputConnection>>,
}

impl TextEntry {
    pub(crate) fn new(app: &mut App, session: Entity<Session>) -> Ref<TextEntry> {
        app.create(TextEntry {
            session,
            connection: None,
        })
    }

    /// Starts taking text for the session in `view`, and shows the on-screen keyboard where
    /// there is one.
    pub(crate) fn open(self: Handle<Self>, app: &mut App, view: Option<ViewId>) {
        if let Some(connection) = app.get(self).connection.handle() {
            connection.show(app);
            return;
        }
        let connection = TextInput::attach(app, self.as_text_input_client(), configuration(view));
        connection.set_editing_state(app, empty_value());
        connection.show(app);
        app.get_mut(self).connection = Some(connection);
    }

    /// Stops taking text, and drops what was being composed.
    pub(crate) fn close(self: Handle<Self>, app: &mut App) {
        if let Some(connection) = app.get_mut(self).connection.take() {
            connection.close(app);
        }
        let session = app.get(self).session.clone();
        set_composing(app, &session, "");
    }

    /// Tells the platform where the cursor is, for an input method's window of candidates:
    /// `caret` in the coordinates of the grid, of `size`, which `transform` takes to the view's.
    pub(crate) fn place_caret(
        self: Handle<Self>,
        app: &mut App,
        size: Size,
        transform: Matrix4,
        caret: Rect,
    ) {
        let Some(connection) = app.get(self).connection.handle() else {
            return;
        };
        connection.set_editable_size_and_transform(app, size, transform);
        connection.set_composing_rect(app, caret);
        connection.set_caret_rect(app, caret);
    }
}

/// Text as a terminal takes it: as typed, with nothing corrected, suggested or capitalized.
fn configuration(view: Option<ViewId>) -> TextInputConfiguration {
    let mut configuration = TextInputConfiguration::builder()
        .input_type(TextInputType::VISIBLE_PASSWORD)
        .autocorrect(false)
        .enable_suggestions(false)
        .input_action(TextInputAction::Newline)
        .build();
    configuration.view_id = view.map(|view| view.0 as i32);
    configuration
}

/// No text, the caret at its start.
fn empty_value() -> TextEditingValue {
    TextEditingValue::new().selection(TextSelection::collapsed(0, TextAffinity::Downstream))
}

impl TextInputClient for TextEntry {
    fn current_text_editing_value(self: Handle<Self>, _app: &App) -> Option<TextEditingValue> {
        Some(empty_value())
    }

    /// The platform's edit: text being composed is shown, and committed text is typed.
    fn update_editing_value(self: Handle<Self>, app: &mut App, value: TextEditingValue) {
        let session = app.get(self).session.clone();
        if value.is_composing_range_valid() && !value.composing.is_collapsed() {
            set_composing(app, &session, &value.text);
            return;
        }
        set_composing(app, &session, "");
        if value.text.is_empty() {
            return;
        }
        type_bytes(app, &session, value.text.as_bytes());
        if let Some(connection) = app.get(self).connection.handle() {
            connection.set_editing_state(app, empty_value());
        }
    }

    /// Enter is typed as a key, which reaches the shell before the platform asks for the action.
    fn perform_action(self: Handle<Self>, _app: &mut App, _action: TextInputAction) {}

    fn connection_closed(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).connection = None;
        let session = app.get(self).session.clone();
        set_composing(app, &session, "");
    }
}
