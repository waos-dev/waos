//! Samplers.

use wasdk::gpu::{Device, Sampler};
use wgpu::custom::{DispatchSampler, SamplerInterface};

use crate::convert;

/// The sampler behind a [`wgpu::Sampler`] from this backend.
#[derive(Debug)]
pub struct WasdkSampler {
    sampler: Sampler,
}

downcast!(WasdkSampler: api wgpu::Sampler);

impl WasdkSampler {
    pub(crate) fn wit_sampler(&self) -> &Sampler {
        &self.sampler
    }
}

pub(crate) fn create_sampler(
    device: &Device,
    desc: &wgpu::SamplerDescriptor<'_>,
) -> DispatchSampler {
    let descriptor = convert::sampler_descriptor_to_wit(desc);
    DispatchSampler::custom(WasdkSampler {
        sampler: device.create_sampler(&descriptor),
    })
}

impl SamplerInterface for WasdkSampler {}
