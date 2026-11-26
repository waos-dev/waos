//! Textures, their views, and external textures, which sample a texture's planes as RGBA.

use wasdk::gpu::{Device, ExternalTexture, TexelCopyTextureInfo, Texture, TextureView};
use wgpu::custom::{
    DispatchExternalTexture, DispatchTexture, DispatchTextureView, ExternalTextureInterface,
    TextureInterface, TextureViewInterface,
};

use crate::convert;

/// The texture behind a [`wgpu::Texture`] from this backend: `texture.as_custom::<WasdkTexture>()`.
#[derive(Debug)]
pub struct WasdkTexture {
    texture: Texture,
}

downcast!(WasdkTexture: api wgpu::Texture, dispatch DispatchTexture);

impl WasdkTexture {
    pub(crate) fn new(texture: Texture) -> Self {
        WasdkTexture { texture }
    }

    /// The engine's texture behind the wgpu one.
    pub fn wit_texture(&self) -> &Texture {
        &self.texture
    }
}

pub(crate) fn create_texture(
    device: &Device,
    desc: &wgpu::TextureDescriptor<'_>,
) -> DispatchTexture {
    let descriptor = convert::texture_descriptor_to_wit(desc);
    DispatchTexture::custom(WasdkTexture::new(device.create_texture(&descriptor)))
}

pub(crate) fn texel_copy_texture_to_wit<'a>(
    info: &wgpu::TexelCopyTextureInfo<'a>,
) -> TexelCopyTextureInfo<'a> {
    TexelCopyTextureInfo {
        texture: WasdkTexture::of(info.texture).wit_texture(),
        mip_level: info.mip_level,
        origin: convert::origin_to_wit(info.origin),
        aspect: convert::texture_aspect_to_wit(info.aspect),
    }
}

impl TextureInterface for WasdkTexture {
    fn create_view(&self, desc: &wgpu::TextureViewDescriptor<'_>) -> DispatchTextureView {
        let descriptor = convert::texture_view_descriptor_to_wit(desc);
        DispatchTextureView::custom(WasdkTextureView {
            view: self.texture.create_view(&descriptor),
        })
    }

    fn destroy(&self) {
        self.texture.destroy();
    }
}

/// The view behind a [`wgpu::TextureView`] from this backend.
#[derive(Debug)]
pub struct WasdkTextureView {
    view: TextureView,
}

downcast!(WasdkTextureView: api wgpu::TextureView);

impl WasdkTextureView {
    pub(crate) fn wit_texture_view(&self) -> &TextureView {
        &self.view
    }
}

impl TextureViewInterface for WasdkTextureView {}

pub(crate) fn create_external_texture(
    device: &Device,
    desc: &wgpu::ExternalTextureDescriptor<'_>,
    planes: &[&wgpu::TextureView],
) -> DispatchExternalTexture {
    let planes: Vec<&TextureView> = (planes.iter())
        .map(|plane| WasdkTextureView::of(plane).wit_texture_view())
        .collect();
    let descriptor = convert::external_texture_descriptor_to_wit(desc);
    DispatchExternalTexture::custom(WasdkExternalTexture {
        texture: device.create_external_texture(&descriptor, &planes),
    })
}

/// The external texture behind a [`wgpu::ExternalTexture`] from this backend.
#[derive(Debug)]
pub struct WasdkExternalTexture {
    texture: ExternalTexture,
}

downcast!(WasdkExternalTexture: api wgpu::ExternalTexture);

impl WasdkExternalTexture {
    pub(crate) fn wit_external_texture(&self) -> &ExternalTexture {
        &self.texture
    }
}

impl ExternalTextureInterface for WasdkExternalTexture {
    fn destroy(&self) {
        self.texture.destroy();
    }
}
