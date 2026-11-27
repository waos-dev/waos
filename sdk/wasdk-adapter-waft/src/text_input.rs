//! A view's text input: the platform's, as Waft's [`TextInputHost`].
//!
//! Flutter's model on both sides: the framework keeps the text and hands the platform every
//! change, and the platform's input methods edit a copy and send each edit back as the whole new
//! value. The framework does not echo that edit back, so this side keeps the latest value
//! either side has, and a key the framework did not handle types into it ([`typing`]). Every
//! change of the text, from either side or typed here, must pass through that value: a key
//! typed into a stale one would undo the input method's last edit.
//!
//! The framework places the caret and the text being composed in the text's own coordinates,
//! and says separately how those map into the view's logical pixels, Flutter's
//! `setEditableSizeAndTransform`; the platform takes both rectangles in the view's physical
//! pixels.
//!
//! [`typing`]: crate::typing

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use waft_embedder::{
    KeyData, Matrix4, Offset, Rect, Size, TextEditingValue, TextInputAction,
    TextInputConfiguration, TextInputHost, ViewMetrics, transform3,
};
use wasdk::views::{self, PixelRect};

use crate::text_editing::{platform_configuration, platform_editing_value};
use crate::typing::{ActiveTextInput, TextInputKeyEffect, inserted};

pub(crate) struct WasdkTextInput {
    platform_input: views::TextInput,
    /// The view's metrics, shared with it: its device pixel ratio scales the rectangles.
    view_metrics: Rc<Cell<ViewMetrics>>,
    /// The current client's, from `set_client` to `clear_client`, or until the platform ends
    /// editing.
    active: Cell<Option<ActiveTextInput>>,
    /// What the framework set last, or the input method's last edit, or the last key typed
    /// here, whichever came last.
    latest_value: RefCell<TextEditingValue>,
    /// From the text's coordinates to the view's logical pixels.
    client_transform: Cell<Matrix4>,
    /// Whether the program's host sends the view Edit's commands: where it has a menu bar.
    host_edit_commands: bool,
}

/// What typing one key did, for the framework to be told: the value after it, and the action
/// it asked for.
#[derive(Default)]
pub(crate) struct TypedKey {
    pub(crate) edited: Option<TextEditingValue>,
    pub(crate) action: Option<TextInputAction>,
}

impl WasdkTextInput {
    /// The view's text input, scaling its rectangles by `view_metrics`, whose host sends Edit's
    /// commands as `host_edit_commands` says.
    pub(crate) fn new(
        platform_input: views::TextInput,
        view_metrics: Rc<Cell<ViewMetrics>>,
        host_edit_commands: bool,
    ) -> WasdkTextInput {
        WasdkTextInput {
            platform_input,
            view_metrics,
            active: Cell::new(None),
            latest_value: RefCell::new(TextEditingValue::EMPTY),
            client_transform: Cell::new(Matrix4::IDENTITY),
            host_edit_commands,
        }
    }

    /// Types `key`, which the framework did not handle, while editing: its character goes in
    /// at the selection, and Enter acts. The platform is told the new value here.
    pub(crate) fn type_key(&self, key: &KeyData, chord_held: bool) -> TypedKey {
        let Some(active) = self.active.get() else {
            return TypedKey::default();
        };
        match active.key_effect(key, chord_held) {
            TextInputKeyEffect::None => TypedKey::default(),
            TextInputKeyEffect::Insert(text) => TypedKey {
                edited: Some(self.insert(&text)),
                action: None,
            },
            TextInputKeyEffect::Enter { insert, action } => TypedKey {
                edited: insert.map(|text| self.insert(&text)),
                action: Some(action),
            },
        }
    }

    /// An input method's edit, which keys type into from now on.
    pub(crate) fn platform_edited(&self, value: &TextEditingValue) {
        *self.latest_value.borrow_mut() = value.clone();
    }

    /// The platform ended editing: keys type nothing until the framework starts again.
    pub(crate) fn platform_ended_editing(&self) {
        self.active.set(None);
    }

    /// Puts `text` in at the selection, and tells the platform the value after it.
    fn insert(&self, text: &str) -> TextEditingValue {
        let value = inserted(&self.latest_value.borrow(), text);
        self.set_editing_state(&value);
        value
    }

    fn view_pixel_rect(&self, rect: Rect) -> PixelRect {
        view_pixel_rect(
            rect,
            self.client_transform.get(),
            self.view_metrics.get().device_pixel_ratio,
        )
    }
}

/// Each of Flutter's text input calls is the platform's call of the same name.
impl TextInputHost for WasdkTextInput {
    fn set_client(&self, configuration: &TextInputConfiguration) {
        self.active.set(Some(ActiveTextInput::new(configuration)));
        self.platform_input
            .set_client(platform_configuration(configuration));
    }

