//! The adapter a program's `request-adapter` gives, as WebGPU leaves a browser to choose: the GPU
//! the shell's compositor shows from, where an adapter there meets WebGPU's default limits, so a
//! program's pictures reach the screen with no copy between GPUs; otherwise the best of the rest.
//! An adapter that meets WebGPU's default limits comes before one that does not, which a program
//! written for WebGPU may outgrow (a virtual machine's OpenGL with no storage buffers at all);
//! then hardware before software, and Vulkan, Metal and D3D12 before OpenGL; then as the program's
//! power preference ranks the adapter's type. `forceFallbackAdapter` gives a software adapter
//! alone, as WebGPU's does.
//!
//! The invariant: every adapter enumerated and not given is dropped again, since wgpu-core keeps
//! each one enumerated until it is.

use wgpu_core::global::Global;
use wgpu_core::id::{AdapterId, SurfaceId};
use wgpu_types::{
    AdapterInfo, Backend, DeviceType, Limits, PowerPreference, RequestAdapterOptions,
};

use super::{wgpu_core, wgpu_types};
use crate::shell::GpuId;

/// The engine's choice of adapter for its programs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramsAdapter {
    /// The GPU the shell's compositor shows from, where it names one.
    compositor_gpu: Option<GpuId>,
}

impl ProgramsAdapter {
    pub(crate) fn new(compositor_gpu: Option<GpuId>) -> ProgramsAdapter {
        ProgramsAdapter { compositor_gpu }
    }

    /// Whether `adapter` is on the GPU the compositor shows from; true where it names none, as
    /// Apple's systems and Android, whose compositors take any GPU's memory.
    pub(crate) fn on_the_compositors_gpu(&self, instance: &Global, adapter: AdapterId) -> bool {
        self.compositor_gpu
            .is_none_or(|gpu| crate::os::adapter_gpu(instance, adapter) == Some(gpu))
    }

    /// How `adapter` ranks for a program asking with `options`: lower first.
    fn rank(
        &self,
        instance: &Global,
        adapter: AdapterId,
        options: &RequestAdapterOptions<SurfaceId>,
    ) -> (bool, bool, bool, bool, u8) {
        let info = instance.adapter_get_info(adapter);
        let meets_limits = Limits::default().check_limits(&instance.adapter_limits(adapter));
        let shown_from = meets_limits && self.on_the_compositors_gpu(instance, adapter);
        (
            !shown_from,
            !meets_limits,
            is_software(&info),
            info.backend == Backend::Gl,
            power_rank(options.power_preference, info.device_type),
        )
    }
}

impl waos_gpu_host::AdapterChooser for ProgramsAdapter {
    fn choose(
        &self,
        instance: &Global,
        options: &RequestAdapterOptions<SurfaceId>,
    ) -> Option<AdapterId> {
        let adapters =
            instance.enumerate_adapters(wgpu_types::Backends::all(), options.apply_limit_buckets);
        let chosen = adapters
            .iter()
            .copied()
            .filter(|&adapter| {
                !options.force_fallback_adapter || is_software(&instance.adapter_get_info(adapter))
            })
            .min_by_key(|&adapter| self.rank(instance, adapter, options));
        for &adapter in adapters.iter().filter(|&&adapter| Some(adapter) != chosen) {
            instance.adapter_drop(adapter);
        }
        if let Some(adapter) = chosen {
            let info = instance.adapter_get_info(adapter);
            tracing::debug!(name = info.name, backend = ?info.backend, "a program's adapter");
        }
        chosen
    }
}

/// Whether `adapter` draws on the CPU: lavapipe, llvmpipe, WARP.
fn is_software(adapter: &AdapterInfo) -> bool {
    adapter.device_type == DeviceType::Cpu
}

/// Where an adapter of `device_type` comes for `preference`, lower first, as WebGPU's power
/// preference hints: discrete first for high performance, integrated first for low power.
fn power_rank(preference: PowerPreference, device_type: DeviceType) -> u8 {
    match (preference, device_type) {
        (PowerPreference::HighPerformance, DeviceType::DiscreteGpu)
        | (PowerPreference::LowPower, DeviceType::IntegratedGpu) => 0,
        (PowerPreference::HighPerformance, DeviceType::IntegratedGpu)
        | (PowerPreference::LowPower, DeviceType::DiscreteGpu) => 1,
        (_, DeviceType::VirtualGpu) => 2,
        (_, DeviceType::Other) => 3,
        (_, DeviceType::Cpu) => 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_power_preference_puts_its_kind_of_gpu_first() {
        let high = PowerPreference::HighPerformance;
        let low = PowerPreference::LowPower;
        assert!(
            power_rank(high, DeviceType::DiscreteGpu) < power_rank(high, DeviceType::IntegratedGpu)
        );
        assert!(
            power_rank(low, DeviceType::IntegratedGpu) < power_rank(low, DeviceType::DiscreteGpu)
        );
        assert!(power_rank(high, DeviceType::IntegratedGpu) < power_rank(high, DeviceType::Cpu));
    }
}
