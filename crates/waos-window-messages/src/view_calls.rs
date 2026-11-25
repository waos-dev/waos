//! What the engine asks of a program's view that only the main thread of its window's process can
//! carry out: the cursor and the focus, which the view's canvas carries out; whether the program
//! handles Back, which whatever shows the view reads; the style of the system's bars over it,
//! which whatever shows the view puts over its canvas; its text input, which the canvas's text input
//! does; and its window, which Waft's window does. Each is a
//! [`ViewCall`], the value of one call of the engine's shell traits on the view, which reaches the
//! window's main thread on one queue per view, in the order the calls were made, in WaOS's process
//! or in a program's window process alike.

use std::fmt::{self, Debug};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use waos::bindings::wasdk::platform::screens::ScreenId;
use waos::bindings::wasdk::platform::views::{
    Cursor, OverlayStyle, PixelRect, TextEditingValue, TextInputConfiguration, ViewFocusDirection,
    ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{Rect, Size};

use crate::records::{
    CursorDef, OverlayStyleDef, PixelRectDef, RectDef, TextEditingValueDef,
    TextInputConfigurationDef, ViewFocusDirectionDef, ViewFocusStateDef, option_size,
};

/// One call on a view: a method of the engine's `ShellView` other than `present`, or a call on
/// its text input or its window.
#[derive(Debug, Serialize, Deserialize)]
pub enum ViewCall {
    SetCursor(#[serde(with = "CursorDef")] Cursor),
    RequestFocusChange(
        #[serde(with = "ViewFocusStateDef")] ViewFocusState,
        #[serde(with = "ViewFocusDirectionDef")] ViewFocusDirection,
    ),
    SetHandlesBack(bool),
    TextInput(TextInputCall),
    Window(WindowCall),
    SetOverlayStyle(#[serde(with = "OverlayStyleDef")] OverlayStyle),
}

/// One call on a view's text input: a method of the engine's `ShellTextInput`.
#[derive(Serialize, Deserialize)]
pub enum TextInputCall {
    SetClient(#[serde(with = "TextInputConfigurationDef")] TextInputConfiguration),
    Show,
    UpdateConfig(#[serde(with = "TextInputConfigurationDef")] TextInputConfiguration),
    SetEditingState(#[serde(with = "TextEditingValueDef")] TextEditingValue),
    SetCaretRect(#[serde(with = "PixelRectDef")] PixelRect),
    SetComposingRect(#[serde(with = "PixelRectDef")] PixelRect),
    ClearClient,
    Hide,
}

/// One call on the window around a view: a method of the engine's `ShellWindow`, or of its
/// `ShellWindowSizeLimits`.
#[derive(Debug, Serialize, Deserialize)]
pub enum WindowCall {
    SetFrame(#[serde(with = "RectDef")] Rect, Option<Duration>),
    SetTitle(String),
    Show,
    Hide,
    Close,
    SetMinSize(#[serde(with = "option_size")] Option<Size>),
    SetMaxSize(#[serde(with = "option_size")] Option<Size>),
    Drag,
    SetMaximized(bool),
    SetMinimized(bool),
    /// Full screen or back, and the screen to fill, the window's own where none.
    SetFullscreen(bool, Option<ScreenId>),
}

/// Written without the text, which may be a password: calls are traced.
impl Debug for TextInputCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextInputCall::SetClient(configuration) => {
                f.debug_tuple("SetClient").field(configuration).finish()
            }
            TextInputCall::Show => f.write_str("Show"),
            TextInputCall::UpdateConfig(configuration) => {
                f.debug_tuple("UpdateConfig").field(configuration).finish()
            }
            TextInputCall::SetEditingState(value) => f
                .debug_struct("SetEditingState")
                .field("selection", &value.selection)
                .field("composing", &value.composing)
                .finish_non_exhaustive(),
            TextInputCall::SetCaretRect(rect) => f.debug_tuple("SetCaretRect").field(rect).finish(),
            TextInputCall::SetComposingRect(rect) => {
                f.debug_tuple("SetComposingRect").field(rect).finish()
            }
            TextInputCall::ClearClient => f.write_str("ClearClient"),
            TextInputCall::Hide => f.write_str("Hide"),
        }
    }
}

#[cfg(test)]
mod tests {
    use waos::bindings::wasdk::platform::views::{TextAffinity, TextRange, TextSelection};

    use super::*;

    #[test]
    fn a_traced_editing_state_leaves_the_text_out() {
        let call = TextInputCall::SetEditingState(TextEditingValue {
            text: "hunter2".to_owned(),
            selection: Some(TextSelection {
                base_offset: 7,
                extent_offset: 7,
                affinity: TextAffinity::Downstream,
                is_directional: false,
            }),
            composing: Some(TextRange { start: 0, end: 7 }),
        });
        let traced = format!("{call:?}");
        assert!(!traced.contains("hunter2"), "{traced}");
        assert!(traced.contains("selection"), "{traced}");
    }
}
