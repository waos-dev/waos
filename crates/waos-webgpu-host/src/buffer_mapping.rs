//! Whether a buffer is mapped, kept where both the buffer and its device reach it: WebGPU's
//! `destroy()` unmaps every buffer of the device before it loses the device.
//! https://www.w3.org/TR/webgpu/#dom-gpudevice-destroy
//!
//! The invariant: a buffer that can be mapped, one made with a map usage or mapped at creation, is
//! among its device's `MappableBuffers` for as long as the program holds it.

use std::sync::{Arc, Mutex, Weak};

use wgpu_core::global::Global;
use wgpu_core::id::BufferId;

use crate::gpu_object::GpuObject;
use crate::wasi::webgpu::webgpu;

/// A buffer and whether it is mapped, shared by the program's handle and its device's list.
pub(crate) struct MappableBuffer {
    buffer: GpuObject<BufferId>,
    map_state: Mutex<webgpu::GpuBufferMapState>,
}

impl MappableBuffer {
    pub(crate) fn new(buffer: GpuObject<BufferId>, map_state: webgpu::GpuBufferMapState) -> Self {
        MappableBuffer {
            buffer,
            map_state: Mutex::new(map_state),
        }
    }

    pub(crate) fn id(&self) -> BufferId {
        self.buffer.id()
    }

    pub(crate) fn label(&self) -> &crate::labels::ObjectLabel {
        self.buffer.label()
    }

    pub(crate) fn map_state(&self) -> webgpu::GpuBufferMapState {
        *self.map_state.lock().unwrap()
    }

    pub(crate) fn set_map_state(&self, map_state: webgpu::GpuBufferMapState) {
        *self.map_state.lock().unwrap() = map_state;
    }
}

/// A device's buffers that can be mapped, which it unmaps when it is destroyed.
#[derive(Default)]
pub(crate) struct MappableBuffers(Mutex<Vec<Weak<MappableBuffer>>>);

impl MappableBuffers {
    /// Adds `buffer`, and forgets the buffers the program has let go of.
    pub(crate) fn add(&self, buffer: &Arc<MappableBuffer>) {
        let mut buffers = self.0.lock().unwrap();
        buffers.retain(|buffer| buffer.strong_count() > 0);
        buffers.push(Arc::downgrade(buffer));
    }

    /// Unmaps every buffer that is mapped or waiting to be; a mapping still pending is aborted.
    pub(crate) fn unmap_all(&self, instance: &Global) {
        let buffers = self.0.lock().unwrap();
        for buffer in buffers.iter().filter_map(Weak::upgrade) {
            if buffer.map_state() != webgpu::GpuBufferMapState::Unmapped {
                // The device is about to be lost, and no error is reported from a lost device.
                let _ = instance.buffer_unmap(buffer.id());
                buffer.set_map_state(webgpu::GpuBufferMapState::Unmapped);
            }
        }
    }
}
