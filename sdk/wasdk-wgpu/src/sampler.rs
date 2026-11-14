//! Samplers.

use wasdk::webgpu::{GpuDevice, GpuSampler};
use wgpu::custom::{DispatchSampler, SamplerInterface};

use crate::convert;

#[derive(Debug)]
pub struct WasdkSampler {
    sampler: GpuSampler,
}

downcast!(WasdkSampler: api wgpu::Sampler);

impl WasdkSampler {
    pub(crate) fn gpu_sampler(&self) -> &GpuSampler {
        &self.sampler
    }
}

pub(crate) fn create_sampler(
    device: &GpuDevice,
    desc: &wgpu::SamplerDescriptor<'_>,
) -> DispatchSampler {
    let descriptor = convert::sampler_descriptor_to_wasi(desc);
    DispatchSampler::custom(WasdkSampler {
        sampler: device.create_sampler(Some(&descriptor)),
    })
}

impl SamplerInterface for WasdkSampler {}
