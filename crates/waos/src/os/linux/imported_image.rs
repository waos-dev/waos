//! A hardware buffer's memory as a texture on a wgpu-core device: a `VkImage` in the dmabuf's DRM
//! format modifier and plane layout (`VK_EXT_image_drm_format_modifier`), bound to memory imported
//! from a duplicate of its fd (`VK_EXT_external_memory_dma_buf`) with a dedicated allocation, which
//! wgpu-hal takes as a texture whose memory it does not own (`TextureMemory::External`) and
//! wgpu-core as one of its own.
//!
//! The invariant: the image, its memory and the buffer under them are held by the wgpu-hal
//! texture's drop callback, which wgpu calls once the GPU is done with the texture, and which
//! destroys the image and frees the memory then; the device outlives them, since wgpu-core's
//! texture holds its device.

use std::borrow::Cow;

use std::os::fd::{AsRawFd, IntoRawFd};

use anyhow::{Context, Result, bail};
use ash::vk;
use wgpu_hal::api::Vulkan;

use crate::gpu::wgpu_types::error::{ErrorType, WebGpuError};
use crate::gpu::{wgpu_core, wgpu_types};
use crate::os::vulkan::device_opener::imports;
use crate::shell::HardwareBuffer;

use super::dmabuf::Dmabuf;

/// The one format a buffer is allocated in, and so the one its image has.
const FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;

/// How a texture over a hardware buffer is used: what wgpu is told, the formats besides its own it
/// is viewed in, and the state it arrives in.
pub(super) struct ImportedTexture<'a> {
    pub(super) label: &'static str,
    pub(super) usage: wgpu_types::TextureUsages,
    pub(super) view_formats: &'a [wgpu_types::TextureFormat],
    pub(super) arrives: wgpu_types::TextureUses,
}

impl ImportedTexture<'_> {
    /// A wgpu texture over `buffer`'s BGRA8 pixels, of `size`, on `device` of `gpu`, through an
    /// image imported over its memory; `release`, where given, is called once wgpu has freed it.
    ///
    /// On a lost device wgpu-core answers an invalid texture, with the loss as its error: the
    /// texture is handed out as it is.
    pub(super) fn over(
        &self,
        buffer: &HardwareBuffer,
        [width, height]: [u32; 2],
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        release: Option<wgpu_hal::DropCallback>,
    ) -> Result<wgpu_core::id::TextureId> {
        let size = wgpu_types::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let hal_texture = self.hal_texture(buffer, size, gpu, device, release)?;
        let descriptor = wgpu_types::TextureDescriptor {
            label: Some(Cow::Borrowed(self.label)),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu_types::TextureDimension::D2,
            format: super::FORMAT,
            usage: self.usage,
            view_formats: self.view_formats.to_vec(),
        };
        // SAFETY: the texture was made on `device` and matches the descriptor, with every usage
        // the descriptor names, and is in the state `arrives` says.
        let (texture, error) = unsafe {
            gpu.create_texture_from_hal(
                Box::new(hal_texture),
                device,
                &descriptor,
                self.arrives,
                None,
            )
        };
        match error {
            Some(error) if error.webgpu_error_type() != ErrorType::DeviceLost => {
                bail!("{error:#?}")
            }
            _ => Ok(texture),
        }
    }

    /// The wgpu-hal texture over an image imported of `buffer` on `device`, which holds the image
    /// and `release` until wgpu frees it.
    fn hal_texture(
        &self,
        buffer: &HardwareBuffer,
        size: wgpu_types::Extent3d,
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        release: Option<wgpu_hal::DropCallback>,
    ) -> Result<wgpu_hal::vulkan::Texture> {
        // SAFETY: the hal device is only read, and is let go before the texture it makes is
        // handed to wgpu-core.
        let hal = unsafe { gpu.device_as_hal::<Vulkan>(device) };
        let Some(hal) = hal else {
            bail!("a hardware buffer needs the program's device to be a Vulkan device")
        };
        if !imports(&hal) {
            bail!(
                "the program's device was opened without the Vulkan extensions that import a hardware buffer"
            )
        }
        let mutable = self
            .view_formats
            .iter()
            .any(|format| *format != super::FORMAT);
        let image = ImportedImage::of(buffer, &hal, image_usage(self.usage), mutable)?;
        let raw = image.image;
        let hal_descriptor = wgpu_hal::TextureDescriptor {
            label: Some(self.label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu_types::TextureDimension::D2,
            format: super::FORMAT,
            usage: hal_uses(self.usage),
            memory_flags: wgpu_hal::MemoryFlags::empty(),
            view_formats: self.view_formats.to_vec(),
        };
        let held = move || {
            drop(image);
            if let Some(release) = release {
                release();
            }
        };
        // SAFETY: `raw` is a live 2D BGRA8 image of exactly this size, with one mip level and one
        // layer, made on the device with every usage the descriptor names and, where it is viewed
        // in other formats, mutable in its format, its memory bound; the drop callback destroys
        // both, so wgpu-hal destroys neither.
        Ok(unsafe {
            hal.texture_from_raw(
                raw,
                &hal_descriptor,
                Some(Box::new(held)),
                wgpu_hal::vulkan::TextureMemory::External,
            )
        })
    }
}

/// A `VkImage` bound to a hardware buffer's imported memory on one device, and the buffer, held
/// until the image and the memory are let go.
struct ImportedImage {
    device: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    _buffer: HardwareBuffer,
}

