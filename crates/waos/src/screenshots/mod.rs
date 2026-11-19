//! A view's screenshot: what it shows on screen, drawn again at its size, for a tool that looks
//! at the program's window (the design's decisions 60 and 62). The program's last picture lies over
//! the layers it was presented with, each asked what it shows now
//! ([`ShellCompositionLayer::current_picture`](crate::shell::ShellCompositionLayer)): the buffer
//! presented to it last, or the frame of the video or the camera on it. They are drawn together on
//! the GPU, on a device of the engine's own (`device`), each where its placement puts it in the
//! program's picture, beneath the drawing, the bottom-most first (`quads`), the same on every
//! platform, and the result is read back as the pixels of a hardware buffer: BGRA, premultiplied.
//! Not the system compositor's own picture of the window, which only Apple's systems would draw
//! offscreen.
//!
//! The invariant a later editor will break: a picture read is held from whoever would draw over
//! it until the GPU has read it, a buffer a program presented marked in use, as the compositor
//! marks one it shows, so its layer does not give it back to the program meanwhile, and a frame
//! held as its frame, so its pool does not hand its memory out again.

mod device;
mod quads;

use std::sync::Mutex;

use crate::HardwareBufferMode;
use crate::bindings::wasdk::platform::hardware_buffers::{
    HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer,
};
use crate::bindings::wasdk::platform::views::Placement;
use crate::gpu::GpuInstance;
use crate::os::{FrameMemory, HardwareBufferMemory, InUse};
use crate::shell::LayerPicture;
use crate::sync::locked;
use crate::views::PlacedShellLayer;

use device::ScreenshotDevice;
use quads::{Fit, Quad, Turn};

/// What draws the engine's screenshots: its GPU instance, and the device opened on it for them,
/// once one is taken.
pub(crate) struct Screenshots {
    gpu: GpuInstance,
    hardware_buffers: HardwareBufferMode,
    /// None before the first screenshot, and after a draw that failed, which opens it again.
    device: Mutex<Option<ScreenshotDevice>>,
}

/// A layer's picture as a screenshot reads it: its memory, how it fills the layer, and what holds
/// it as it is while it is read.
struct HeldPicture {
    memory: HardwareBufferMemory,
    fit: Fit,
    /// How it is shown: a frame's turn and mirror, as the screen shows it.
    turn: Turn,
    _held: Held,
}

/// What holds a layer's picture as it is, for as long as it is held.
enum Held {
    /// A buffer presented to the layer, marked in use so the layer does not give it back.
    Buffer { _in_use: InUse },
    /// A video's frame, which keeps its memory from its pool.
    Frame { _frame: FrameMemory },
}

impl Screenshots {
    pub(crate) fn new(gpu: GpuInstance, hardware_buffers: HardwareBufferMode) -> Screenshots {
        Screenshots {
            gpu,
            hardware_buffers,
            device: Mutex::new(None),
        }
    }

    /// `drawing`, a view's picture, over the pictures of `layers`, the layers it was presented
    /// with, the bottom-most first, in a picture `size` pixels wide and high: BGRA, premultiplied.
    /// Blocks while the GPU draws it.
    pub(crate) fn take(
        &self,
        size: [u32; 2],
        drawing: &HardwareBufferMemory,
        layers: &[PlacedShellLayer],
    ) -> Result<PixelBuffer, ImageError> {
        let pictures = pictures_of(layers);
        let quads = quads_of(&pictures, drawing);
        let target =
            HardwareBufferMemory::new(size[0], size[1], target_usage(), self.hardware_buffers)
                .map_err(failed)?;
        self.draw(&target, &quads).map_err(failed)?;
        target.read_pixels()
    }

    /// Draws `quads` into `target` on the device, opening it first where it is not.
    fn draw(
        &self,
        target: &HardwareBufferMemory,
        quads: &[(Quad, &HardwareBufferMemory)],
    ) -> anyhow::Result<()> {
        let mut device = locked(&self.device);
        let opened = match device.take() {
            Some(opened) => opened,
            None => ScreenshotDevice::open(&self.gpu, self.hardware_buffers)?,
        };
        opened.draw(&self.gpu, target, quads)?;
        // Kept only once it drew, so a device that failed, lost among them, is opened again.
        *device = Some(opened);
        Ok(())
    }
}

/// What each of `layers` shows now, held while it is read, with where the layer is placed; a
/// layer that shows nothing is left out.
fn pictures_of(layers: &[PlacedShellLayer]) -> Vec<(HeldPicture, &Placement)> {
    layers
        .iter()
        .filter_map(|(layer, placement)| Some((held(layer.current_picture()?)?, placement)))
        .collect()
}

