//! What a view of the program shows, which stays in WaOS: the layers in the root of a layer context
//! of WaOS's, shown in the program's window by Core Animation's `CALayerHost`, private, as Chrome's
//! browser shows its GPU process's layers (`ui/accelerated_widget_mac/display_ca_layer_tree.mm`,
//! `GotCALayerFrame`). It is declared as Chrome declares it (`ui/base/cocoa/remote_layer_api.h`)
//! and checked at run time as Chrome checks it (`ui/base/cocoa/remote_layer_api.mm`,
//! `RemoteLayerAPISupported`): the class, and its `contextId` and `setContextId:`. A new size of
//! the window lands with WaOS's layout at that size, and the program's picture at it, through a
//! fence of the window's own context, made in the transaction that commits the window's size, as
//! WebKit's UI process makes one when its view resizes
//! (`Source/WebKit/UIProcess/mac/TiledCoreAnimationDrawingAreaProxy.mm`, `createFence`:
//! `-[CALayer context]`, then `-[CAContext createFencePort]`), checked at run time the same way.
//!
//! The invariant: the window process draws nothing of its own (decision 5 of
//! `docs/proposals/2026-10-29-macos-apps.md`); a host only shows what WaOS's context holds.

#![allow(unsafe_code)]

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, sel};
use objc2_core_foundation::CGPoint;
use objc2_quartz_core::CALayer;
use waos_window_messages::os::macos::SendRight;

/// Mach's null port, which `createFencePort` answers when it made none.
const NO_PORT: u32 = 0;

/// Whether this system has every class and method a layer host and its fences need; why not, if
/// it lacks one. Chrome's `RemoteLayerAPISupported`, for the side that shows another process's
/// layers, with what WebKit's `createFence` calls.
pub(crate) fn check_layer_hosts() -> Result<(), String> {
    let needed = [
        (c"CALayerHost", vec![sel!(contextId), sel!(setContextId:)]),
        (c"CALayer", vec![sel!(context)]),
        (c"CAContext", vec![sel!(createFencePort)]),
    ];
    for (name, methods) in needed {
        let class = AnyClass::get(name)
            .ok_or_else(|| format!("Core Animation has no {}", name.to_string_lossy()))?;
        for method in methods {
            if !class.responds_to(method) {
                return Err(format!(
                    "{} has no -{}",
                    name.to_string_lossy(),
                    method.name().to_string_lossy()
                ));
            }
        }
    }
    Ok(())
}

fn host_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"CALayerHost")
}

/// A `CALayerHost` showing the root of WaOS's layer context: a layer of the window's own tree, which
/// the window's canvas shows. Main thread only, where Waft's `MacOs::show_system_layer` shows it
/// ("Main thread.", its documentation says).
pub(crate) struct LayerHost(Retained<CALayer>);

impl LayerHost {
    /// A host of the context `context_id`. Only after [`check_layer_hosts`] answered that the
    /// system has what it needs, as Chrome's browser makes one only for a context its GPU process
    /// made where `RemoteLayerAPISupported` answered so.
    pub(crate) fn new(context_id: u32) -> LayerHost {
        let class = host_class().expect("check_layer_hosts found CALayerHost");
        // SAFETY: `CALayerHost` is a `CALayer`, made as any object is.
        let host: Retained<CALayer> = unsafe { msg_send![class, new] };
        // The context's root is laid out from the host's top-left corner, as Chrome anchors its
        // host (`GotCALayerFrame`).
        host.setAnchorPoint(CGPoint::new(0.0, 0.0));
        // SAFETY: the class answers `setContextId:`, as `check_layer_hosts` found.
        let () = unsafe { msg_send![&*host, setContextId: context_id] };
        LayerHost(host)
    }

    /// The layer, for the canvas to show.
    pub(crate) fn layer(&self) -> &CALayer {
        &self.0
    }

    /// A fence installed on the transaction this thread commits next in the window the host is
    /// shown in, which that commit waits for until every right to it is let go or Core Animation
    /// gives up on it, as WebKit's UI process waits for the fence it sends with a new size
    /// (`createFence`: the fence is "installed", and the process cannot answer "during the
    /// fence-wait"). Sent to WaOS with a new size the window waits at, so the window's change of
    /// size lands with WaOS's layout at it and the program's picture at that size, which WaOS
    /// keeps the fence for: the window commits only once WaOS says the picture is shown, so no
    /// commit waits on the fence meanwhile. None while the host is in no window.
    pub(crate) fn fence(&self) -> Option<SendRight> {
        // SAFETY: every layer answers `context`, as `check_layer_hosts` found; none in no window.
        let context: Option<Retained<AnyObject>> = unsafe { msg_send![&*self.0, context] };
        // SAFETY: a context answers `createFencePort`, as `check_layer_hosts` found.
        let port: u32 = unsafe { msg_send![&*context?, createFencePort] };
        // SAFETY: `createFencePort` gives its caller the one send right it made (WebKit adopts it
        // so, `LayerHostingContext::createFencePort`).
        (port != NO_PORT).then(|| unsafe { SendRight::adopt(port) })
    }
}
