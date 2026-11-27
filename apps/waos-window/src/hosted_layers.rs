//! WaOS's layers shown on a composition layer of the window process's: a host of the layer context
//! WaOS names (`os::LayerHost`), shown as the layer's content through Waft's macOS host
//! (`MacOs::show_system_layer`), for a program's view and for a prompt's dialog alike.
//!
//! The invariant: the window process draws nothing of what it shows so; the layer only shows what
//! WaOS's context holds.

use std::sync::Arc;

use waft::ui::{CompositionLayer, PlatformRef};
use waft_embedder_winit::WinitPlatform;

use crate::os::LayerHost;

/// Shows on `layer` WaOS's layer context `context_id`, in a host of it; answers the host.
pub(crate) fn show_hosted_layers(
    platform: &PlatformRef,
    layer: &Arc<dyn CompositionLayer>,
    context_id: u32,
) -> LayerHost {
    let host = LayerHost::new(context_id);
    let macos = platform
        .downcast_ref::<WinitPlatform>()
        .and_then(WinitPlatform::macos);
    match macos {
        Some(macos) => macos.show_system_layer(layer, host.layer()),
        None => tracing::warn!("WaOS's layers are shown by Waft's macOS host, not this one"),
    }
    host
}
