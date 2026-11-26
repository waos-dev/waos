//! The texture and what reads it: its views, the external textures over them, and samplers. A
//! texture keeps what it was made with, which wgpu-core has no way to answer.

use std::sync::Arc;

use wasmtime::component::{Resource, ResourceTable};
use wgpu_core::id::{ExternalTextureId, SamplerId, TextureId, TextureViewId};

use crate::devices::Device;
use crate::errors::ErrorSink;
use crate::wasdk::gpu::gpu::{self, TexelCopyTextureInfo, TextureViewDescriptor};
use crate::{GpuCtx, GpuObject};

/// A program's handle on a texture: one it made, its alone, or a frame its embedder made and
/// keeps a handle on too, until the frame is presented.
pub struct Texture {
    object: Arc<GpuObject<TextureId>>,
    descriptor: TextureDescriptor,
    errors: Arc<ErrorSink>,
}

/// What a texture was made with, as wgpu-types describes it, its label the program's own string.
pub type TextureDescriptor =
    wgpu_types::TextureDescriptor<Option<String>, Vec<wgpu_types::TextureFormat>>;

pub type TextureView = GpuObject<TextureViewId>;
pub type ExternalTexture = GpuObject<ExternalTextureId>;
pub type Sampler = GpuObject<SamplerId>;

impl Texture {
    pub(crate) fn new(
        object: Arc<GpuObject<TextureId>>,
        descriptor: TextureDescriptor,
        errors: Arc<ErrorSink>,
    ) -> Texture {
        Texture {
            object,
            descriptor,
            errors,
        }
    }

    /// The texture `descriptor` describes, on `device`.
    pub(crate) fn create(device: &Device, descriptor: TextureDescriptor) -> Texture {
        let instance = device.instance();
        let core_descriptor = descriptor.map_label(|label| label.as_deref().map(Into::into));
        let (texture, error) =
            instance.device_create_texture(device.device_id(), &core_descriptor, None);
        device
            .errors
            .report(error, "Device::create_texture", descriptor.label.as_deref());
        Texture::new(
            Arc::new(GpuObject::new(texture, instance)),
            descriptor,
            Arc::clone(&device.errors),
        )
    }

    pub fn id(&self) -> TextureId {
        self.object.id()
    }
}

/// A texture's side of a copy, as wgpu-core takes it.
pub(crate) fn texel_copy_texture(
    table: &ResourceTable,
    info: TexelCopyTextureInfo,
) -> wasmtime::Result<wgpu_types::TexelCopyTextureInfo<TextureId>> {
    Ok(wgpu_types::TexelCopyTextureInfo {
        texture: table.get(&info.texture)?.id(),
        mip_level: info.mip_level,
        origin: info.origin.into(),
        aspect: info.aspect.into(),
    })
}

impl gpu::HostTexture for GpuCtx<'_> {
    fn create_view(
        &mut self,
        texture: Resource<Texture>,
        descriptor: TextureViewDescriptor,
    ) -> wasmtime::Result<Resource<TextureView>> {
        let descriptor = descriptor.into();
        let texture = self.table.get(&texture)?;
        let (view, error) = self
            .instance
            .texture_create_view(texture.id(), &descriptor, None);
        texture
            .errors
            .report(error, "Texture::create_view", descriptor.label.as_deref());
        Ok(self.table.push(GpuObject::new(view, self.instance))?)
    }

    fn descriptor(
        &mut self,
        texture: Resource<Texture>,
    ) -> wasmtime::Result<gpu::TextureDescriptor> {
        Ok((&self.table.get(&texture)?.descriptor).into())
    }

    fn destroy(&mut self, texture: Resource<Texture>) -> wasmtime::Result<()> {
        let texture = self.table.get(&texture)?.id();
        self.instance.texture_destroy(texture);
        Ok(())
    }

    fn drop(&mut self, texture: Resource<Texture>) -> wasmtime::Result<()> {
        self.table.delete(texture)?;
        Ok(())
    }
}

impl gpu::HostTextureView for GpuCtx<'_> {
    fn drop(&mut self, view: Resource<TextureView>) -> wasmtime::Result<()> {
        self.table.delete(view)?;
        Ok(())
    }
}

impl gpu::HostExternalTexture for GpuCtx<'_> {
    fn destroy(&mut self, texture: Resource<ExternalTexture>) -> wasmtime::Result<()> {
        let texture = self.table.get(&texture)?.id();
        self.instance.external_texture_destroy(texture);
        Ok(())
    }

    fn drop(&mut self, texture: Resource<ExternalTexture>) -> wasmtime::Result<()> {
        self.table.delete(texture)?;
        Ok(())
    }
}

impl gpu::HostSampler for GpuCtx<'_> {
    fn drop(&mut self, sampler: Resource<Sampler>) -> wasmtime::Result<()> {
        self.table.delete(sampler)?;
        Ok(())
    }
}
