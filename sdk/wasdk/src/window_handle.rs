//! A view as the window other libraries take: `raw-window-handle`'s traits on [`View`], and the
//! view a window handle names.
//!
//! A renderer makes its surface from "a window", and a dialog names its parent by one, which in
//! Rust is anything that gives a `raw-window-handle`. That crate lists the systems it knows and
//! has no entry for WaOS, so a view's handle is the web's numbered one, a `WebWindowHandle`, as a
//! browser canvas is named by the number in its `data-raw-handle`: the number is one this crate
//! gives each view the first time its handle is asked for, kept in a table of the program's views,
//! since a view's id is a `u64`, which the handle's `u32` cannot carry. wgpu's own backend for the
//! web finds its canvas by that number the same way, following no pointer.
//!
//! The invariant: a handle is only ever read as a number, looked up in [`NUMBERED_VIEWS`]; no
//! pointer a handle carries is followed. A number another library put in a `Web` handle can name
//! the wrong view at worst, never memory.

use std::sync::{Mutex, PoisonError};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    WebWindowHandle, WindowHandle,
};

use crate::views::{self, View, ViewId};

/// The views a window handle was asked of, each numbered by its place here plus one, so no view
/// is numbered 0. A view gone keeps its number, which then names none.
static NUMBERED_VIEWS: Mutex<Vec<ViewId>> = Mutex::new(Vec::new());

/// The number `view`'s window handle carries: the one it was given, or the next.
fn number_of(view: ViewId) -> u32 {
    let mut numbered = NUMBERED_VIEWS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let index = match numbered.iter().position(|&numbered| numbered == view) {
        Some(index) => index,
        None => {
            numbered.push(view);
            numbered.len() - 1
        }
    };
    u32::try_from(index + 1).expect("fewer than four billion views")
}

/// The view numbered `number`, if one is.
fn view_numbered(number: u32) -> Option<ViewId> {
    let index = usize::try_from(number.checked_sub(1)?).ok()?;
    let numbered = NUMBERED_VIEWS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    numbered.get(index).copied()
}

impl HasWindowHandle for View {
    /// A `Web` handle carrying this view's number.
    #[allow(unsafe_code)]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = WebWindowHandle::new(number_of(self.id()));
        // SAFETY: the handle carries a number and no pointer, so nothing it borrows can dangle.
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for View {
    /// The web's display, which carries nothing: the view is reached through its window handle.
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::web())
    }
}

/// The view `handle` names, as a handle of the caller's own to it, kept for as long as the
/// caller likes; none for a handle that is not a view's, or whose view has gone.
///
/// What a library handed a window calls to reach WaOS's view: a renderer's surface, a dialog's
/// parent. The window may be a [`View`] or another library's window over one, which answers its
/// view's handle as its own.
pub fn view_of_window_handle(handle: WindowHandle<'_>) -> Option<View> {
    let RawWindowHandle::Web(web) = handle.as_raw() else {
        return None;
    };
    let id = view_numbered(web.id)?;
    views::views().into_iter().find(|view| view.id() == id)
}

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;
    use std::sync::Arc;

    use raw_window_handle::{AppKitWindowHandle, WebCanvasWindowHandle};

    use super::*;

    /// A handle borrowed from nothing: what a test hands `view_of_window_handle`, which follows
    /// no pointer of any handle.
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
    fn a_view_keeps_its_number_and_each_view_has_its_own() {
        let first = number_of(7);
        assert_eq!(number_of(7), first);
        assert_ne!(number_of(8), first);
        assert_eq!(view_numbered(first), Some(7));
    }

    #[test]
    fn a_number_no_view_was_given_names_none() {
        assert_eq!(view_numbered(0), None);
        assert_eq!(view_numbered(u32::MAX), None);
    }

    /// A canvas handle, from a library that points it at something of its own, is never
    /// followed.
    #[test]
    fn a_canvas_handle_is_not_a_view() {
        let canvas = NonNull::<u8>::dangling().cast();
        let handle = WebCanvasWindowHandle::new(canvas);
        assert!(view_of_window_handle(lent(handle.into())).is_none());
    }

    #[test]
    fn a_native_window_is_not_a_view() {
        let window = NonNull::<u8>::dangling().cast();
        let handle = AppKitWindowHandle::new(window);
        assert!(view_of_window_handle(lent(handle.into())).is_none());
    }
}
