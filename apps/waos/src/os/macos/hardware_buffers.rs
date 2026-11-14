//! A program's hardware buffer as WaOS hands it between threads: an `IOSurface`, which a picture
//! the program presented carries from the frame's thread to the main thread when it answers its
//! window's resize, and whose size is the size the picture was drawn at, the size of the canvas the
//! program set as it took the frame's texture.
//!
//! The invariant: the buffer is only carried across threads here, never changed, as Waft's
//! compositor carries one (`CrossThreadBuffer` in its Core Animation layers).

#![allow(unsafe_code)]

use waos::HardwareBuffer;

/// A program's hardware buffer, handed between threads.
pub(crate) struct CrossThreadBuffer(HardwareBuffer);

// SAFETY: an `IOSurface` is made to be shared between threads and processes: its retain count and
// use count are atomic. The binding cannot say so.
unsafe impl Send for CrossThreadBuffer {}

impl CrossThreadBuffer {
    pub(crate) fn new(buffer: HardwareBuffer) -> CrossThreadBuffer {
        CrossThreadBuffer(buffer)
    }

    /// Its width and height, in pixels.
    pub(crate) fn size(&self) -> [u32; 2] {
        [self.0.width(), self.0.height()].map(|side| u32::try_from(side).unwrap_or(u32::MAX))
    }

    /// The buffer, on the thread that shows it.
    pub(crate) fn into_buffer(self) -> HardwareBuffer {
        self.0
    }
}

/// A hardware buffer of `width` by `height` BGRA8 pixels, as the engine allocates one, for a test
/// to present.
#[cfg(test)]
pub(crate) fn test_buffer(width: usize, height: usize) -> HardwareBuffer {
    use objc2_core_foundation::{CFDictionary, CFNumber, CFString};
    use objc2_io_surface::{
        IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat,
        kIOSurfaceWidth,
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
        CFNumber::new_isize(4),
        CFNumber::new_i32(BGRA),
    ];
    let values: [&CFNumber; 4] = [&numbers[0], &numbers[1], &numbers[2], &numbers[3]];
    let properties = CFDictionary::from_slices(&keys, &values);
    // SAFETY: every value is of the type its key names.
    unsafe { IOSurfaceRef::new(properties.as_opaque()) }.expect("an IOSurface for a test")
}