/// The quads of `pictures`, the bottom-most first, each with the memory its picture lies in, those
/// nothing lets show left out, then `drawing`'s over them.
fn quads_of<'a>(
    pictures: &'a [(HeldPicture, &Placement)],
    drawing: &'a HardwareBufferMemory,
) -> Vec<(Quad, &'a HardwareBufferMemory)> {
    let layers = pictures.iter().filter_map(|(picture, placement)| {
        let size = [picture.memory.width(), picture.memory.height()];
        Some((
            Quad::layer(placement, size, picture.fit, picture.turn)?,
            &picture.memory,
        ))
    });
    let drawn = (Quad::drawing([drawing.width(), drawing.height()]), drawing);
    layers.chain([drawn]).collect()
}

/// `picture`, a layer's, held while it is read; none, said so, for a frame whose memory is not a
/// hardware buffer.
fn held(picture: LayerPicture) -> Option<HeldPicture> {
    match picture {
        LayerPicture::Buffer(buffer) => {
            let memory = HardwareBufferMemory::of(buffer);
            Some(HeldPicture {
                _held: Held::Buffer {
                    _in_use: memory.in_use(),
                },
                memory,
                fit: Fit::Fill,
                turn: Turn::default(),
            })
        }
        LayerPicture::Frame(frame) => {
            let turn = Turn {
                rotation: frame.rotation,
                flip: frame.flip,
            };
            let frame = FrameMemory::new(frame.picture);
            let memory = frame
                .hardware_buffer()
                .inspect_err(|error| tracing::warn!("a layer's frame is left out: {error:#}"))
                .ok()?;
            Some(HeldPicture {
                memory,
                fit: Fit::Contain,
                turn,
                _held: Held::Frame { _frame: frame },
            })
        }
    }
}

/// What a screenshot's own hardware buffer is allocated for: drawn into on the GPU, then read by
/// the CPU.
fn target_usage() -> HardwareBufferUsage {
    HardwareBufferUsage::GPU_COLOR_OUTPUT | HardwareBufferUsage::CPU_READ
}

