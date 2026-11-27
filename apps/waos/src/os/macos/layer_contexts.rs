//! Layers another process shows: Core Animation's `CAContext`, private, made on WaOS's connection
//! to the window server with a root layer, whose id a program's window process hosts in a
//! `CALayerHost`, as Chrome's GPU process shares its layers with the browser
//! (`ui/accelerated_widget_mac/ca_layer_tree_coordinator.mm`, `EnsureCAContextAndRootLayer`). It is
//! declared as Chrome declares it (`ui/base/cocoa/remote_layer_api.h`) and checked at run time as
//! Chrome checks it (`ui/base/cocoa/remote_layer_api.mm`, `RemoteLayerAPISupported`): the class,
//! `contextWithCGSConnection:options:`, and the `contextId` and `layer` properties; and, for the
//! fence a window process sends with a new size, `setFencePort:`, which WebKit's content side calls
//! in the transaction that lays its root out at that size
//! (`Source/WebKit/Platform/cocoa/LayerHostingContextManager.mm`, `setVideoLayerSizeFenced`).
//!
//! The invariant: a context and its root are touched on the main thread only, where Waft's
//! `MacOs::place_layer_in` places a layer in the root ("Main thread.", its documentation says); and
//! a fence is set in the same transaction as the layout it holds the window to, with the program's
//! picture at that size where the window waits in its resize for it, as `setVideoLayerSizeFenced`
//! sets it, then let go at once, since the window's commit waits for it (WebKit's `createFence`,
//! whose process cannot answer "during the fence-wait"). WaOS may keep a fence until that picture,
//! never after it: the window commits the transaction carrying it only once WaOS says the picture
//! is shown.

#![allow(unsafe_code)]

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{ffi, msg_send, sel};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSDictionary, NSString};
use objc2_quartz_core::{CALayer, CATransaction};
use waos_window_messages::os::macos::SendRight;

/// Chrome's `CGSConnectionID`: a process's connection to the window server.
type ConnectionId = u32;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    /// The process's own connection to the window server, as Chrome declares it
    /// (`remote_layer_api.h`).
    fn CGSMainConnectionID() -> ConnectionId;
}

/// Whether this system has every class and method a layer context needs; why not, if it lacks
/// one. Chrome's `RemoteLayerAPISupported`, for the side that shares its layers, with
/// `setFencePort:`.
pub(crate) fn check_layer_contexts() -> Result<(), String> {
    let class = context_class().ok_or("Core Animation has no CAContext")?;
    let class_methods = [sel!(contextWithCGSConnection:options:)];
    for method in class_methods {
        if class.class_method(method).is_none() {
            return Err(format!(
                "CAContext has no +{}",
                method.name().to_string_lossy()
            ));
        }
    }
    // Dynamic properties, which an instance answers for no method of the class: Chrome looks
    // them up as properties.
    for property in [c"contextId", c"layer"] {
        // SAFETY: a class from the runtime and a NUL-terminated name.
        let found = unsafe { ffi::class_getProperty(class, property.as_ptr()) };
        if found.is_null() {
            return Err(format!(
                "CAContext has no property {}",
                property.to_string_lossy()
            ));
        }
    }
    let fence: Sel = sel!(setFencePort:);
    if !class.responds_to(fence) {
        return Err("CAContext has no -setFencePort:".to_owned());
    }
    Ok(())
}

fn context_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"CAContext")
}

/// A `CAContext` on WaOS's connection to the window server, with its root layer: what a program's
/// window process shows by the context's id. Main thread only, as the module says.
pub(crate) struct LayerContext {
    context: Retained<AnyObject>,
    root: Retained<CALayer>,
}

impl LayerContext {
    /// A context whose root is laid out from its top-left corner, as a view's layer is. Only after
    /// [`check_layer_contexts`] answered that the system has what it needs, as Chrome makes one
    /// only where `RemoteLayerAPISupported` answered so (`CALayerTreeCoordinator`).
    pub(crate) fn new() -> LayerContext {
        let root = CALayer::new();
        // Chrome's root (`EnsureCAContextAndRootLayer`): flipped, as AppKit's coordinates go up.
        root.setGeometryFlipped(true);
        // Sized from its top-left corner, where the window shows it from, as Chrome anchors the
        // layer it shows a context in (`DisplayCALayerTree::GotCALayerFrame`).
        root.setAnchorPoint(CGPoint::new(0.0, 0.0));
        root.setPosition(CGPoint::new(0.0, 0.0));
        let class = context_class().expect("check_layer_contexts found CAContext");
        let options = NSDictionary::<NSString, AnyObject>::new();
        // SAFETY: the class answers this method, as `check_layer_contexts` found; WaOS has a
        // connection to the window server, being an app; Chrome passes the same arguments.
        let context: Retained<AnyObject> = unsafe {
            msg_send![class, contextWithCGSConnection: CGSMainConnectionID(), options: &*options]
        };
        // SAFETY: `layer` is a property of every context, as `check_layer_contexts` found.
        let () = unsafe { msg_send![&*context, setLayer: &*root] };
        LayerContext { context, root }
    }

    /// The id another process shows the context by.
    pub(crate) fn id(&self) -> u32 {
        // SAFETY: `contextId` is a property of every context, as `check_layer_contexts` found.
        unsafe { msg_send![&*self.context, contextId] }
    }

    /// Sizes the root to `size`, its width and height in points, and has `place` lay its sublayers
    /// out in it, and show what goes with that layout, in one transaction; under `fence`, when the
    /// window showing the context sent one, whose own change of size waits for it, so both land in
    /// the same frame (WebKit's `LayerHostingContextManager::setVideoLayerSizeFenced` sets the
    /// fence, then lays the root out). The fence is let go once the transaction is committed.
    pub(crate) fn lay_out(
        &self,
        size: (f64, f64),
        fence: Option<SendRight>,
        place: impl FnOnce(&CALayer),
    ) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(fence) = &fence {
            // SAFETY: the context answers `setFencePort:`, as `check_layer_contexts` found; the
            // right is the fence's own, kept until after the commit.
            let () = unsafe { msg_send![&*self.context, setFencePort: fence.name()] };
        }
        let (width, height) = size;
        self.root.setBounds(CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width, height),
        ));
        place(&self.root);
        CATransaction::commit();
        drop(fence);
    }
}

impl Drop for LayerContext {
    /// The context lets go of its root first, as Chrome's does when it makes a new one.
    fn drop(&mut self) {
        // SAFETY: as in `new`; a context with no layer shows nothing.
        let () = unsafe { msg_send![&*self.context, setLayer: Option::<&CALayer>::None] };
    }
}
