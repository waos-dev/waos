//! The shell's side of a view's text input, and of its cursor and focus, as the engine holds them
//! ([`ViewCalls`]), and of the view's window ([`WindowCalls`]): every call, from whichever thread
//! makes it, is handed on as the [`ViewCall`] it is, to the view's queue on its window's main
//! thread in this process, or to the process its window is in, a window's among its view's in the
//! order they were made. What comes of a call comes back through the view's link.

use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waos::bindings::wasdk::platform::screens::ScreenId;
use waos::bindings::wasdk::platform::views::{PixelRect, TextEditingValue, TextInputConfiguration};
use waos::bindings::wasdk::platform::windows::{Rect, Size};
use waos::{ShellTextInput, ShellWindow, ShellWindowSizeLimits};
use waos_window_messages::{TextInputCall, ViewCall, WindowCall};

/// Where one view's calls go, in the order they were made.
#[derive(Clone)]
pub struct ViewCalls(Arc<dyn Fn(ViewCall) + Send + Sync>);

/// A view's queue: what its calls are sent on, and where they arrive on its window's main thread.
pub(crate) fn view_call_queue() -> (UnboundedSender<ViewCall>, UnboundedReceiver<ViewCall>) {
    mpsc::unbounded()
}

impl ViewCalls {
    /// The calls `send` hands on.
    pub fn new(send: impl Fn(ViewCall) + Send + Sync + 'static) -> ViewCalls {
        ViewCalls(Arc::new(send))
    }

    /// The calls queued on `queue`. Once the view is gone from its window nobody carries them
    /// out, and nobody needs to.
    pub(crate) fn queued(queue: UnboundedSender<ViewCall>) -> ViewCalls {
        ViewCalls::new(move |call| {
            let _ = queue.unbounded_send(call);
        })
    }

    pub fn send(&self, call: ViewCall) {
        (self.0)(call);
    }

    fn text_input(&self, call: TextInputCall) {
        self.send(ViewCall::TextInput(call));
    }
}

/// Where the calls on one view's window go: among its view's calls, in the order they were made;
/// and whether the window has size limits, which the window said as it opened.
pub struct WindowCalls {
    calls: ViewCalls,
    size_limits: bool,
}

impl WindowCalls {
    /// The calls on the window of the view whose calls go to `calls`, a window with size limits
    /// where `size_limits` says so.
    pub fn new(calls: ViewCalls, size_limits: bool) -> WindowCalls {
        WindowCalls { calls, size_limits }
    }

    fn send(&self, call: WindowCall) {
        self.calls.send(ViewCall::Window(call));
    }
}

impl ShellTextInput for ViewCalls {
    fn set_client(&self, configuration: TextInputConfiguration) {
        self.text_input(TextInputCall::SetClient(configuration));
    }

    fn show(&self) {
        self.text_input(TextInputCall::Show);
    }

    fn update_config(&self, configuration: TextInputConfiguration) {
        self.text_input(TextInputCall::UpdateConfig(configuration));
    }

    fn set_editing_state(&self, value: TextEditingValue) {
        self.text_input(TextInputCall::SetEditingState(value));
    }

    fn set_caret_rect(&self, rect: PixelRect) {
        self.text_input(TextInputCall::SetCaretRect(rect));
    }

    fn set_composing_rect(&self, rect: PixelRect) {
        self.text_input(TextInputCall::SetComposingRect(rect));
    }

    fn clear_client(&self) {
        self.text_input(TextInputCall::ClearClient);
    }

    fn hide(&self) {
        self.text_input(TextInputCall::Hide);
    }
}

impl ShellWindow for WindowCalls {
    fn set_frame(&self, frame: Rect, animate: Option<Duration>) {
        self.send(WindowCall::SetFrame(frame, animate));
    }

    fn set_title(&self, title: &str) {
        self.send(WindowCall::SetTitle(title.to_owned()));
    }

    fn show(&self) {
        self.send(WindowCall::Show);
    }

    fn hide(&self) {
        self.send(WindowCall::Hide);
    }

    fn close(&self) {
        self.send(WindowCall::Close);
    }

    fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
        self.size_limits
            .then_some(self as &dyn ShellWindowSizeLimits)
    }

    fn drag(&self) {
        self.send(WindowCall::Drag);
    }

    fn set_maximized(&self, maximized: bool) {
        self.send(WindowCall::SetMaximized(maximized));
    }

    fn set_minimized(&self, minimized: bool) {
        self.send(WindowCall::SetMinimized(minimized));
    }

    fn set_fullscreen(&self, fullscreen: bool, screen: Option<ScreenId>) {
        self.send(WindowCall::SetFullscreen(fullscreen, screen));
    }
}

impl ShellWindowSizeLimits for WindowCalls {
    fn set_min_size(&self, size: Option<Size>) {
        self.send(WindowCall::SetMinSize(size));
    }

    fn set_max_size(&self, size: Option<Size>) {
        self.send(WindowCall::SetMaxSize(size));
    }
}

#[cfg(test)]
mod tests {
    use waos::bindings::wasdk::platform::views::Cursor;

    use super::*;

    fn received(arriving: &mut UnboundedReceiver<ViewCall>) -> Vec<String> {
        std::iter::from_fn(|| arriving.try_recv().ok())
            .map(|call| format!("{call:?}"))
            .collect()
    }

    fn queue() -> (ViewCalls, UnboundedReceiver<ViewCall>) {
        let (queue, arriving) = view_call_queue();
        (ViewCalls::queued(queue), arriving)
    }

    #[test]
    fn a_views_calls_arrive_in_the_order_they_were_made() {
        let (calls, mut arriving) = queue();
        let window = WindowCalls::new(calls.clone(), true);
        let caret = PixelRect {
            x: 4.0,
            y: 8.0,
            width: 2.0,
            height: 30.0,
        };
        calls.send(ViewCall::SetCursor(Cursor::Text));
        ShellTextInput::set_caret_rect(&calls, caret);
        window.set_title("Panel");
        ShellTextInput::hide(&calls);
        window.drag();
        window.set_maximized(true);
        window.set_minimized(false);
        window.close();
        assert_eq!(
            received(&mut arriving),
            [
                "SetCursor(Cursor::Text)",
                "TextInput(SetCaretRect(PixelRect { x: 4.0, y: 8.0, width: 2.0, height: 30.0 }))",
                "Window(SetTitle(\"Panel\"))",
                "TextInput(Hide)",
                "Window(Drag)",
                "Window(SetMaximized(true))",
                "Window(SetMinimized(false))",
                "Window(Close)",
            ]
        );
    }

    #[test]
    fn a_window_has_size_limits_where_it_said_it_has_them_and_they_go_with_its_calls() {
        let (calls, mut arriving) = queue();
        assert!(
            WindowCalls::new(calls.clone(), false)
                .size_limits()
                .is_none()
        );

        let window = WindowCalls::new(calls, true);
        let limits = window.size_limits().expect("it has size limits");
        limits.set_min_size(Some(Size {
            width: 320.0,
            height: 240.0,
        }));
        limits.set_max_size(None);
        assert_eq!(
            received(&mut arriving),
            [
                "Window(SetMinSize(Some(Size { width: 320.0, height: 240.0 })))",
                "Window(SetMaxSize(None))",
            ]
        );
    }

    #[test]
    fn a_call_after_the_main_thread_let_go_is_dropped() {
        let (calls, arriving) = queue();
        drop(arriving);
        WindowCalls::new(calls, true).show();
    }

    /// The engine calls them from the program's thread.
    #[test]
    fn the_calls_cross_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<ViewCalls>();
        assert_shared::<WindowCalls>();
    }
}
