//! The videos a view shows, as its frames draw them: for each reader a slicing of the scene asked
//! for, the image of its frame on screen. A reader is asked for the frame of the moment the picture
//! reaches the screen, the display time of the refresh being drawn, when a slicing first asks for
//! it, and at each refresh after, which draws the view again when a new one came: as Waft's macOS
//! host asks a player's video output for each refresh's target time.
//!
//! The invariant: a reader's frame is held only while the view's slicings ask for the reader. Once
//! one no longer does, its entry goes, and with it the texture imported of the frame's buffer,
//! which holds the frame's place in its decoder's pool.

use std::cell::RefCell;
use std::sync::Arc;

use waft_embedder::{Image, VideoFrameReader};
use wasdk::views::Instant;

use crate::moments::framework_instant;
use crate::renderer::Renderer;
use crate::targets;

/// The videos one view shows, and the image of the frame each shows now.
#[derive(Default)]
pub(crate) struct ShownVideos {
    shown: RefCell<Vec<ShownVideo>>,
}

/// One reader the view's slicings ask for.
struct ShownVideo {
    reader: Arc<dyn VideoFrameReader>,
    /// The image of the frame on screen; none before the reader's first frame.
    image: Option<Image>,
    /// Whether the slicing being made asked for the reader.
    asked: bool,
}

impl ShownVideos {
    /// A slicing of the view's scene begins: it asks for the readers it shows through
    /// [`image_of`](Self::image_of), and [`end_slicing`](Self::end_slicing) lets go of the others.
    pub(crate) fn begin_slicing(&self) {
        for video in self.shown.borrow_mut().iter_mut() {
            video.asked = false;
        }
    }

    /// The image of the frame `reader` shows in the slicing being made: the one on screen, or,
    /// for a reader no slicing asked for before, its frame for `display_time`.
    pub(crate) fn image_of(
        &self,
        renderer: &Renderer,
        reader: &Arc<dyn VideoFrameReader>,
        display_time: Instant,
    ) -> Option<Image> {
        let mut shown = self.shown.borrow_mut();
        if let Some(video) = shown
            .iter_mut()
            .find(|video| Arc::ptr_eq(&video.reader, reader))
        {
            video.asked = true;
            return video.image.clone();
        }
        let image = frame_image(renderer, reader, display_time);
        shown.push(ShownVideo {
            reader: Arc::clone(reader),
            image: image.clone(),
            asked: true,
        });
        image
    }

    /// The slicing is made: the readers it did not ask for are let go, with their frames.
    pub(crate) fn end_slicing(&self) {
        self.shown.borrow_mut().retain(|video| video.asked);
    }

    /// Whether the view shows a video, whose new frames it takes at each refresh.
    pub(crate) fn shows_video(&self) -> bool {
        !self.shown.borrow().is_empty()
    }

    /// Takes each shown reader's frame for `display_time`, where it is a new one; whether any
    /// was, so the view is drawn again.
    pub(crate) fn take_new_frames(&self, renderer: &Renderer, display_time: Instant) -> bool {
        let mut came = false;
        for video in self.shown.borrow_mut().iter_mut() {
            if let Some(image) = frame_image(renderer, &video.reader, display_time) {
                video.image = Some(image);
                came = true;
            }
        }
        came
    }
}

/// The image of `reader`'s frame for `display_time`, if it is a new one: its buffer imported as a
/// texture on the renderer's device.
fn frame_image(
    renderer: &Renderer,
    reader: &Arc<dyn VideoFrameReader>,
    display_time: Instant,
) -> Option<Image> {
    let frame = reader.frame_for(framework_instant(display_time))?;
    let image = renderer.import_frame(frame.as_ref())?;
    tracing::trace!(target: targets::VIDEO_FRAMES, display_time, "a video's frame to draw");
    Some(image)
}
