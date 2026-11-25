//! The GPU side: the one wgpu-core instance every program draws on, made with the engine and
//! outliving every program; how a program's device is opened on it, and the devices it opened;
//! when a program may submit GPU work, which is not while it is frozen, its own or what the engine
//! submits for it; and what of its work is in flight. `wasi:webgpu` itself is served by
//! `waos-webgpu-host` over this instance, with its objects in each program's store, so that a
//! program ended gives them back with its store.
//!
//! The invariant: wgpu-core is named here and in the modules that make hardware buffers of a
//! program's frames, and nowhere an interface's state can see. The wgpu crates are reached through
//! the WebGPU host's re-exports, so the engine works at the version the host does.

mod devices;
mod in_flight;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub(crate) use waos_webgpu_host::reexports::{wgpu_core, wgpu_types};
use waos_webgpu_host::{DeviceOpener, SubmissionGate};

pub(crate) use devices::OpenedDevices;
pub(crate) use in_flight::{Finishing, InFlight};

use crate::HardwareBufferMode;
use crate::lifecycle::{ExecutionState, Lifecycle};

/// The instance a program's device is opened on: wgpu-core's, over every backend this machine
/// offers, D3D12's alone on Windows.
pub(crate) type GpuInstance = Arc<wgpu_core::global::Global>;

/// A fresh instance for an engine, over every backend this machine offers; on Windows D3D12's
/// alone, whose devices open the textures shared by NT handle a program draws into (`os`).
pub(crate) fn instance() -> GpuInstance {
    let backends = if cfg!(windows) {
        wgpu_types::Backends::DX12
    } else {
        wgpu_types::Backends::all()
    };
    Arc::new(wgpu_core::global::Global::new(
        "waos",
        wgpu_types::InstanceDescriptor {
            backends,
            flags: instance_flags(),
            backend_options: Default::default(),
            memory_budget_thresholds: Default::default(),
            display: None,
        },
        None,
    ))
}

/// wgpu's flags for this build, except that on Linux GPU objects get no debug names
/// (`InstanceFlags::DEBUG`): the Vulkan loader names one by walking every device's list without a
/// lock (`loader_get_icd_and_device`, loader 1.4.341), so a name set while another thread makes
/// or drops a device crashes the process. Validation stays. Back to the build's flags once the
/// loader locks that walk.
fn instance_flags() -> wgpu_types::InstanceFlags {
    let flags = wgpu_types::InstanceFlags::from_build_config();
    if cfg!(target_os = "linux") {
        flags - wgpu_types::InstanceFlags::DEBUG
    } else {
        flags
    }
}

/// How many objects `gpu` holds now of each kind, over every program: wgpu-core's registries,
/// where a program's objects are until its handles or its store give them back. What a test
/// reads to see a program stopped give them back; no count of the GPU leaves the engine.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn objects_by_kind(gpu: &wgpu_core::global::Global) -> Vec<(&'static str, usize)> {
    let hub = gpu.generate_report().hub;
    [
        ("adapters", hub.adapters),
        ("devices", hub.devices),
        ("queues", hub.queues),
        ("pipeline layouts", hub.pipeline_layouts),
        ("shader modules", hub.shader_modules),
        ("bind group layouts", hub.bind_group_layouts),
        ("bind groups", hub.bind_groups),
        ("command encoders", hub.command_encoders),
        ("command buffers", hub.command_buffers),
        ("render bundles", hub.render_bundles),
        ("render pipelines", hub.render_pipelines),
        ("compute pipelines", hub.compute_pipelines),
        ("query sets", hub.query_sets),
        ("buffers", hub.buffers),
        ("textures", hub.textures),
        ("texture views", hub.texture_views),
        ("samplers", hub.samplers),
    ]
    .into_iter()
    .map(|(kind, registry)| (kind, registry.num_allocated))
    .collect()
}

/// Waits, blocking, for `submission` on `device`, or for everything submitted there so far where
/// none: the program's work that drew what is waited for. The caller holds the device meanwhile. A
/// wait that fails is said, and over.
pub(crate) fn wait_for(
    gpu: &wgpu_core::global::Global,
    device: wgpu_core::id::DeviceId,
    submission: Option<wgpu_core::SubmissionIndex>,
) {
    let waited = gpu.device_poll(
        device,
        wgpu_types::PollType::Wait {
            submission_index: submission,
            timeout: None,
        },
    );
    if let Err(error) = waited {
        tracing::warn!("waiting for a program's GPU work: {error:#?}");
    }
}

