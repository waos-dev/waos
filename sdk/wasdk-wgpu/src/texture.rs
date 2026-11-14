//! Textures and their views.

use wasdk::webgpu::{
    GpuDevice, GpuTexelCopyTextureInfo, GpuTexture, GpuTextureDescriptor, GpuTextureView,
    GpuTextureViewDescriptor,
};
use wgpu::custom::{DispatchTexture, DispatchTextureView, TextureInterface, TextureViewInterface};

use crate::convert;

/// The texture behind a [`wgpu::Texture`] from this backend: `texture.as_custom::<WasdkTexture>()`.
#[derive(Debug)]
pub struct WasdkTexture {
    texture: GpuTexture,
}

downcast!(WasdkTexture: api wgpu::Texture);

impl WasdkTexture {
    pub(crate) fn new(texture: GpuTexture) -> Self {
        WasdkTexture { texture }
    }

    /// The wasi texture behind the wgpu one.
    pub fn gpu_texture(&self) -> &GpuTexture {
        &self.texture
    }
}

pub(crate) fn create_texture(
    device: &GpuDevice,
    desc: &wgpu::TextureDescriptor<'_>,
) -> DispatchTexture {
    let descriptor = GpuTextureDescriptor {
        size: convert::extent_to_wasi(desc.size),
        mip_level_count: Some(desc.mip_level_count),
        sample_count: Some(desc.sample_count),
        dimension: Some(convert::texture_dimension_to_wasi(desc.dimension)),
        format: convert::texture_format_to_wasi(desc.format),
        usage: convert::texture_usages_to_wasi(desc.usage),
        view_formats: Some(
            desc.view_formats
                .iter()
                .map(|format| convert::texture_format_to_wasi(*format))
                .collect(),
        ),
        texture_binding_view_dimension: None,
        label: convert::label(desc.label),
    };
    DispatchTexture::custom(WasdkTexture::new(device.create_texture(&descriptor)))
}

pub(crate) fn texel_copy_texture_to_wasi<'a>(
    info: &wgpu::TexelCopyTextureInfo<'a>,
) -> GpuTexelCopyTextureInfo<'a> {
    GpuTexelCopyTextureInfo {
        texture: WasdkTexture::of(info.texture).gpu_texture(),
        mip_level: Some(info.mip_level),
        origin: Some(convert::origin_to_wasi(info.origin)),
        aspect: Some(convert::texture_aspect_to_wasi(info.aspect)),
    }
}

impl TextureInterface for WasdkTexture {
    fn create_view(&self, desc: &wgpu::TextureViewDescriptor<'_>) -> DispatchTextureView {
        let descriptor = GpuTextureViewDescriptor {
            format: desc.format.map(convert::texture_format_to_wasi),
            dimension: desc.dimension.map(convert::texture_view_dimension_to_wasi),
            usage: desc.usage.map(convert::texture_usages_to_wasi),
            aspect: Some(convert::texture_aspect_to_wasi(desc.aspect)),
            base_mip_level: Some(desc.base_mip_level),
            mip_level_count: desc.mip_level_count,
            base_array_layer: Some(desc.base_array_layer),
            array_layer_count: desc.array_layer_count,
            swizzle: None,
            label: convert::label(desc.label),
        };
        DispatchTextureView::custom(WasdkTextureView {
            view: self.texture.create_view(Some(&descriptor)),
        })
    }

    fn destroy(&self) {
        self.texture.destroy();
    }
}

#[derive(Debug)]
pub struct WasdkTextureView {
    view: GpuTextureView,
}

downcast!(WasdkTextureView: api wgpu::TextureView);

impl WasdkTextureView {
    pub(crate) fn gpu_texture_view(&self) -> &GpuTextureView {
        &self.view
    }
}

impl TextureViewInterface for WasdkTextureView {}
