//! Hardware buffers. A view's: the pool a program's frames are drawn into, the texture made over one
//! for each frame, and how a drawn frame reaches the shell. The program draws each frame into one
//! of a few hardware buffers the engine made, and the finished one is handed to the shell as a
//! hardware buffer it shows, rather than presented through a swapchain.
//!
//! The shell says it is done with a hardware buffer only once it finds the compositor no longer
//! using it, which on macOS it asks at the view's next present: the compositor lets go of a
//! hardware buffer about a refresh after the next replaces it (measured: about two refreshes after
//! the GPU finishes the frame, at 60 and at 120 Hz), and says nothing when it does. So, as Flutter's
//! `FlutterSurfaceManager` and Firefox's `SurfacePoolCA` do, a frame never waits for a hardware
//! buffer: a view starts with three, and when a frame is due while the shell holds every one, the
//! pool adds another. Spares go back once the view has drawn nothing for [`SPARE_IDLE`] of the
//! engine's time (`engine_time`), at the first tick after. `present` does not wait for the GPU
//! either: a thread of the view's own waits for the frame, and hands it to the shell once it is
//! drawn, while the program goes on with the next; a view with two frames waiting so is served none
//! (`views::frames_in_flight`), which bounds how far the pool grows behind a slow GPU. A frame the
//! program configured opaque is made opaque on its way, whatever alpha it was drawn with
//! (`alpha_clearer`), since a compositor shows the alpha a buffer holds. While the
//! program is frozen every hardware buffer goes but the one on screen and the one being drawn
//! (`view_hardware_buffers`).
//!
//! A program's own hardware buffers are apart from its views': a buffer it allocates, or an image
//! its hardware codecs decode (`image_codecs`), its bytes counted in the program's stats
//! (`counted_bytes`), and a video frame's, which keeps the frame (`program_buffer`). A buffer
//! answers what it can be used for, which on some systems is more than the program asked; a
//! program that reads one it did not ask to read hears of it once (`unasked_read`). `host` serves
//! `wasdk:platform/hardware-buffers`: a buffer's pixels are copied into the program's memory while
//! the buffer is locked for reading (`os`), once the GPU has finished what the program submitted
//! and wrote to draw into it. A texture of a buffer is made on the program's device over its
//! memory, as a view's frame's is, by `gpu_interop`.
//!
//! The invariant: a hardware buffer the shell holds is never drawn into, nor the one a view keeps
//! as the picture it shows, which is held as the shell's is until the next replaces it
//! (`views::shown_picture`).

mod alpha_clearer;
mod canvas;
mod counted_bytes;
mod finisher;
pub(crate) mod host;
mod pool;
mod program_buffer;
mod shown_formats;
mod unasked_read;
mod view_hardware_buffers;

use std::time::Duration;

pub(crate) use canvas::{Canvases, ViewCanvas};
pub(crate) use pool::Released;
pub use program_buffer::ProgramHardwareBuffer;
pub(crate) use program_buffer::SubmittedWork;
pub(crate) use shown_formats::shown_formats;
pub(crate) use unasked_read::UnaskedRead;

/// How many hardware buffers a view starts with, and keeps once it has drawn nothing for a while:
/// one being drawn, one handed over and not yet shown, one on screen.
const STARTING_HARDWARE_BUFFERS: usize = 3;

/// How long a view draws nothing, on the engine's clock, before its spare hardware buffers go back
/// to the system, at the tick that finds it so.
const SPARE_IDLE: Duration = Duration::from_secs(1);
