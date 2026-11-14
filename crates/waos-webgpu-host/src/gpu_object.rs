//! Every wgpu-core object this host hands a program is owned by the resource it is handed out
//! as. When the resource goes, whether the program drops its handle or its store is dropped with
//! the handle still in its table, the object is given back to the instance that made it, and
//! wgpu-core frees it once no submission still uses it.
//!
//! The invariant: this is the one place wgpu-core's `*_drop` functions are called. A resource
//! that holds an id any other way leaks it when its program ends.

use std::sync::Arc;

use wgpu_core::global::Global;
use wgpu_core::id;

use crate::labels::ObjectLabel;

/// A wgpu-core object a program holds: its id, dropped from the instance that made it when this
/// goes, and its WebGPU label.
pub struct GpuObject<Id: GpuObjectId> {
    id: Id,
    instance: Arc<Global>,
    label: ObjectLabel,
}

impl<Id: GpuObjectId> GpuObject<Id> {
    /// The object `id` on `instance`, with no label: one this crate made, or one its embedder
    /// made and hands to a program, such as a canvas's texture.
    pub fn new(id: Id, instance: &Arc<Global>) -> GpuObject<Id> {
        GpuObject {
            id,
            instance: Arc::clone(instance),
            label: ObjectLabel::default(),
        }
    }

    /// The object, labelled as its descriptor labels it.
    pub(crate) fn labelled(self, label: Option<&str>) -> GpuObject<Id> {
        self.label.set(label.unwrap_or_default().to_owned());
        self
    }

    pub fn id(&self) -> Id {
        self.id
    }

    /// The instance that made the object, which it is given back to.
    pub(crate) fn instance(&self) -> &Arc<Global> {
        &self.instance
    }

    pub(crate) fn label(&self) -> &ObjectLabel {
        &self.label
    }
}

impl<Id: GpuObjectId> Drop for GpuObject<Id> {
    fn drop(&mut self) {
        self.id.drop_from(&self.instance);
    }
}

/// The id of one kind of wgpu-core object, and how an object of that kind is given back.
pub trait GpuObjectId: Copy + Send + Sync + 'static {
    /// Drops the object from `instance`'s registry.
    fn drop_from(self, instance: &Global);
}

/// Each kind of object with the function wgpu-core drops it with.
macro_rules! dropped_with {
    ($($kind:ty => $drop:ident,)*) => {
        $(
            impl GpuObjectId for $kind {
                fn drop_from(self, instance: &Global) {
                    instance.$drop(self);
                }
            }
        )*
    };
}

dropped_with! {
    id::AdapterId => adapter_drop,
    id::DeviceId => device_drop,
    id::QueueId => queue_drop,
    id::BufferId => buffer_drop,
    id::TextureId => texture_drop,
    id::TextureViewId => texture_view_drop,
    id::SamplerId => sampler_drop,
    id::BindGroupLayoutId => bind_group_layout_drop,
    id::PipelineLayoutId => pipeline_layout_drop,
    id::BindGroupId => bind_group_drop,
    id::ShaderModuleId => shader_module_drop,
    id::RenderPipelineId => render_pipeline_drop,
    id::ComputePipelineId => compute_pipeline_drop,
    id::CommandEncoderId => command_encoder_drop,
    id::CommandBufferId => command_buffer_drop,
    id::RenderBundleId => render_bundle_drop,
    id::QuerySetId => query_set_drop,
}
