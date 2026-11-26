//! Query sets.

use wasdk::gpu::{Device, QuerySet};
use wasdk::gpu_types::QuerySetDescriptor;
use wgpu::custom::{DispatchQuerySet, QuerySetInterface};

use crate::convert;

/// The query set behind a [`wgpu::QuerySet`] from this backend.
#[derive(Debug)]
pub struct WasdkQuerySet {
    query_set: QuerySet,
}

downcast!(WasdkQuerySet: api wgpu::QuerySet, dispatch DispatchQuerySet);

impl WasdkQuerySet {
    pub(crate) fn wit_query_set(&self) -> &QuerySet {
        &self.query_set
    }
}

pub(crate) fn create_query_set(
    device: &Device,
    desc: &wgpu::QuerySetDescriptor<'_>,
) -> DispatchQuerySet {
    let descriptor = QuerySetDescriptor {
        label: convert::label(desc.label),
        ty: convert::query_type_to_wit(desc.ty),
        count: desc.count,
    };
    DispatchQuerySet::custom(WasdkQuerySet {
        query_set: device.create_query_set(&descriptor),
    })
}

impl QuerySetInterface for WasdkQuerySet {
    fn destroy(&self) {
        self.query_set.destroy();
    }
}
