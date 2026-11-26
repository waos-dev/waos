//! Shared textures: made on a D3D12 device of the engine's own, which only makes them, on the
//! adapter the compositor shows from, its LUID as the compositor names it, where programs' devices
//! are put too (`gpu::ProgramsAdapter`); on the high-performance adapter where no LUID is named;
//! and opened on a program's device as one of wgpu-core's textures, in the common state every user
//! leaves them in.
//!
//! The invariant: a texture is made on the adapter its readers are on, since a handle opens only
//! on the adapter that made it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::sync::{LazyLock, Mutex};

use anyhow::{Context as _, Result, anyhow, bail};
use wgpu_hal::api::Dx12;
use windows::Win32::Foundation::{GENERIC_ALL, HANDLE};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
    D3D12_RESOURCE_STATE_COMMON, D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12Device, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};
use windows::core::{GUID, IUnknown, PCWSTR, implement};

use crate::gpu::wgpu_types::error::{ErrorType, WebGpuError};
use crate::gpu::{wgpu_core, wgpu_types};
use crate::hardware_buffers::Released;
use crate::shell::{GpuId, HardwareBuffer};

/// The engine's device that makes shared textures, as D3D12 hands it out.
struct Maker(ID3D12Device);

// SAFETY: a D3D12 device is free-threaded: Microsoft documents its methods as callable from any
// thread at once.
unsafe impl Send for Maker {}

/// The makers, one for each adapter, made at its first texture; why not, where it gives no D3D12
/// device.
static MAKERS: LazyLock<Mutex<HashMap<Option<GpuId>, Result<Maker, String>>>> =
    LazyLock::new(Mutex::default);

/// A D3D12 device on the adapter `gpu` names, or wgpu-core's choice at high performance where it
/// names none, on an instance of its own, kept for the process.
fn maker(gpu: Option<GpuId>) -> Result<Maker> {
    let instance = wgpu_core::global::Global::new(
        "waos hardware buffers",
        wgpu_types::InstanceDescriptor {
            backends: wgpu_types::Backends::DX12,
            flags: wgpu_types::InstanceFlags::from_build_config(),
            backend_options: Default::default(),
            memory_budget_thresholds: Default::default(),
            display: None,
        },
        None,
    );
    let adapter = match gpu {
        Some(gpu) => adapter_named(&instance, gpu)
            .with_context(|| format!("no D3D12 adapter of {gpu:?} for hardware buffers"))?,
        None => instance
            .request_adapter(
                &wgpu_types::RequestAdapterOptions {
                    power_preference: wgpu_types::PowerPreference::HighPerformance,
                    ..Default::default()
                },
                wgpu_types::Backends::DX12,
                None,
            )
            .context("no D3D12 adapter for hardware buffers")?,
    };
    let (device, _queue) = instance
        .adapter_request_device(adapter, &Default::default(), None, None)
        .context("no D3D12 device for hardware buffers")?;
    // SAFETY: the hal device is only read, its raw device kept with a reference of its own.
    let raw = unsafe { instance.device_as_hal::<Dx12>(device) }
        .map(|hal| hal.raw_device().clone())
        .context("the hardware buffers' device is not D3D12's")?;
    // The instance and its device live as long as the process, so the raw device stays valid.
    std::mem::forget(instance);
    Ok(Maker(raw))
}

/// The adapter of `instance` that `gpu` names; the others enumerated are dropped again.
fn adapter_named(
    instance: &wgpu_core::global::Global,
    gpu: GpuId,
) -> Option<wgpu_core::id::AdapterId> {
    let adapters = instance.enumerate_adapters(wgpu_types::Backends::DX12, false);
    let named = adapters
        .iter()
        .copied()
        .find(|&adapter| super::adapter_gpu(instance, adapter) == Some(gpu));
    for &adapter in adapters.iter().filter(|&&adapter| Some(adapter) != named) {
        instance.adapter_drop(adapter);
    }
    named
}

/// A BGRA8 texture of `size` pixels, render target and shader resource, in a heap shared with
/// other devices, and its NT handle, which keeps its memory alive.
pub(super) fn make(size: [u32; 2], gpu: Option<GpuId>) -> Result<OwnedHandle> {
    let mut makers = MAKERS.lock().unwrap();
    let maker = makers
        .entry(gpu)
        .or_insert_with(|| maker(gpu).map_err(|error| format!("{error:#}")));
    let maker = maker.as_ref().map_err(|error| anyhow!("{error}"))?;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let description = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(size[0]),
        Height: size[1],
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        ..Default::default()
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: the descriptions outlive the call; the resource it answers is owned here.
    unsafe {
        maker.0.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_SHARED,
            &description,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
    }
    .context("making a shared texture")?;
    let resource = resource.context("D3D12 answered no shared texture")?;
    // SAFETY: the resource is alive; the handle answered is new and this function's to own.
    let handle = unsafe {
        maker
            .0
            .CreateSharedHandle(&resource, None, GENERIC_ALL.0, PCWSTR::null())
    }
    .context("sharing a texture")?;
    // SAFETY: a handle the call just opened, owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// How a texture is opened over a hardware buffer on a device.
pub(super) struct Opened<'a> {
    pub(super) label: &'static str,
    pub(super) usage: wgpu_types::TextureUsages,
    pub(super) view_formats: &'a [wgpu_types::TextureFormat],
}