    fn show(&self) {
        self.platform_input.show();
    }

    fn update_config(&self, configuration: &TextInputConfiguration) {
        self.active.set(Some(ActiveTextInput::new(configuration)));
        self.platform_input
            .update_config(platform_configuration(configuration));
    }

    fn clear_client(&self) {
        self.active.set(None);
        self.platform_input.clear_client();
    }

    fn hide(&self) {
        self.platform_input.hide();
    }

    fn set_editing_state(&self, value: &TextEditingValue) {
        *self.latest_value.borrow_mut() = value.clone();
        self.platform_input
            .set_editing_state(&platform_editing_value(value));
    }

    fn set_composing_rect(&self, rect: Rect) {
        self.platform_input
            .set_composing_rect(self.view_pixel_rect(rect));
    }

    fn set_caret_rect(&self, rect: Rect) {
        self.platform_input
            .set_caret_rect(self.view_pixel_rect(rect));
    }

    fn set_client_geometry(&self, _size: Size, transform: &Matrix4) {
        self.client_transform.set(*transform);
    }

    /// No: a key the program does not handle is the program's to type (the WIT's `views`), so
    /// the platform turns none into an edit, and the framework's own key bindings take
    /// Backspace, Delete and the arrows.
    fn handles_editing_keys(&self) -> bool {
        false
    }

    /// Where the program has a menu bar, its host sends a view Edit's commands as events: a text
    /// field leaves ⌘C, ⌘X and ⌘V unhandled, so the key's reply says so, and acts on the view's
    /// `copy`, `cut` and `paste` events. Elsewhere, as on Windows and Linux, the field handles them
    /// itself, as every app there does.
    fn provides_edit_commands(&self) -> bool {
        self.host_edit_commands
    }
}

/// `rect`, in the text's coordinates, in the view's physical pixels: the bounds of its corners
/// under `transform`, which lands in logical pixels, scaled by `device_pixel_ratio`.
fn view_pixel_rect(rect: Rect, transform: Matrix4, device_pixel_ratio: f64) -> PixelRect {
    let logical = transformed_bounds(rect, transform);
    PixelRect {
        x: logical.left * device_pixel_ratio,
        y: logical.top * device_pixel_ratio,
        width: logical.width() * device_pixel_ratio,
        height: logical.height() * device_pixel_ratio,
    }
}

/// The smallest rectangle around `rect`'s corners under `transform`, as Flutter's macOS text
/// input plugin applies the editable's transform to a rectangle.
fn transformed_bounds(rect: Rect, transform: Matrix4) -> Rect {
    let corner = |x, y| transform3(transform, Offset::new(x, y));
    let diagonal = Rect::from_points(corner(rect.left, rect.top), corner(rect.right, rect.bottom));
    let antidiagonal =
        Rect::from_points(corner(rect.right, rect.top), corner(rect.left, rect.bottom));
    diagonal.expand_to_include(antidiagonal)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARET: Rect = Rect::from_ltwh(10.0, 4.0, 2.0, 16.0);

    /// A rectangle's four numbers, for comparing.
    fn placed(rect: PixelRect) -> [f64; 4] {
        [rect.x, rect.y, rect.width, rect.height]
    }

    #[test]
    fn with_no_transform_a_rectangle_is_only_scaled() {
        assert_eq!(
            placed(view_pixel_rect(CARET, Matrix4::IDENTITY, 2.0)),
            [20.0, 8.0, 4.0, 32.0]
        );
    }

    #[test]
    fn a_field_moved_down_the_view_moves_its_caret() {
        let field_at = Matrix4::translation(100.0, 50.0);
        assert_eq!(
            placed(view_pixel_rect(CARET, field_at, 2.0)),
            [220.0, 108.0, 4.0, 32.0]
        );
    }

    #[test]
    fn a_scaled_field_scales_its_caret() {
        let field_at = Matrix4::translation(100.0, 50.0).then(&Matrix4::scale(2.0, 2.0));
        let caret = view_pixel_rect(CARET, field_at, 1.0);
        assert_eq!(caret.width, 4.0);
        assert_eq!(caret.height, 32.0);
    }

    #[test]
    fn a_turned_field_gives_the_bounds_of_its_caret() {
        let quarter_turn = Matrix4::rotation(std::f32::consts::FRAC_PI_2);
        let caret = view_pixel_rect(CARET, quarter_turn, 1.0);
        assert!((caret.width - 16.0).abs() < 1e-4, "{caret:?}");
        assert!((caret.height - 2.0).abs() < 1e-4, "{caret:?}");
    }
}
