//! A program view's text input, as a client of Waft's: the program's calls open, feed and close
//! Waft's text input connection, and what the input methods do with the text goes back to the
//! program.
//!
//! Waft's host does the input methods' work, as it does for any Waft text field: it applies
//! their edits to the value the program last set and hands the result here. Plain keys never
//! reach it, since the canvas takes every key for the program, which types them itself.
//!
//! The invariant: Waft keeps one connection for the whole application, and a client that
//! attaches takes it over without a word to the one before, so nothing here touches the
//! connection once it is another's.

use waft::ui::{Matrix4, Rect};
use waft::{App, BuildContext, Entity, Handle, OptionRetained, Ref, Size, Subscription};
use waft_services::{
    TextEditingValue as WaftEditingValue, TextInput, TextInputAction as WaftAction,
    TextInputClient, TextInputConnection,
};
use waos::bindings::wasdk::platform::views::{PixelRect, TextEditingValue, TextInputConfiguration};
use waos_window_messages::TextInputCall;

use crate::logic::geometry::logical_rect;
use crate::logic::text_input::{
    program_action, program_editing_value, waft_configuration, waft_editing_value,
};
use crate::state::shown_view::ShownView;

pub(crate) struct ProgramTextInput {
    view: Entity<ShownView>,
    /// Where the view is drawn: the input methods' windows are placed relative to it.
    canvas: BuildContext,
    /// Waft's connection since the program last started editing, until it stopped.
    connection: Option<Ref<TextInputConnection>>,
    /// The value the program last set.
    editing_value: Option<WaftEditingValue>,
    /// Keeps the program's calls coming while the canvas is in the tree.
    program_calls: Option<Subscription>,
}

impl ProgramTextInput {
    /// The text input of `view`, drawn in `canvas`, carrying out the program's calls from now on.
    pub(crate) fn open(
        app: &mut App,
        view: Entity<ShownView>,
        canvas: BuildContext,
    ) -> Ref<ProgramTextInput> {
        let text_input = app.create(ProgramTextInput {
            view: view.clone(),
            canvas,
            connection: None,
            editing_value: None,
            program_calls: None,
        });
        let receiver = text_input.handle();
        let program_calls = app.subscribe(&view, move |app, change: &TextInputCall| {
            receiver.apply(app, change);
        });
        app.get_mut(receiver).program_calls = Some(program_calls);
        text_input
    }

    /// Ends editing and carries out no more of the program's calls: the canvas is gone. The
    /// canvas lets go of the text input after this, which frees it.
    pub(crate) fn close(self: Handle<Self>, app: &mut App) {
        self.clear_client(app);
        app.get_mut(self).program_calls = None;
    }

    fn apply(self: Handle<Self>, app: &mut App, change: &TextInputCall) {
        match change {
            TextInputCall::SetClient(configuration) => self.set_client(app, configuration),
            TextInputCall::Show => self.show(app),
            TextInputCall::UpdateConfig(configuration) => self.update_config(app, configuration),
            TextInputCall::SetEditingState(value) => self.set_editing_state(app, value),
            TextInputCall::SetCaretRect(rect) => self.set_caret_rect(app, *rect),
            TextInputCall::SetComposingRect(rect) => self.set_composing_rect(app, *rect),
            TextInputCall::ClearClient => self.clear_client(app),
            TextInputCall::Hide => self.hide(app),
        }
    }

    /// Takes Waft's text input over for the view's window as a new client: Waft's `attach`, which
    /// is Flutter's `setClient`. The keyboard shows only at the program's `show`.
    fn set_client(self: Handle<Self>, app: &mut App, configuration: &TextInputConfiguration) {
        let configuration = waft_configuration(configuration, self.shown_view(app).waft_view());
        let connection = TextInput::attach(app, self.as_text_input_client(), configuration);
        app.get_mut(self).connection = Some(connection);
    }

    fn update_config(self: Handle<Self>, app: &mut App, configuration: &TextInputConfiguration) {
        let configuration = waft_configuration(configuration, self.shown_view(app).waft_view());
        if let Some(connection) = self.attached_connection(app) {
            connection.update_config(app, configuration);
        }
    }