impl Opened<'_> {
    /// A wgpu-core texture over `buffer`, on `device` of `gpu`, in the common state; `release`,
    /// where given, is called once the texture's resource is let go, when wgpu has freed it.
    ///
    /// On a lost device wgpu-core answers an invalid texture, with the loss as its error: the
    /// texture is handed out as it is.
    pub(super) fn over(
        &self,
        buffer: &HardwareBuffer,
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        release: Option<Released>,
    ) -> Result<wgpu_core::id::TextureId> {
        let [width, height] = buffer.size();
        let size = wgpu_types::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let resource = opened_on(buffer, gpu, device)?;
        if let Some(release) = release {
            release_with(&resource, release)?;
        }
        // SAFETY: wgpu-hal takes the resource as a texture whose memory it does not own, of the
        // size and format the shared texture was made with, one mip, one sample.
        let hal_texture = unsafe {
            wgpu_hal::dx12::Device::texture_from_raw(
                resource,
                super::FORMAT,
                wgpu_types::TextureDimension::D2,
                size,
                1,
                1,
            )
        };
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
        // SAFETY: the texture is the resource just opened, of the descriptor's size and format,
        // one mip and one sample, in the common state every user leaves it in, `PRESENT` to
        // wgpu-hal on D3D12.
        let (texture, error) = unsafe {
            gpu.create_texture_from_hal(
                Box::new(hal_texture),
                device,
                &descriptor,
                wgpu_types::TextureUses::PRESENT,
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
}

/// `buffer`'s texture, opened on `device` of `gpu`.
fn opened_on(
    buffer: &HardwareBuffer,
    gpu: &wgpu_core::global::Global,
    device: wgpu_core::id::DeviceId,
) -> Result<ID3D12Resource> {
    // SAFETY: the hal device is only read, and let go before this returns.
    let hal = unsafe { gpu.device_as_hal::<Dx12>(device) }
        .context("a hardware buffer needs the program's device to be a D3D12 device")?;
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: the handle is the buffer's, alive while the buffer is; the resource answered is
    // owned here.
    unsafe {
        hal.raw_device()
            .OpenSharedHandle(HANDLE(buffer.handle().as_raw_handle()), &mut resource)
    }
    .context(
        "opening a hardware buffer on the program's device, which may be on another adapter",
    )?;
    resource.context("D3D12 answered no texture for a hardware buffer")
}

/// What tells D3D12 which private data of a resource is the engine's release.
const RELEASE_DATA: GUID = GUID::from_u128(0x5a8f3c41_0b7e_4d2a_9c61_3e7f1d2b8a90);

/// Has `release` called once `resource` is destroyed: D3D12 lets go of an interface set as an
/// object's private data when the object goes, which is when wgpu has freed the texture over it.
fn release_with(resource: &ID3D12Resource, release: Released) -> Result<()> {
    let holder: IUnknown = ReleaseOnDrop(Mutex::new(Some(release))).into();
    // SAFETY: the GUID outlives the call; D3D12 holds a reference on the holder until the
    // resource goes.
    unsafe { resource.SetPrivateDataInterface(&RELEASE_DATA, &holder) }
        .context("holding a hardware buffer while its texture lives")
}

/// A COM object that calls its release when its last reference goes.
#[implement(IAgileObject)]
struct ReleaseOnDrop(Mutex<Option<Released>>);

impl IAgileObject_Impl for ReleaseOnDrop_Impl {}

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if let Some(release) = self.0.get_mut().unwrap().take() {
            release();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_texture_shared_here_opens_on_another_device_and_lets_go_once_wgpu_frees_it() {
        let size = [64, 32];
        let buffer = HardwareBuffer::new(make(size).unwrap(), size);
        let gpu = crate::gpu::instance();
        let options = wgpu_types::RequestAdapterOptions {
            power_preference: wgpu_types::PowerPreference::HighPerformance,
            ..Default::default()
        };
        let adapter = gpu
            .request_adapter(&options, wgpu_types::Backends::DX12, None)
            .unwrap();
        let (device, _queue) = gpu
            .adapter_request_device(adapter, &Default::default(), None, None)
            .unwrap();
        let (release, released) = mpsc::channel();
        let opened = Opened {
            label: "test shared texture",
            usage: wgpu_types::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };
        let release = Box::new(move || release.send(()).unwrap());
        let texture = opened.over(&buffer, &gpu, device, Some(release)).unwrap();
        assert!(released.try_recv().is_err(), "held while the texture lives");

        gpu.texture_drop(texture);
        gpu.device_poll(device, wgpu_types::PollType::wait_indefinitely())
            .unwrap();
        released
            .recv_timeout(Duration::from_secs(5))
            .expect("let go once wgpu freed the texture");
    }
}