impl ImportedImage {
    /// The image of `buffer` on `device`, for `usage`, mutable in its format where `mutable`; why
    /// Vulkan made none otherwise.
    fn of(
        buffer: &HardwareBuffer,
        device: &wgpu_hal::vulkan::Device,
        usage: vk::ImageUsageFlags,
        mutable: bool,
    ) -> Result<ImportedImage> {
        let raw = device.raw_device();
        let dmabuf = buffer.dmabuf();
        let image = create_image(raw, dmabuf, usage, mutable)?;
        // SAFETY: a live device and the image just made on it.
        let requirements = unsafe { raw.get_image_memory_requirements(image) };
        let fds =
            ash::khr::external_memory_fd::Device::new(device.shared_instance().raw_instance(), raw);
        let mut fd_properties = vk::MemoryFdPropertiesKHR::default();
        // SAFETY: a live device opened with the extension and a live dmabuf fd.
        let properties = unsafe {
            fds.get_memory_fd_properties(
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                dmabuf.fd.as_raw_fd(),
                &mut fd_properties,
            )
        };
        let memory_types = requirements.memory_type_bits & fd_properties.memory_type_bits;
        if properties.is_err() || memory_types == 0 {
            // SAFETY: the image made above, bound to nothing and used by nothing.
            unsafe { raw.destroy_image(image, None) };
            bail!("the dmabuf's memory is of no type its image takes")
        }
        // Vulkan owns the fd it imports once the allocation succeeds.
        let fd = dmabuf
            .fd
            .try_clone()
            .context("duplicating the dmabuf's fd")?;
        let mut import = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(fd.as_raw_fd());
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let allocation = vk::MemoryAllocateInfo::default()
            .push_next(&mut import)
            .push_next(&mut dedicated)
            .allocation_size(requirements.size)
            .memory_type_index(memory_types.trailing_zeros());
        // SAFETY: a live device; the allocation imports the buffer for the image it is dedicated
        // to, as the extension asks, in a memory type the buffer's properties allow.
        let memory = match unsafe { raw.allocate_memory(&allocation, None) } {
            Ok(memory) => {
                // The allocation took the fd.
                let _ = fd.into_raw_fd();
                memory
            }
            Err(error) => {
                // SAFETY: the image made above, bound to nothing and used by nothing.
                unsafe { raw.destroy_image(image, None) };
                bail!("vkAllocateMemory with a hardware buffer: {error}")
            }
        };
        let imported = ImportedImage {
            device: raw.clone(),
            image,
            memory,
            _buffer: buffer.clone(),
        };
        // SAFETY: the image and its dedicated memory, at offset 0, as the extension asks.
        unsafe { raw.bind_image_memory(image, memory, 0) }.context("vkBindImageMemory")?;
        Ok(imported)
    }
}

impl Drop for ImportedImage {
    fn drop(&mut self) {
        // SAFETY: the image and memory this owns, which wgpu calls this for once the GPU is done
        // with the texture over them.
        unsafe {
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

/// An image over a dmabuf's memory in its modifier and plane layout, for `usage`, mutable in its
/// format where `mutable`, which a view in its sRGB counterpart needs.
fn create_image(
    device: &ash::Device,
    dmabuf: &Dmabuf,
    usage: vk::ImageUsageFlags,
    mutable: bool,
) -> Result<vk::Image> {
    let [width, height] = [dmabuf.width, dmabuf.height];
    let plane = [vk::SubresourceLayout {
        offset: u64::from(dmabuf.offset),
        size: 0,
        row_pitch: u64::from(dmabuf.stride),
        array_pitch: 0,
        depth_pitch: 0,
    }];
    let mut layout = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
        .drm_format_modifier(dmabuf.modifier)
        .plane_layouts(&plane);
    let mut external = vk::ExternalMemoryImageCreateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let flags = if mutable {
        vk::ImageCreateFlags::MUTABLE_FORMAT
    } else {
        vk::ImageCreateFlags::empty()
    };
    let create = vk::ImageCreateInfo::default()
        .push_next(&mut external)
        .push_next(&mut layout)
        .flags(flags)
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    // SAFETY: a live device; the create info and what it points to live across the call.
    unsafe { device.create_image(&create, None) }.context("vkCreateImage over a hardware buffer")
}

/// wgpu-hal's name for what a texture of `usage` is for, as wgpu-core maps a color texture's.
fn hal_uses(usage: wgpu_types::TextureUsages) -> wgpu_types::TextureUses {
    use wgpu_types::{TextureUsages, TextureUses};
    [
        (TextureUsages::COPY_SRC, TextureUses::COPY_SRC),
        (TextureUsages::COPY_DST, TextureUses::COPY_DST),
        (TextureUsages::TEXTURE_BINDING, TextureUses::RESOURCE),
        (TextureUsages::RENDER_ATTACHMENT, TextureUses::COLOR_TARGET),
    ]
    .into_iter()
    .filter(|(texture, _)| usage.contains(*texture))
    .fold(TextureUses::empty(), |all, (_, hal)| all | hal)
}

/// Vulkan's usage for an image a texture of `usage` is over, as wgpu-hal maps it.
fn image_usage(usage: wgpu_types::TextureUsages) -> vk::ImageUsageFlags {
    wgpu_hal::vulkan::conv::map_texture_usage(hal_uses(usage))
}
