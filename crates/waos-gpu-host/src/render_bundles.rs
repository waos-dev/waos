//! The render bundle encoder, which records draws once to replay in many render passes, and the
//! bundle it finishes into. wgpu-core validates what a bundle records at `finish`, so a command
//! here is refused only then.

use std::sync::Arc;

use wasmtime::component::Resource;
use wgpu_core::command;
use wgpu_core::global::Global;
use wgpu_core::id::RenderBundleId;

use crate::binding::BindGroup;
use crate::buffers::Buffer;
use crate::conversions::{label, non_zero_u64};
use crate::devices::Device;
use crate::errors::ErrorSink;
use crate::pipelines::RenderPipeline;
use crate::wasdk::gpu::gpu::{self, IndexFormat, Range};
use crate::{GpuCtx, GpuObject};

/// A render bundle encoder, with the instance it records on and its device's errors.
pub struct RenderBundleEncoder {
    encoder: Box<command::RenderBundleEncoder>,
    instance: Arc<Global>,
    errors: Arc<ErrorSink>,
}

pub type RenderBundle = GpuObject<RenderBundleId>;

impl RenderBundleEncoder {
    /// An encoder for bundles `descriptor` describes, on `device`.
    pub(crate) fn create(
        device: &Device,
        descriptor: command::RenderBundleEncoderDescriptor<'static>,
    ) -> RenderBundleEncoder {
        let instance = device.instance();
        let (encoder, error) =
            instance.device_create_render_bundle_encoder(device.device_id(), &descriptor);
        device.errors.report(
            error,
            "Device::create_render_bundle_encoder",
            descriptor.label.as_deref(),
        );
        RenderBundleEncoder {
            encoder,
            instance: Arc::clone(instance),
            errors: Arc::clone(&device.errors),
        }
    }

    /// Records a command with `record`. The encoder ends only at `finish`, which consumes it, so
    /// recording is never refused, as wgpu expects of it.
    fn record<E: std::fmt::Debug>(
        &mut self,
        record: impl FnOnce(&Global, &mut command::RenderBundleEncoder) -> Result<(), E>,
    ) -> wasmtime::Result<()> {
        record(&self.instance, &mut self.encoder)
            .map_err(|error| wasmtime::format_err!("a render bundle encoder ended: {error:?}"))
    }
}

impl GpuCtx<'_> {
    fn bundle_encoder(
        &mut self,
        encoder: &Resource<RenderBundleEncoder>,
    ) -> wasmtime::Result<&mut RenderBundleEncoder> {
        Ok(self.table.get_mut(encoder)?)
    }
}

impl gpu::HostRenderBundleEncoder for GpuCtx<'_> {
    fn set_pipeline(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        pipeline: Resource<RenderPipeline>,
    ) -> wasmtime::Result<()> {
        let pipeline = self.table.get(&pipeline)?.id();
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_set_pipeline(encoder, pipeline)
        })
    }

    fn set_bind_group(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        index: u32,
        bind_group: Option<Resource<BindGroup>>,
        offsets: Vec<u32>,
    ) -> wasmtime::Result<()> {
        let bind_group = bind_group
            .map(|bind_group| Ok::<_, wasmtime::Error>(self.table.get(&bind_group)?.id()))
            .transpose()?;
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_set_bind_group(encoder, index, bind_group, &offsets)
        })
    }

    fn set_index_buffer(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        buffer: Resource<Buffer>,
        format: IndexFormat,
        offset: u64,
        size: Option<u64>,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        let size = non_zero_u64(size, "set-index-buffer's size")?;
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_set_index_buffer(
                encoder,
                buffer,
                format.into(),
                offset,
                size,
            )
        })
    }

    fn set_vertex_buffer(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        slot: u32,
        buffer: Option<Resource<Buffer>>,
        offset: u64,
        size: Option<u64>,
    ) -> wasmtime::Result<()> {
        let buffer = buffer
            .map(|buffer| Ok::<_, wasmtime::Error>(self.table.get(&buffer)?.id()))
            .transpose()?;
        let size = non_zero_u64(size, "set-vertex-buffer's size")?;
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_set_vertex_buffer(encoder, slot, buffer, offset, size)
        })
    }

    fn set_immediates(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        offset: u32,
        data: Vec<u8>,
    ) -> wasmtime::Result<()> {
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_set_immediates(encoder, offset, &data)
        })
    }

    fn draw(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        vertices: Range,
        instances: Range,
    ) -> wasmtime::Result<()> {
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_draw(
                encoder,
                vertices.end.wrapping_sub(vertices.start),
                instances.end.wrapping_sub(instances.start),
                vertices.start,
                instances.start,
            )
        })
    }

    fn draw_indexed(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        indices: Range,
        base_vertex: i32,
        instances: Range,
    ) -> wasmtime::Result<()> {
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_draw_indexed(
                encoder,
                indices.end.wrapping_sub(indices.start),
                instances.end.wrapping_sub(instances.start),
                indices.start,
                base_vertex,
                instances.start,
            )
        })
    }

    fn draw_indirect(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        buffer: Resource<Buffer>,
        offset: u64,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_draw_indirect(encoder, buffer, offset)
        })
    }

    fn draw_indexed_indirect(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        buffer: Resource<Buffer>,
        offset: u64,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.bundle_encoder(&encoder)?.record(|instance, encoder| {
            instance.render_bundle_encoder_draw_indexed_indirect(encoder, buffer, offset)
        })
    }

    fn finish(
        &mut self,
        encoder: Resource<RenderBundleEncoder>,
        bundle_label: Option<String>,
    ) -> wasmtime::Result<Resource<RenderBundle>> {
        let mut encoder = self.table.delete(encoder)?;
        let descriptor = command::RenderBundleDescriptor {
            label: label(bundle_label),
        };
        let (bundle, error) =
            self.instance
                .render_bundle_encoder_finish(&mut encoder.encoder, &descriptor, None);
        encoder.errors.report(
            error,
            "RenderBundleEncoder::finish",
            descriptor.label.as_deref(),
        );
        Ok(self.table.push(GpuObject::new(bundle, self.instance))?)
    }

    fn drop(&mut self, encoder: Resource<RenderBundleEncoder>) -> wasmtime::Result<()> {
        self.table.delete(encoder)?;
        Ok(())
    }
}

impl gpu::HostRenderBundle for GpuCtx<'_> {
    fn drop(&mut self, bundle: Resource<RenderBundle>) -> wasmtime::Result<()> {
        self.table.delete(bundle)?;
        Ok(())
    }
}
