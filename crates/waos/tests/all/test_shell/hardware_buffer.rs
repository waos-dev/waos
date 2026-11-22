//! A hardware buffer as the test shell keeps it across threads, and its pixels. Carrying an
//! `IOSurface` between threads is what every shell on Apple's systems does to show one (Waft's
//! embedder vouches for the same), and reading its pixels is what a test does to check what was
//! drawn.

// Wrapping the system's thread-safety and its lock is what this module is for; the rest of the
// tests have no `unsafe`.
#![allow(unsafe_code)]

use waos::HardwareBuffer;

/// A presented hardware buffer, kept by the shell.
pub struct HeldHardwareBuffer(pub HardwareBuffer);

// SAFETY: an `IOSurface` is a thread-safe Core Foundation object: its retain count is atomic
// and it is made to be passed between processes. Rust cannot see that through the generated
// binding, which is neither.
#[cfg(target_vendor = "apple")]
unsafe impl Send for HeldHardwareBuffer {}
#[cfg(target_vendor = "apple")]
unsafe impl Sync for HeldHardwareBuffer {}

impl HeldHardwareBuffer {
    /// The buffer, on the thread it was carried to.
    pub fn into_buffer(self) -> HardwareBuffer {
        self.0
    }
}

impl Clone for HeldHardwareBuffer {
    /// A hold more on the same memory, not a copy.
    fn clone(&self) -> HeldHardwareBuffer {
        HeldHardwareBuffer(self.0.clone())
    }
}

/// Bytes one pixel occupies: the hardware buffers are BGRA8.
#[cfg(target_vendor = "apple")]
const BYTES_PER_PIXEL: usize = 4;

/// The pixel at (`x`, `y`) of `buffer`, as the bytes in memory: blue, green, red, alpha. The
/// surface is locked for reading meanwhile, so the GPU's writes are visible.
#[cfg(target_vendor = "apple")]
pub fn pixel(buffer: &HeldHardwareBuffer, x: usize, y: usize) -> [u8; 4] {
    use std::ptr;

    use objc2_io_surface::IOSurfaceLockOptions;

    let surface = &*buffer.0;
    // SAFETY: a read-only lock with no seed, released below.
    let locked = unsafe { surface.lock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    assert_eq!(locked, 0, "the surface locks for reading");
    assert!(
        x < surface.width() && y < surface.height(),
        "the pixel is within the hardware buffer"
    );
    let offset = y * surface.bytes_per_row() + x * BYTES_PER_PIXEL;
    // SAFETY: the surface is locked, so its base address is mapped, and the offset is within the
    // row's bytes of a pixel inside the hardware buffer.
    let bytes = unsafe {
        let address = surface.base_address().as_ptr().cast::<u8>().add(offset);
        ptr::read_unaligned(address.cast::<[u8; BYTES_PER_PIXEL]>())
    };
    // SAFETY: unlocks the lock taken above.
    let unlocked = unsafe { surface.unlock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    assert_eq!(unlocked, 0, "the surface unlocks");
    bytes
}

/// A `width` by `height` BGRA `IOSurface`, every pixel `bgra`: a hardware buffer as the hardware
/// codecs decode one.
#[cfg(target_vendor = "apple")]
pub fn surface_filled(width: usize, height: usize, bgra: [u8; 4]) -> HardwareBuffer {
    use std::ptr;

    use objc2_core_foundation::{CFDictionary, CFNumber, CFString};
    use objc2_io_surface::{
        IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight,
        kIOSurfacePixelFormat, kIOSurfaceWidth,
    };

    /// `'BGRA'`, as an `OSType`.
    const BGRA: i32 = 0x4247_5241;
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
        CFNumber::new_isize(BYTES_PER_PIXEL as isize),
        CFNumber::new_i32(BGRA),
    ];
    let values: [&CFNumber; 4] = numbers.each_ref().map(|number| &**number);
    let properties = CFDictionary::from_slices(&keys, &values);
    // SAFETY: every value is of the type its key names.
    let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }.expect("an IOSurface");
    // SAFETY: a lock with no seed, released below; each row is `bytes_per_row` long and holds
    // `width` pixels of four bytes.
    unsafe {
        assert_eq!(surface.lock(IOSurfaceLockOptions(0), ptr::null_mut()), 0);
        let base = surface.base_address().as_ptr().cast::<u8>();
        for row in 0..height {
            for column in 0..width {
                let pixel = base.add(row * surface.bytes_per_row() + column * BYTES_PER_PIXEL);
                ptr::copy_nonoverlapping(bgra.as_ptr(), pixel, BYTES_PER_PIXEL);
            }
        }
        assert_eq!(surface.unlock(IOSurfaceLockOptions(0), ptr::null_mut()), 0);
    }
    surface
}

/// Every pixel of a hardware buffer, read at once: its rows from the top, each pixel's bytes as
/// they are in memory, blue, green, red, alpha.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pixels: Vec<[u8; 4]>,
}

impl Picture {
    /// The pixel at (`x`, `y`).
    pub fn at(&self, x: usize, y: usize) -> [u8; 4] {
        self.pixels[y * self.width + x]
    }
}

/// Every pixel of `buffer`, the surface locked for reading meanwhile, so the GPU's writes are
/// visible.
#[cfg(target_vendor = "apple")]
pub fn picture(buffer: &HeldHardwareBuffer) -> Picture {
    use std::ptr;

    use objc2_io_surface::IOSurfaceLockOptions;

    let surface = &*buffer.0;
    // SAFETY: a read-only lock with no seed, released below.
    let locked = unsafe { surface.lock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    assert_eq!(locked, 0, "the surface locks for reading");
    let (width, height, bytes_per_row) =
        (surface.width(), surface.height(), surface.bytes_per_row());
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            // SAFETY: the surface is locked, so its base address is mapped, and the offset is
            // within the row's bytes of a pixel inside the hardware buffer.
            pixels.push(unsafe {
                let offset = y * bytes_per_row + x * BYTES_PER_PIXEL;
                let address = surface.base_address().as_ptr().cast::<u8>().add(offset);
                ptr::read_unaligned(address.cast::<[u8; BYTES_PER_PIXEL]>())
            });
        }
    }
    // SAFETY: unlocks the lock taken above.
    let unlocked = unsafe { surface.unlock(IOSurfaceLockOptions::ReadOnly, ptr::null_mut()) };
    assert_eq!(unlocked, 0, "the surface unlocks");
    Picture {
        width,
        height,
        pixels,
    }
}
