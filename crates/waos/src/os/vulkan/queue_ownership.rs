//! A hardware buffer's image handed between the program's device and the compositor: released to
//! `VK_QUEUE_FAMILY_FOREIGN_EXT` once the work that drew it is submitted, and acquired back before
//! it is drawn into again (decision 8 of `docs/proposals/2026-11-11-android-pictures.md`, as
//! amended). On the emulator a buffer whose last write was a copy shows black without the release
//! and right with it, a change of layout alone not helping.
//!
//! wgpu-core records no transfer to another queue family and offers no way to read the layout it
//! tracks, so each step is two command buffers in one submission: one of wgpu-core's, which moves
//! the texture to a state chosen from its usage ([`handed_state`]) and so to the layout wgpu-hal
//! gives that state, and keeps the texture alive until the submission is done; and one recorded
//! raw (`command_encoder_as_hal_mut`), the barrier that transfers the image, keeping that layout.
//!
//! The invariant: wgpu-core's tracked state of the texture and the image's real layout agree after
//! each submission, so wgpu-core's own barriers after it start from the right layout; the
//! transfer's barrier is the one command wgpu-core does not know of, and changes no layout it
//! tracks.

use std::borrow::Cow;

use anyhow::{Context, Result, anyhow, bail};
use ash::vk;
use waos_gpu_host::{Device, GpuObject};
use wgpu_hal::api::Vulkan;

use crate::gpu::{GpuInstance, wgpu_core, wgpu_types};

/// What labels the handover's command buffers in a GPU debugger.
const LABEL: Option<Cow<'static, str>> = Some(Cow::Borrowed("waos hardware buffer handover"));

/// Which way an image goes between the program's device and the compositor.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Transfer {
    /// To the compositor, after the work that drew it: the image keeps its layout.
    Release,
    /// A new image over memory the compositor had, before it is drawn into: from no layout, its
    /// pixels left undefined, into the one its state gives.
    Acquire,
    /// Back from the compositor, an image released to it earlier: in the layout it was released
    /// in, its pixels kept.
    AcquireBack,
}

/// The state a texture of `usage` is in when it is handed over, which its usage allows: drawn
/// into, the usual last write of a frame; copied into, for a frame a copy writes; sampled; or
/// copied from, which every texture of a hardware buffer allows.
pub(crate) fn handed_state(usage: wgpu_types::TextureUsages) -> wgpu_types::TextureUses {
    use wgpu_types::{TextureUsages, TextureUses};
    if usage.contains(TextureUsages::RENDER_ATTACHMENT) {
        TextureUses::COLOR_TARGET
    } else if usage.contains(TextureUsages::COPY_DST) {
        TextureUses::COPY_DST
    } else if usage.contains(TextureUsages::TEXTURE_BINDING) {
        TextureUses::RESOURCE
    } else {
        TextureUses::COPY_SRC
    }
}

/// Submits on `queue` of `device` the move of `texture` to `state` and the transfer of its image,
/// in the order `transfer` needs: the move first for a release, the transfer first for either
/// acquire. Answers the submission, after everything submitted on the queue before it, what the
/// program wrote on it among that.
pub(crate) fn submit_transfer(
    gpu: &GpuInstance,
    device: &Device,
    texture: wgpu_core::id::TextureId,
    state: wgpu_types::TextureUses,
    transfer: Transfer,
) -> Result<wgpu_core::SubmissionIndex> {
    let moved = moved_to(gpu, device.device_id(), texture, state)?;
    let transferred = transferred(gpu, device.device_id(), texture, state, transfer)?;
    let commands = match transfer {
        Transfer::Release => [moved.id(), transferred.id()],
        Transfer::Acquire | Transfer::AcquireBack => [transferred.id(), moved.id()],
    };
    gpu.queue_submit(device.queue_id(), &commands)
        .map_err(|(_, error)| anyhow!("{error}"))
        .context("submitting a hardware buffer's handover")
}

/// A command buffer that moves `texture` to `state`, wgpu-core recording the barrier.
fn moved_to(
    gpu: &GpuInstance,
    device: wgpu_core::id::DeviceId,
    texture: wgpu_core::id::TextureId,
    state: wgpu_types::TextureUses,
) -> Result<GpuObject<wgpu_core::id::CommandBufferId>> {
    let encoder = encoder(gpu, device)?;
    let transition = wgpu_types::TextureTransition {
        texture,
        selector: None,
        state,
    };
    gpu.command_encoder_transition_resources(
        encoder.id(),
        std::iter::empty(),
        std::iter::once(transition),
    )
    .context("moving a hardware buffer's texture to its handed state")?;
    finished(gpu, &encoder)
}

