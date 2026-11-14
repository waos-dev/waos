//! Vulkan's share of drawing into the system's hardware buffers, where a program's device imports
//! them through Vulkan (Android's `AHardwareBuffer`, Linux's dmabuf): the device opened with the
//! system's import extensions (`device_opener`), and a buffer's image handed to the compositor and
//! taken back (`queue_ownership`). What the import itself is, is each system's.

// Opening a device through wgpu-hal and recording a raw barrier are Vulkan's calls.
#![allow(unsafe_code)]

pub(crate) mod device_opener;
pub(crate) mod queue_ownership;
