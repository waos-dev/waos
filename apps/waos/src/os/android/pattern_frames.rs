//! A frame of WaOS's simulated camera as Android's host hands a camera's frames out: an RGBA8
//! `AHardwareBuffer`, drawn by the CPU while it is locked, which a frame reader answers and a
//! composition layer shows. The pattern is drawn in BGRA (`test_pattern`), so each pixel's red and
//! blue are swapped as it is written.
//!
//! The invariant: a buffer is written only while it is locked, and handed out only once it is
//! unlocked, so nobody reads a frame half drawn.

// Writing a locked buffer's memory is a C API's; this module writes the `unsafe` for it.
#![allow(unsafe_code)]

use std::io;

use ndk::hardware_buffer::{
    HardwareBuffer as AHardwareBuffer, HardwareBufferDesc, HardwareBufferUsage,
};
use ndk::hardware_buffer_format::HardwareBufferFormat;
use waos::{HardwareBuffer, VideoPicture};

/// Bytes one pixel occupies.
const BYTES_PER_PIXEL: usize = 4;

/// A frame handed between threads: the simulated camera's, drawn on its own thread and read on a
/// program's.
pub(crate) struct CrossThreadFrame(VideoPicture);

impl CrossThreadFrame {
    pub(crate) fn new(frame: VideoPicture) -> CrossThreadFrame {
        CrossThreadFrame(frame)
    }

    /// The frame, one more reference on it.
    pub(crate) fn frame(&self) -> VideoPicture {
        self.0.clone()
    }
}

/// A frame `width` by `height`, its pixels drawn by `draw` in BGRA into rows `row_bytes` apart and
/// written as RGBA, as a reader hands it out; why not, where Android allocated or locked no buffer.
pub(crate) fn pattern_frame(
    width: u32,
    height: u32,
    draw: impl FnOnce(usize, &mut [u8]),
) -> io::Result<VideoPicture> {
    let buffer = AHardwareBuffer::allocate(HardwareBufferDesc {
        width,
        height,
        layers: 1,
        format: HardwareBufferFormat::R8G8B8A8_UNORM,
        usage: HardwareBufferUsage::GPU_SAMPLED_IMAGE
            | HardwareBufferUsage::COMPOSER_OVERLAY
            | HardwareBufferUsage::CPU_WRITE_OFTEN
            | HardwareBufferUsage::CPU_READ_RARELY,
        stride: 0,
    })
    .map_err(|error| {
        io::Error::other(format!(
            "Android allocated no buffer for the test pattern: {error}"
        ))
    })?;
    let row_bytes = buffer.describe().stride as usize * BYTES_PER_PIXEL;
    let address = buffer
        .lock(HardwareBufferUsage::CPU_WRITE_OFTEN, None, None)
        .map_err(|error| {
            io::Error::other(format!("the test pattern's buffer did not lock: {error}"))
        })?;
    // SAFETY: the lock maps the whole buffer, `stride` pixels of 4 bytes a row, `height` rows, for
    // this thread alone to write until the unlock below.
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(address.cast::<u8>(), row_bytes * height as usize)
    };
    draw(row_bytes, pixels);
    swap_red_and_blue(width, row_bytes, pixels);
    let _ = buffer.unlock();
    Ok(VideoPicture::from(buffer))
}

/// The buffer a frame is, which a composition layer shows.
pub(crate) fn surface_of(frame: &VideoPicture) -> Option<HardwareBuffer> {
    Some(frame.clone())
}

/// What `read` reads of `frame`'s pixels, `width` by `height`, given back in BGRA as the pattern
/// was drawn, rows `row_bytes` apart, while the buffer is locked for reading: a test's look at
/// what was drawn.
#[cfg(test)]
pub(crate) fn read_pattern_frame<T>(
    frame: &VideoPicture,
    read: impl FnOnce(u32, u32, usize, &[u8]) -> T,
) -> T {
    let buffer = frame.buffer();
    let description = buffer.describe();
    let row_bytes = description.stride as usize * BYTES_PER_PIXEL;
    let address = buffer
        .lock(HardwareBufferUsage::CPU_READ_RARELY, None, None)
        .expect("the frame's buffer locks");
    // SAFETY: the lock maps the whole buffer for reading; it is copied before the unlock.
    let mut pixels = unsafe {
        std::slice::from_raw_parts(
            address.cast::<u8>(),
            row_bytes * description.height as usize,
        )
    }
    .to_vec();
    let _ = buffer.unlock();
    swap_red_and_blue(description.width, row_bytes, &mut pixels);
    read(description.width, description.height, row_bytes, &pixels)
}

/// Swaps each pixel's first and third bytes, BGRA to RGBA and back.
fn swap_red_and_blue(width: u32, row_bytes: usize, pixels: &mut [u8]) {
    for row in pixels.chunks_exact_mut(row_bytes) {
        for pixel in row[..width as usize * BYTES_PER_PIXEL].chunks_exact_mut(BYTES_PER_PIXEL) {
            pixel.swap(0, 2);
        }
    }
}
