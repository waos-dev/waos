//! The system's frame readers as the test shell keeps them: each reader numbered in the order made,
//! keeping the display times it is asked for and handing out a frame a test puts in, once. A frame
//! is a 64 by 48 pixel buffer on an `IOSurface`, BGRA, filled with one colour, as a decoder hands
//! one out on Apple's systems.

// Making and filling a pixel buffer, and carrying it between threads, is what this module is for;
// the rest of the tests have no `unsafe`.
#![allow(unsafe_code)]

use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use waos::{ShellVideoFrameReader, ShellVideoFrameReaders, VideoFrame};

/// A frame's size, in pixels.
pub const FRAME_WIDTH: usize = 64;
pub const FRAME_HEIGHT: usize = 48;

/// Makes readers, numbered from 0 in the order made.
#[derive(Default)]
pub struct TestVideoFrameReaders {
    made: Mutex<Vec<Arc<TestReader>>>,
}

impl TestVideoFrameReaders {
    /// Each reader made so far, in order.
    pub fn made(&self) -> Vec<Arc<TestReader>> {
        self.made.lock().unwrap().clone()
    }
}

impl ShellVideoFrameReaders for TestVideoFrameReaders {
    fn make_reader(&self) -> Arc<dyn ShellVideoFrameReader> {
        let mut made = self.made.lock().unwrap();
        let reader = Arc::new(TestReader {
            number: made.len(),
            asked: Mutex::default(),
            next: Mutex::default(),
        });
        made.push(Arc::clone(&reader));
        reader
    }
}

/// A reader the test readers made.
pub struct TestReader {
    /// Where it comes among the readers made, from 0.
    pub number: usize,
    /// The display times it was asked for, in order.
    asked: Mutex<Vec<Instant>>,
    /// The frame it hands out at the next asking.
    next: Mutex<Option<HeldFrame>>,
}

impl TestReader {
    /// The display times it was asked for so far, in order.
    pub fn asked(&self) -> Vec<Instant> {
        self.asked.lock().unwrap().clone()
    }

    /// A frame filled with `bgra`, the bytes of each pixel as they are in memory, for the reader's
    /// next asking.
    #[cfg(target_vendor = "apple")]
    pub fn put_frame(&self, bgra: [u8; 4]) {
        *self.next.lock().unwrap() = Some(HeldFrame(frame_of(bgra)));
    }
}

impl ShellVideoFrameReader for TestReader {
    fn frame_for(&self, display_time: Instant) -> Option<VideoFrame> {
        self.asked.lock().unwrap().push(display_time);
        self.next.lock().unwrap().take().map(|frame| frame.0)
    }
}

/// The number of the test reader behind `reader`, which the test readers made.
pub fn number_of(reader: Arc<dyn ShellVideoFrameReader>) -> usize {
    let reader: Arc<dyn Any + Send + Sync> = reader;
    reader
        .downcast::<TestReader>()
        .expect("a reader the test readers made")
        .number
}

/// A frame on its way to the engine: waiting in a reader, or in a media asset's answer.
pub struct HeldFrame(VideoFrame);

impl HeldFrame {
    pub fn new(frame: VideoFrame) -> HeldFrame {
        HeldFrame(frame)
    }

    /// The frame, handed over whole.
    pub fn into_frame(self) -> VideoFrame {
        self.0
    }
}

#[cfg(target_vendor = "apple")]
impl Clone for HeldFrame {
    /// A retain, not a copy.
    fn clone(&self) -> HeldFrame {
        HeldFrame(self.0.clone())
    }
}

#[cfg(not(target_vendor = "apple"))]
impl Clone for HeldFrame {
    fn clone(&self) -> HeldFrame {
        match self.0.picture {}
    }
}

// SAFETY: a pixel buffer is a Core Foundation object whose retain count is atomic; the reader only
// keeps it and hands it out. Rust cannot see that through the generated binding, which is neither.
unsafe impl Send for HeldFrame {}

/// A 64 by 48 BGRA pixel buffer on an `IOSurface`, every pixel `bgra`.
#[cfg(target_vendor = "apple")]
pub fn frame_of(bgra: [u8; 4]) -> VideoFrame {
    frame_sized(FRAME_WIDTH, FRAME_HEIGHT, bgra)
}

/// A `width` by `height` BGRA pixel buffer on an `IOSurface`, every pixel `bgra`: a frame.
#[cfg(target_vendor = "apple")]
pub fn frame_sized(width: usize, height: usize, bgra: [u8; 4]) -> VideoFrame {
    use std::ptr::NonNull;

    use objc2_core_foundation::{CFDictionary, CFRetained, CFType};
    use objc2_core_video::{
        CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
        CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
        kCVPixelBufferIOSurfacePropertiesKey, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
    };

    // An empty dictionary of surface properties asks for a buffer on an `IOSurface`.
    let surface_properties = CFDictionary::<CFType, CFType>::empty();
    // SAFETY: the key is a constant CoreVideo exports.
    let key = unsafe { kCVPixelBufferIOSurfacePropertiesKey };
    let attributes = CFDictionary::from_slices(&[key], &[&*surface_properties]);
    let mut buffer = std::ptr::null_mut();
    // SAFETY: attributes of the types their keys name, and `buffer` a valid place for the answer.
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            kCVPixelFormatType_32BGRA,
            Some(attributes.as_opaque()),
            NonNull::from(&mut buffer),
        )
    };
    assert_eq!(status, kCVReturnSuccess, "a pixel buffer is made");
    // SAFETY: a buffer made just now, handed over with its one retain.
    let frame = unsafe { CFRetained::from_raw(NonNull::new(buffer).expect("a pixel buffer")) };
    // SAFETY: locked for writing, its base address mapped until unlocked below; each row is
    // `bytes_per_row` long and holds `width` pixels of four bytes.
    unsafe {
        assert_eq!(
            CVPixelBufferLockBaseAddress(&frame, CVPixelBufferLockFlags(0)),
            kCVReturnSuccess
        );
        let base = CVPixelBufferGetBaseAddress(&frame).cast::<u8>();
        let bytes_per_row = CVPixelBufferGetBytesPerRow(&frame);
        for row in 0..height {
            for column in 0..width {
                let pixel = base.add(row * bytes_per_row + column * 4);
                std::ptr::copy_nonoverlapping(bgra.as_ptr(), pixel, 4);
            }
        }
        CVPixelBufferUnlockBaseAddress(&frame, CVPixelBufferLockFlags(0));
    }
    VideoFrame::upright(frame)
}