/// A command buffer holding only the barrier that transfers `texture`'s image `transfer`'s way, in
/// the layout wgpu-hal gives `state`, recorded raw.
fn transferred(
    gpu: &GpuInstance,
    device: wgpu_core::id::DeviceId,
    texture: wgpu_core::id::TextureId,
    state: wgpu_types::TextureUses,
    transfer: Transfer,
) -> Result<GpuObject<wgpu_core::id::CommandBufferId>> {
    // SAFETY: the image's handle is only copied; the texture stays alive through the move's
    // command buffer in the same submission.
    let image = unsafe { gpu.texture_as_hal::<Vulkan>(texture) }
        .map(|texture| unsafe { texture.raw_handle() })
        .context("a hardware buffer's texture is destroyed")?;
    // SAFETY: the hal device is only read, and let go before this returns.
    let (raw_device, family) = {
        let hal = unsafe { gpu.device_as_hal::<Vulkan>(device) }
            .context("a hardware buffer's handover needs a Vulkan device")?;
        (hal.raw_device().clone(), hal.queue_family_index())
    };
    let layout = wgpu_hal::vulkan::conv::derive_image_layout(state, crate::os::FORMAT);
    let barrier = barrier(image, family, layout, transfer);
    let encoder = encoder(gpu, device)?;
    let (source_stage, destination_stage) = match transfer {
        Transfer::Release => (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
        ),
        Transfer::Acquire | Transfer::AcquireBack => (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::ALL_COMMANDS,
        ),
    };
    // SAFETY: the callback records one barrier into the encoder's open command buffer, which
    // wgpu-core then finishes and submits; the image is the texture's, alive across the
    // submission.
    let recorded = unsafe {
        gpu.command_encoder_as_hal_mut::<Vulkan, _, _>(encoder.id(), |encoder| {
            let encoder = encoder?;
            raw_device.cmd_pipeline_barrier(
                encoder.raw_handle(),
                source_stage,
                destination_stage,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            Some(())
        })
    };
    if recorded.is_none() {
        bail!("the device gave no command buffer to record a hardware buffer's handover in")
    }
    finished(gpu, &encoder)
}

/// The barrier that transfers `image`, in `layout`, between queue family `family` and the
/// compositor's, `transfer`'s way.
fn barrier(
    image: vk::Image,
    family: u32,
    layout: vk::ImageLayout,
    transfer: Transfer,
) -> vk::ImageMemoryBarrier<'static> {
    let colour = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let barrier = vk::ImageMemoryBarrier::default()
        .image(image)
        .subresource_range(colour)
        .new_layout(layout);
    match transfer {
        Transfer::Release => barrier
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .old_layout(layout)
            .src_queue_family_index(family)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT),
        Transfer::Acquire | Transfer::AcquireBack => {
            let old_layout = match transfer {
                Transfer::AcquireBack => layout,
                _ => vk::ImageLayout::UNDEFINED,
            };
            barrier
                .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
                .old_layout(old_layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                .dst_queue_family_index(family)
        }
    }
}

/// A command encoder on `device`.
fn encoder(
    gpu: &GpuInstance,
    device: wgpu_core::id::DeviceId,
) -> Result<GpuObject<wgpu_core::id::CommandEncoderId>> {
    let (encoder, error) = gpu.device_create_command_encoder(
        device,
        &wgpu_types::CommandEncoderDescriptor { label: LABEL },
        None,
    );
    let encoder = GpuObject::new(encoder, gpu);
    match error {
        Some(error) => Err(anyhow!("{error}")).context("a hardware buffer's handover encoder"),
        None => Ok(encoder),
    }
}

/// `encoder`'s commands, finished.
fn finished(
    gpu: &GpuInstance,
    encoder: &GpuObject<wgpu_core::id::CommandEncoderId>,
) -> Result<GpuObject<wgpu_core::id::CommandBufferId>> {
    let (commands, error) = gpu.command_encoder_finish(
        encoder.id(),
        &wgpu_types::CommandBufferDescriptor { label: LABEL },
        None,
    );
    let commands = GpuObject::new(commands, gpu);
    match error {
        Some((_, error)) => Err(anyhow!("{error}")).context("a hardware buffer's handover"),
        None => Ok(commands),
    }
}