    fn set_editing_state(self: Handle<Self>, app: &mut App, value: &TextEditingValue) {
        let value = waft_editing_value(value);
        app.get_mut(self).editing_value = Some(value.clone());
        if let Some(connection) = self.attached_connection(app) {
            connection.set_editing_state(app, value);
        }
    }

    fn set_caret_rect(self: Handle<Self>, app: &mut App, rect: PixelRect) {
        let Some(connection) = self.attached_connection(app) else {
            return;
        };
        if let Some(rect) = self.canvas_rect(app, connection, rect) {
            connection.set_caret_rect(app, rect);
        }
    }

    fn set_composing_rect(self: Handle<Self>, app: &mut App, rect: PixelRect) {
        let Some(connection) = self.attached_connection(app) else {
            return;
        };
        if let Some(rect) = self.canvas_rect(app, connection, rect) {
            connection.set_composing_rect(app, rect);
        }
    }

    fn show(self: Handle<Self>, app: &mut App) {
        if let Some(connection) = self.attached_connection(app) {
            connection.show(app);
        }
    }

    /// Closes the connection, Flutter's `clearClient`, which Waft ignores once it is another's.
    /// Waft hides the keyboard after the frame unless another client takes the connection by
    /// then.
    fn clear_client(self: Handle<Self>, app: &mut App) {
        if let Some(connection) = app.get_mut(self).connection.take() {
            connection.close(app);
        }
    }

    /// Hides the input method of the view's window, as a Flutter app sends `TextInput.hide`
    /// itself: Waft's connection has no hide of its own, and keeps the client.
    fn hide(self: Handle<Self>, app: &mut App) {
        let window_view = self.shown_view(app).waft_view();
        if let Some(view) = app.platform().view(window_view)
            && let Some(text_input) = view.host().text_input()
        {
            text_input.hide();
        }
    }

    /// The connection, while it is still this view's.
    fn attached_connection(
        self: Handle<Self>,
        app: &mut App,
    ) -> Option<Handle<TextInputConnection>> {
        let connection = app.get(self).connection.handle()?;
        connection.attached(app).then_some(connection)
    }

    /// `rect` in the canvas's logical pixels, with `connection` told where the canvas is, so the
    /// rectangle lands where the program drew it. None until the canvas has been laid out.
    fn canvas_rect(
        self: Handle<Self>,
        app: &mut App,
        connection: Handle<TextInputConnection>,
        rect: PixelRect,
    ) -> Option<Rect> {
        let ratio = self.shown_view(app).device_pixel_ratio();
        let (size, transform) = self.canvas_place(app)?;
        connection.set_editable_size_and_transform(app, size, transform);
        Some(logical_rect(rect, ratio))
    }

    /// The canvas's size, and the transform from it to its window's.
    fn canvas_place(self: Handle<Self>, app: &App) -> Option<(Size, Matrix4)> {
        let canvas = app.get(self).canvas;
        let size = canvas.size(app)?;
        let transform = canvas.find_render_object(app)?.get_transform_to(app, None);
        Some((size, transform))
    }

    fn shown_view(self: Handle<Self>, app: &App) -> &ShownView {
        app.get(self).view.read(app)
    }
}

/// What the input methods did goes to the program, among its view's events.
impl TextInputClient for ProgramTextInput {
    fn current_text_editing_value(self: Handle<Self>, app: &App) -> Option<WaftEditingValue> {
        app.get(self).editing_value.clone()
    }

    fn update_editing_value(self: Handle<Self>, app: &mut App, value: WaftEditingValue) {
        self.shown_view(app)
            .view()
            .text_input_editing_value(program_editing_value(&value));
    }

    fn perform_action(self: Handle<Self>, app: &mut App, action: WaftAction) {
        self.shown_view(app)
            .view()
            .text_input_action(program_action(action));
    }

    /// The platform ended editing: the connection is no longer this view's.
    fn connection_closed(self: Handle<Self>, app: &mut App) {
        if let Some(connection) = app.get_mut(self).connection.take() {
            connection.connection_closed_received(app);
        }
        self.shown_view(app).view().text_input_closed();
    }
}
