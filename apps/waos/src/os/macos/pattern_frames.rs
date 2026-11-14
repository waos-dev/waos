//! A frame of WaOS's simulated camera as macOS hands a camera's frames out: an `IOSurface` of
//! 32-bit BGRA, drawn by the CPU while it is locked, wrapped in the `CVPixelBuffer` a frame reader
//! answers, whose surface a composition layer shows.
//!
//! The invariant: a surface is written only while it is locked, and handed out only once it is
//! unlocked, so nobody reads a frame half drawn.

#![allow(unsafe_code)]

use std::io;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFDictionary, CFNumber, CFString};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreateWithIOSurface, CVPixelBufferGetIOSurface,
};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use waos::{HardwareBuffer, VideoPicture};

/// A frame handed between threads: the simulated camera's, drawn on its own thread and read on a
/// program's.
pub(crate) struct CrossThreadFrame(VideoPicture);

// SAFETY: a `CVPixelBuffer`'s retain count is atomic, and a frame drawn is never written again, so
// any thread may hold it and read it. The binding cannot say so.
unsafe impl Send for CrossThreadFrame {}
// SAFETY: as above: nothing changes a drawn frame, so it is read from any thread at once.
unsafe impl Sync for CrossThreadFrame {}

impl CrossThreadFrame {
    pub(crate) fn new(frame: VideoPicture) -> CrossThreadFrame {
        CrossThreadFrame(frame)
    }

    /// The frame, one more reference on it.
    pub(crate) fn frame(&self) -> VideoPicture {
        self.0.clone()
    }
}

/// `'BGRA'`, as an `OSType`.
const BGRA: i32 = 0x4247_5241;

/// A frame `width` by `height`, its pixels drawn by `draw` into rows `row_bytes` apart, as a
/// reader hands it out; why not, where macOS made no surface or pixel buffer.
pub(crate) fn pattern_frame(
    width: u32,
    height: u32,
    draw: impl FnOnce(usize, &mut [u8]),
) -> io::Result<VideoPicture> {
    let surface = surface(width, height)?;
    draw_into(&surface, draw)?;
    pixel_buffer_on(&surface)
}

/// The surface a frame is on, which a composition layer shows.
pub(crate) fn surface_of(frame: &VideoPicture) -> Option<HardwareBuffer> {
    CVPixelBufferGetIOSurface(Some(frame))
}

/// A new `IOSurface` of `width` by `height` BGRA pixels.
fn surface(width: u32, height: u32) -> io::Result<HardwareBuffer> {
    // SAFETY: the keys are Core Foundation constants the framework exports.
    let keys: [&CFString; 4] = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfacePixelFormat,
        ]
    };
    let numbers = [
        CFNumber::new_isize(width as isize),
        CFNumber::new_isize(height as isize),
        CFNumber::new_isize(4),
        CFNumber::new_i32(BGRA),
    ];
    let values: [&CFNumber; 4] = [&numbers[0], &numbers[1], &numbers[2], &numbers[3]];
    let properties = CFDictionary::from_slices(&keys, &values);
    // SAFETY: every value is of the type its key names.
    unsafe { IOSurfaceRef::new(properties.as_opaque()) }
        .ok_or_else(|| io::Error::other("macOS made no IOSurface for the test pattern"))
}

/// Draws into `surface`'s pixels while it is locked for writing.
fn draw_into(surface: &IOSurfaceRef, draw: impl FnOnce(usize, &mut [u8])) -> io::Result<()> {
    let options = IOSurfaceLockOptions::empty();
    // SAFETY: a null seed is allowed; the lock is undone below before the surface is handed out.
    let locked = unsafe { surface.lock(options, ptr::null_mut()) };
    if locked != 0 {
        return Err(io::Error::other(format!(
            "the test pattern's IOSurface did not lock: {locked}"
        )));
    }
    let row_bytes = surface.bytes_per_row();
    let length = row_bytes * surface.height();
    // SAFETY: while locked, the base address holds `bytes_per_row * height` bytes this thread
    // alone writes.
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(surface.base_address().as_ptr().cast::<u8>(), length)
    };
    draw(row_bytes, pixels);
    // SAFETY: the surface was locked above with the same options and seed.
    unsafe { surface.unlock(options, ptr::null_mut()) };
    Ok(())
}

/// The `CVPixelBuffer` over `surface`, as a camera's frame is.
fn pixel_buffer_on(surface: &IOSurfaceRef) -> io::Result<VideoPicture> {
    let mut made: *mut CVPixelBuffer = ptr::null_mut();
    // SAFETY: `made` is a valid place for the buffer's pointer, and no attributes are passed.
    let returned =
        unsafe { CVPixelBufferCreateWithIOSurface(None, surface, None, NonNull::from(&mut made)) };
    let made = NonNull::new(made)
        .filter(|_| returned == 0)
        .ok_or_else(|| {
            io::Error::other(format!(
                "Core Video made no pixel buffer over the test pattern: {returned}"
            ))
        })?;
    // SAFETY: the buffer was created above, so this holds its one reference.
    Ok(unsafe { objc2_core_foundation::CFRetained::from_raw(made) })
}

/// What `read` reads of `frame`'s pixels, `width` by `height` BGRA, rows `row_bytes` apart, while
/// its surface is locked for reading: a test's look at what was drawn.
#[cfg(test)]
pub(crate) fn read_pattern_frame<T>(
    frame: &VideoPicture,
    read: impl FnOnce(u32, u32, usize, &[u8]) -> T,
) -> T {
    let surface = surface_of(frame).expect("a frame on a surface");
    let options = IOSurfaceLockOptions::ReadOnly;
    // SAFETY: a null seed is allowed; the lock is undone below.
    let locked = unsafe { surface.lock(options, ptr::null_mut()) };
    assert_eq!(locked, 0, "the frame's surface locks");
    let row_bytes = surface.bytes_per_row();
    let length = row_bytes * surface.height();
    // SAFETY: while locked, the base address holds `bytes_per_row * height` bytes, read only.
    let pixels =
        unsafe { std::slice::from_raw_parts(surface.base_address().as_ptr().cast::<u8>(), length) };
    let read = read(
        surface.width() as u32,
        surface.height() as u32,
        row_bytes,
        pixels,
    );
    // SAFETY: the surface was locked above with the same options and seed.
    unsafe { surface.unlock(options, ptr::null_mut()) };
    read
}

#[cfg(test)]
mod tests {
    use objc2_core_video::{CVPixelBufferGetHeight, CVPixelBufferGetWidth};

    use super::*;

    #[test]
    fn a_frame_is_a_pixel_buffer_on_a_surface_of_its_size_holding_what_was_drawn() {
        let frame = pattern_frame(64, 48, |row_bytes, pixels| {
            pixels[..4].copy_from_slice(&[1, 2, 3, 255]);
            assert!(row_bytes >= 64 * 4);
        })
        .unwrap();
        assert_eq!(
            (
                CVPixelBufferGetWidth(&frame),
                CVPixelBufferGetHeight(&frame)
            ),
            (64, 48)
        );
        let surface = surface_of(&frame).expect("on a surface");
        assert_eq!((surface.width(), surface.height()), (64, 48));
    }
}
