//! wgpu-core's noop backend, for the tests: an instance whose one adapter needs no GPU, which
//! makes buffers and maps them and stubs out the rest. Its adapter says it is a software one.

use std::sync::Arc;

use wgpu_core::global::Global;

/// An instance whose one adapter is the noop backend's.
pub(crate) fn instance() -> Arc<Global> {
    let mut descriptor = wgpu_types::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu_types::Backends::NOOP;
    descriptor.backend_options.noop.enable = true;
    Arc::new(Global::new("waos-gpu-host tests", descriptor, None))
}