/// `error`, the engine's, as a screenshot's failure.
fn failed(error: anyhow::Error) -> ImageError {
    ImageError {
        kind: ImageErrorKind::Other,
        message: format!("drawing the screenshot: {error:#}"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use super::Screenshots;

    /// What draws screenshots on an instance of its own, which opens no device until one is
    /// taken: for a test's views.
    pub(crate) fn screenshots() -> Arc<Screenshots> {
        let mode = if cfg!(target_os = "linux") {
            super::HardwareBufferMode::Offscreen
        } else {
            super::HardwareBufferMode::Auto
        };
        Arc::new(Screenshots::new(crate::gpu::instance(), mode))
    }

    /// Screenshots drawn of pictures the tests know, where the engine makes hardware buffers.
    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    mod drawn {
        use super::*;
        use crate::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
        use crate::bindings::wasdk::platform::views::{Mutator, Placement, RoundedRect};
        use crate::bindings::wasdk::platform::windows::{Point, Rect, Size};
        use crate::os::{HardwareBufferMemory, filled};
        use crate::shell::{HardwareBuffer, LayerPicture, ShellCompositionLayer};
        use crate::views::PlacedShellLayer;

        /// Pixels as their bytes lie in a BGRA buffer.
        const RED: [u8; 4] = [0, 0, 255, 255];
        const GREEN: [u8; 4] = [0, 255, 0, 255];
        const GREY: [u8; 4] = [64, 64, 64, 255];
        const CLEAR: [u8; 4] = [0, 0, 0, 0];

        /// The screenshot's size.
        const SIZE: [u32; 2] = [20, 10];

        /// A layer showing a buffer of one colour, or nothing.
        struct Showing(Option<HardwareBufferMemory>);

        impl ShellCompositionLayer for Showing {
            fn present(&self, _buffer: HardwareBuffer, released: Box<dyn FnOnce() + Send + Sync>) {
                released();
            }

            fn current_picture(&self) -> Option<LayerPicture> {
                Some(LayerPicture::Buffer(self.0.as_ref()?.buffer()))
            }
        }

        fn showing(bgra: [u8; 4]) -> Arc<dyn ShellCompositionLayer> {
            Arc::new(Showing(Some(filled(4, 4, |_, _| bgra))))
        }

        fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
            Rect {
                origin: Point { x, y },
                size: Size { width, height },
            }
        }

        fn scale(factor: f32) -> Mutator {
            Mutator::Transform(vec![
                factor, 0.0, 0.0, 0.0, //
                0.0, factor, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ])
        }

        /// The whole screenshot, under `mutators`.
        fn over_all(mutators: Vec<Mutator>) -> Placement {
            Placement {
                rect: rect(0.0, 0.0, 20.0, 10.0),
                mutators,
            }
        }

        /// A drawing grey from column 15 on, and clear before it, where the layers show.
        fn drawing() -> HardwareBufferMemory {
            filled(SIZE[0], SIZE[1], |x, _| if x >= 15 { GREY } else { CLEAR })
        }

        fn taken(layers: &[PlacedShellLayer]) -> PixelBuffer {
            let screenshot = screenshots().take(SIZE, &drawing(), layers).unwrap();
            assert_eq!((screenshot.width, screenshot.height), (SIZE[0], SIZE[1]));
            screenshot
        }

        fn at(screenshot: &PixelBuffer, x: usize, y: usize) -> [u8; 4] {
            let at = y * screenshot.row_bytes as usize + x * 4;
            screenshot.bytes[at..at + 4].try_into().unwrap()
        }

        #[test]
        fn a_layer_shows_beneath_the_drawing_where_its_transform_puts_it_cut_by_its_clip() {
            let placement = Placement {
                rect: rect(0.0, 0.0, 10.0, 5.0),
                mutators: vec![Mutator::ClipRect(rect(0.0, 0.0, 10.0, 10.0)), scale(2.0)],
            };
            let screenshot = taken(&[(showing(RED), placement)]);
            assert_eq!(at(&screenshot, 2, 5), RED, "stretched over its rectangle");
            assert_eq!(at(&screenshot, 9, 9), RED);
            assert_eq!(at(&screenshot, 12, 5), CLEAR, "clipped");
            assert_eq!(at(&screenshot, 17, 5), GREY, "the drawing over it");
        }

        #[test]
        fn a_rounded_clip_cuts_the_corners_and_an_opacity_hides_only_at_none() {
            let corner = Size {
                width: 6.0,
                height: 6.0,
            };
            let rounded = Mutator::ClipRoundedRect(RoundedRect {
                rect: rect(0.0, 0.0, 20.0, 10.0),
                top_left: corner,
                top_right: corner,
                bottom_right: corner,
                bottom_left: corner,
            });
            let screenshot = taken(&[(
                showing(GREEN),
                over_all(vec![rounded, Mutator::Opacity(128)]),
            )]);
            assert_eq!(at(&screenshot, 0, 0), CLEAR, "outside the corner");
            assert_eq!(at(&screenshot, 0, 9), CLEAR);
            assert_eq!(
                at(&screenshot, 10, 5),
                GREEN,
                "whole, the drawing above carrying the opacity"
            );
            let hidden = taken(&[(showing(GREEN), over_all(vec![Mutator::Opacity(0)]))]);
            assert_eq!(at(&hidden, 10, 5), CLEAR);
        }

        #[test]
        fn the_layers_lie_in_order_the_first_beneath() {
            let narrow = Placement {
                rect: rect(5.0, 0.0, 5.0, 10.0),
                mutators: Vec::new(),
            };
            let screenshot = taken(&[
                (showing(RED), over_all(Vec::new())),
                (showing(GREEN), narrow),
            ]);
            assert_eq!(at(&screenshot, 2, 5), RED);
            assert_eq!(at(&screenshot, 7, 5), GREEN, "above the first");
        }

        #[test]
        fn a_layer_showing_nothing_leaves_the_drawing_as_it_is() {
            let nothing: Arc<dyn ShellCompositionLayer> = Arc::new(Showing(None));
            let screenshot = taken(&[(nothing, over_all(Vec::new()))]);
            assert_eq!(at(&screenshot, 2, 5), CLEAR);
            assert_eq!(at(&screenshot, 17, 5), GREY);
        }

        #[test]
        fn the_drawing_lies_at_its_own_size_in_a_screenshot_at_the_views() {
            let small = filled(4, 4, |_, _| GREY);
            let screenshot = screenshots().take(SIZE, &small, &[]).unwrap();
            assert_eq!((screenshot.width, screenshot.height), (SIZE[0], SIZE[1]));
            assert_eq!(at(&screenshot, 3, 3), GREY);
            assert_eq!(at(&screenshot, 4, 4), CLEAR, "beyond the drawing");
        }
    }
}
