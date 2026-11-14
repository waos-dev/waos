//! Every WebGPU object's label: what its descriptor named it at creation, then whatever the
//! program sets. https://www.w3.org/TR/webgpu/#dom-gpuobjectbase-label
//!
//! The label is the host's own: wgpu-core keeps the creation label for its messages and has no
//! way to change it, so a label set later is answered here and not seen in wgpu-core's errors.

use std::sync::Mutex;

use wasmtime::component::Resource;

use crate::compilation_info::ShaderModule;
use crate::gpu_object::{GpuObject, GpuObjectId};
use crate::types::{
    Buffer, CommandEncoder, ComputePassEncoder, ComputePipeline, Device, QuerySet, Queue,
    RenderBundleEncoder, RenderPassEncoder, RenderPipeline, Texture,
};
use crate::WasiWebGpuCtx;

/// A WebGPU object's label, shared by every handle on the object.
#[derive(Debug, Default)]
pub struct ObjectLabel(Mutex<String>);

impl ObjectLabel {
    pub(crate) fn of(label: Option<&str>) -> ObjectLabel {
        ObjectLabel(Mutex::new(label.unwrap_or_default().to_owned()))
    }

    pub(crate) fn get(&self) -> String {
        self.0.lock().unwrap().clone()
    }

    pub(crate) fn set(&self, label: String) {
        *self.0.lock().unwrap() = label;
    }
}

/// A resource that is a WebGPU object, and so has a label.
pub(crate) trait Labelled: Send + 'static {
    fn object_label(&self) -> &ObjectLabel;
}

impl<Id: GpuObjectId> Labelled for GpuObject<Id> {
    fn object_label(&self) -> &ObjectLabel {
        self.label()
    }
}

impl Labelled for Device {
    fn object_label(&self) -> &ObjectLabel {
        self.device.label()
    }
}

impl Labelled for Queue {
    fn object_label(&self) -> &ObjectLabel {
        self.queue.label()
    }
}

impl Labelled for Buffer {
    fn object_label(&self) -> &ObjectLabel {
        self.object.label()
    }
}

impl Labelled for Texture {
    fn object_label(&self) -> &ObjectLabel {
        self.texture.label()
    }
}

impl Labelled for RenderPipeline {
    fn object_label(&self) -> &ObjectLabel {
        self.render_pipeline.label()
    }
}

impl Labelled for ComputePipeline {
    fn object_label(&self) -> &ObjectLabel {
        self.compute_pipeline.label()
    }
}

impl Labelled for CommandEncoder {
    fn object_label(&self) -> &ObjectLabel {
        self.command_encoder.label()
    }
}

impl Labelled for ShaderModule {
    fn object_label(&self) -> &ObjectLabel {
        self.module.label()
    }
}

impl Labelled for QuerySet {
    fn object_label(&self) -> &ObjectLabel {
        self.query_set.label()
    }
}

impl Labelled for RenderPassEncoder {
    fn object_label(&self) -> &ObjectLabel {
        &self.label
    }
}

impl Labelled for ComputePassEncoder {
    fn object_label(&self) -> &ObjectLabel {
        &self.label
    }
}

impl Labelled for RenderBundleEncoder {
    fn object_label(&self) -> &ObjectLabel {
        &self.label
    }
}

impl WasiWebGpuCtx<'_> {
    pub(crate) fn label_of<T: Labelled>(&self, object: &Resource<T>) -> wasmtime::Result<String> {
        Ok(self.table.get(object)?.object_label().get())
    }

    pub(crate) fn relabel<T: Labelled>(
        &self,
        object: &Resource<T>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.table.get(object)?.object_label().set(label);
        Ok(())
    }
}