/// Runs `submit`, GPU work the engine submits on the program's behalf, once the program may
/// submit, as the program's own `queue.submit` does: at once while the program is not frozen and
/// once it is unfrozen while it is. `submit` asks nothing of the lifecycle.
pub(crate) async fn submit_once_unfrozen<R>(
    lifecycle: &Lifecycle,
    mut submit: impl FnMut() -> R,
) -> R {
    loop {
        until_unfrozen(lifecycle).await;
        let mut submitted = None;
        submit_unless_frozen(lifecycle, &mut || submitted = Some(submit()));
        if let Some(submitted) = submitted {
            return submitted;
        }
    }
}

/// Resolves at once while the program is not frozen, and once it is unfrozen while it is.
fn until_unfrozen(lifecycle: &Lifecycle) -> impl Future<Output = ()> + Send + 'static + use<> {
    let mut execution_state = lifecycle.execution_state();
    async move {
        // A lifecycle gone is a program gone, whose work nobody waits for.
        let unfrozen = |state: &ExecutionState| *state != ExecutionState::Frozen;
        let _ = execution_state.wait_for(unfrozen).await;
    }
}

/// Runs `submit` unless the program is frozen, and answers whether it ran. The program's
/// execution state is held as it is meanwhile, so a freeze decided while `submit` runs waits the
/// moment it takes, and no GPU work starts once a freeze has taken. `submit` asks nothing of the
/// lifecycle, which would wait on this.
pub(crate) fn submit_unless_frozen(lifecycle: &Lifecycle, submit: &mut dyn FnMut()) -> bool {
    let execution_state = lifecycle.execution_state();
    let held = execution_state.borrow();
    if *held == ExecutionState::Frozen {
        return false;
    }
    submit();
    true
}

/// How the WebGPU host opens a program's device: on Android with the extensions that import an
/// `AHardwareBuffer` and hand it to the compositor (`os::HardwareBufferImports`).
#[cfg(target_os = "android")]
pub(crate) fn device_opener(_mode: HardwareBufferMode) -> Option<&'static dyn DeviceOpener> {
    Some(&crate::os::HardwareBufferImports)
}

/// How the WebGPU host opens a program's device on Linux: with dmabuf import extensions in `Auto`
/// mode, and plain wgpu-core devices in `Offscreen` mode so software Vulkan (lavapipe) works
/// without `VK_EXT_external_memory_dma_buf` or `VK_EXT_queue_family_foreign`.
#[cfg(target_os = "linux")]
pub(crate) fn device_opener(mode: HardwareBufferMode) -> Option<&'static dyn DeviceOpener> {
    match mode {
        HardwareBufferMode::Auto => Some(&crate::os::HardwareBufferImports),
        HardwareBufferMode::Offscreen => None,
    }
}

/// How the WebGPU host opens a program's device: as wgpu-core opens one. That serves wherever the
/// system's compositor reads a hardware buffer without an extension on the device that drew it,
/// which on Apple's systems an `IOSurface` does.
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) fn device_opener(_mode: HardwareBufferMode) -> Option<&'static dyn DeviceOpener> {
    None
}

/// A device of the engine's own on `adapter` of `gpu`, opened as a program's is
/// ([`device_opener`]), so that it takes the system's hardware buffers as a program's does.
pub(crate) fn open_device(
    gpu: &wgpu_core::global::Global,
    adapter: wgpu_core::id::AdapterId,
    descriptor: &wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>,
    mode: HardwareBufferMode,
) -> Result<
    (wgpu_core::id::DeviceId, wgpu_core::id::QueueId),
    wgpu_core::instance::RequestDeviceError,
> {
    match device_opener(mode) {
        Some(opener) => opener.open(gpu, adapter, descriptor),
        None => gpu.adapter_request_device(adapter, descriptor, None, None),
    }
}

/// A program's submissions wait while it is frozen: iOS refuses GPU work from the background,
/// and what the engine does for a frozen program, a frame it presented coming through, could
/// reach one. Unfrozen, the submission goes through, and none starts once a freeze has taken
/// ([`submit_unless_frozen`]).
impl SubmissionGate for Lifecycle {
    fn until_allowed(&self) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        Box::pin(until_unfrozen(self))
    }

    fn while_allowed(&self, submit: &mut dyn FnMut()) -> bool {
        submit_unless_frozen(self, submit)
    }
}
