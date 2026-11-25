//! A view as the window other libraries take: `raw-window-handle`'s traits on [`View`], and the
//! view a window handle points at.
//!
//! A renderer makes its surface from "a window", and a dialog names its parent by one, which in
//! Rust is anything that gives a `raw-window-handle`. That crate lists the systems it knows and
//! has no entry for WaOS, so a view's handle is the browser canvas's entry, a
//! `WebCanvasWindowHandle`, its pointer at the program's [`View`]: a component has no browser
//! canvas for one to point at, and a view's id is a `u64`, which the numbered `WebWindowHandle`
//! cannot carry.
//!
//! The invariant: the pointer is followed only in [`view_of_window_handle`], while the window the
//! handle was taken from lends it, and only for the view's id. What is answered is a handle of
//! the caller's own to that view, so nothing after needs the pointer.

use std::ptr::NonNull;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    WebCanvasWindowHandle, WindowHandle,
};

use crate::views::{self, View};

impl HasWindowHandle for View {
    /// A `WebCanvas` handle pointing at this view.
    #[allow(unsafe_code)]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = WebCanvasWindowHandle::new(NonNull::from(self).cast());
        // SAFETY: the handle points at `self`, which the handle borrows for as long as it lives.
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for View {
    /// The web's display, which carries nothing: the view is reached through its window handle.
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::web())
    }
}

/// The view `handle` points at, as a handle of the caller's own to it, kept for as long as the
/// caller likes; none for a handle that is not a view's, or whose view has gone.
///
/// What a library handed a window calls to reach WaOS's view: a renderer's surface, a dialog's
/// parent. The window may be a [`View`] or another library's window over one, which answers its
/// view's handle as its own.
#[allow(unsafe_code)]
pub fn view_of_window_handle(handle: WindowHandle<'_>) -> Option<View> {
    let RawWindowHandle::WebCanvas(canvas) = handle.as_raw() else {
        return None;
    };
    // SAFETY: `handle` borrows its window for as long as it lives, so what it points at is
    // there, and a `WebCanvas` handle in a component is only ever a view's, pointing at it.
    let id = unsafe { canvas.obj.cast::<View>().as_ref() }.id();
    views::views().into_iter().find(|view| view.id() == id)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use raw_window_handle::{AppKitWindowHandle, WebWindowHandle};

    use super::*;

    /// A handle borrowed from nothing: what a test hands `view_of_window_handle`, which follows
    /// no pointer of a handle that is not a `WebCanvas`.
    #[allow(unsafe_code)]
    fn lent(handle: RawWindowHandle) -> WindowHandle<'static> {
        // SAFETY: the tests give handles whose pointer is never followed.
        unsafe { WindowHandle::borrow_raw(handle) }
    }

    /// `instance.create_surface(view.clone())` takes it, and Bevy's `RawHandleWrapper` its two
    /// handles.
    #[test]
    fn a_view_is_a_window_shared_or_lent() {
        fn window<T: HasWindowHandle + HasDisplayHandle>() {}
        window::<View>();
        window::<&View>();
        window::<Arc<View>>();
    }

    #[test]
    fn a_numbered_web_handle_is_not_a_view() {
        assert!(view_of_window_handle(lent(WebWindowHandle::new(1).into())).is_none());
    }

    #[test]
    fn a_native_window_is_not_a_view() {
        let window = NonNull::<u8>::dangling().cast();
        let handle = AppKitWindowHandle::new(window);
        assert!(view_of_window_handle(lent(handle.into())).is_none());
    }
}
