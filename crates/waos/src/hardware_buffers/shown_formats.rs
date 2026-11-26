//! The formats of the buffers the system's compositor shows a program's layers in, as the
//! program's `compositor.accepted-formats` answers: those the compositor takes that the engine
//! also makes, in the compositor's order, so the first is shown with no conversion. Where the
//! engine keeps pictures in shared memory, which the compositor maps, it is what the engine makes.
//!
//! The invariant: a format answered is one a program can allocate (`os`'s `unsupported` is none
//! for it), for drawing into and showing.

use crate::BufferKind;
use crate::bindings::wasdk::platform::hardware_buffers::{
    HardwareBufferFormat, HardwareBufferUsage,
};
use crate::os::HardwareBufferMemory;
use crate::shell::AcceptedBuffers;

/// Every format a hardware buffer may be in.
const FORMATS: [HardwareBufferFormat; 4] = [
    HardwareBufferFormat::Bgra8,
    HardwareBufferFormat::Rgba8,
    HardwareBufferFormat::Nv12,
    HardwareBufferFormat::P010,
];

/// The formats a compositor answering `accepted` shows a program's buffers in, when the engine
/// keeps pictures in `kind` of memory.
pub(crate) fn shown_formats(
    accepted: &AcceptedBuffers,
    kind: BufferKind,
) -> Vec<HardwareBufferFormat> {
    let taken: Vec<HardwareBufferFormat> = match kind {
        BufferKind::GpuMemory(_) => accepted
            .gpu_memory
            .iter()
            .flat_map(|memory| memory.formats.iter().map(|format| format.format))
            .collect(),
        BufferKind::SharedMemory => FORMATS.to_vec(),
    };
    // What a buffer a program draws and a layer shows is used for.
    let usage = HardwareBufferUsage::GPU_SAMPLED | HardwareBufferUsage::GPU_COLOR_OUTPUT;
    let mut shown = Vec::new();
    for format in taken {
        if HardwareBufferMemory::unsupported(format, usage).is_none() && !shown.contains(&format) {
            shown.push(format);
        }
    }
    shown
}
