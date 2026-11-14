//! Query sets.

use wasdk::webgpu::{GpuDevice, GpuQuerySet, GpuQuerySetDescriptor, GpuQueryType};
use wgpu::custom::{DispatchQuerySet, QuerySetInterface};

use crate::convert;
use crate::gap::feature_gap;

#[derive(Debug)]
pub struct WasdkQuerySet {
    query_set: GpuQuerySet,
}

downcast!(WasdkQuerySet: api wgpu::QuerySet, dispatch DispatchQuerySet);

impl WasdkQuerySet {
    pub(crate) fn gpu_query_set(&self) -> &GpuQuerySet {
        &self.query_set
    }
}

pub(crate) fn create_query_set(
    device: &GpuDevice,
    desc: &wgpu::QuerySetDescriptor<'_>,
) -> DispatchQuerySet {
    let type_ = match desc.ty {
        wgpu::QueryType::Occlusion => GpuQueryType::Occlusion,
        wgpu::QueryType::Timestamp => GpuQueryType::Timestamp,
        wgpu::QueryType::PipelineStatistics(_) => feature_gap(
            wgpu::Features::PIPELINE_STATISTICS_QUERY,
            "QueryType::PipelineStatistics",
        ),
    };
    let descriptor = GpuQuerySetDescriptor {
        type_,
        count: desc.count,
        label: convert::label(desc.label),
    };
    let query_set = device
        .create_query_set(&descriptor)
        .unwrap_or_else(|error| panic!("wasdk-wgpu: Device::create_query_set: {}", error.message));
    DispatchQuerySet::custom(WasdkQuerySet { query_set })
}

impl QuerySetInterface for WasdkQuerySet {
    fn destroy(&self) {
        self.query_set.destroy();
    }
}
