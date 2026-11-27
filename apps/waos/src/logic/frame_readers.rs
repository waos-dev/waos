//! The programs' frame readers over Waft's: each the engine's at once, as a program makes it, and
//! Waft's on the main thread the first time a player or a camera is given it, since Waft's
//! readers are made where its platform is, while a reader is read from any thread, as Waft's are;
//! or else fed by WaOS itself, frame by frame, when the camera given it is WaOS's simulated one
//! (`simulated_cameras`).
//! A frame Waft's reader answers is handed to the engine as the platform's own, on Apple's systems
//! the pixel buffer behind Waft's Core Video frame and on Android the `AHardwareBuffer` behind
//! Waft's Android frame, as a media asset's picture is (`state::media_assets`).
//!
//! The invariant: a reader has one source at most, Waft's or WaOS's, given it once, so a source
//! given it twice sends its frames to one reader; before it has one, it answers no frame, as a
//! reader fed by nothing answers none.

use std::sync::{Arc, OnceLock};
#[cfg(any(target_os = "macos", target_os = "android"))]
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use waft::ui;
#[cfg(any(target_vendor = "apple", target_os = "android"))]
use waos::VideoPicture;
use waos::{ShellVideoFrameReader, ShellVideoFrameReaders, VideoFrame};

#[cfg(any(target_os = "macos", target_os = "android"))]
use crate::os::CrossThreadFrame;

/// The app's frame readers, as the engine reaches them.
pub(crate) struct ReadersOnRequest;

impl ShellVideoFrameReaders for ReadersOnRequest {
    fn make_reader(&self) -> Arc<dyn ShellVideoFrameReader> {
        Arc::new(ReaderOnRequest::default())
    }
}

/// One reader a program made: Waft's once a player is given it, or WaOS's once its simulated
/// camera is.
#[derive(Default)]
pub(crate) struct ReaderOnRequest {
    source: OnceLock<Source>,
}

/// Where a reader's frames come from.
enum Source {
    Waft(Arc<dyn ui::VideoFrameReader>),
    /// WaOS's simulated camera's, on macOS and Android, where it is built.
    #[cfg(any(target_os = "macos", target_os = "android"))]
    Waos(Arc<FedFrames>),
}

/// The frames WaOS feeds a reader: the newest, and whether it was answered already.
#[cfg(any(target_os = "macos", target_os = "android"))]
#[derive(Default)]
pub(crate) struct FedFrames(Mutex<Option<(CrossThreadFrame, bool)>>);

#[cfg(any(target_os = "macos", target_os = "android"))]
impl FedFrames {
    /// `frame` is the newest, which the reader answers next.
    pub(crate) fn feed(&self, frame: CrossThreadFrame) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((frame, false));
    }

    /// The newest frame, unless it was answered already.
    fn take_new(&self) -> Option<VideoFrame> {
        let mut newest = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (frame, answered) = newest.as_mut()?;
        (!std::mem::replace(answered, true)).then(|| VideoFrame::upright(frame.frame()))
    }
}

impl ReaderOnRequest {
    /// Waft's reader behind this one, made on `readers` the first time it is asked for; none for
    /// a reader WaOS feeds. Called on the main thread, where Waft's readers are.
    pub(crate) fn waft_reader(
        &self,
        readers: &dyn ui::VideoFrameReaders,
    ) -> Option<Arc<dyn ui::VideoFrameReader>> {
        match self
            .source
            .get_or_init(|| Source::Waft(readers.make_reader()))
        {
            Source::Waft(reader) => Some(Arc::clone(reader)),
            #[cfg(any(target_os = "macos", target_os = "android"))]
            Source::Waos(_) => None,
        }
    }

    /// What WaOS feeds this reader with, made the first time it is asked for; none for a reader
    /// Waft's already is.
    #[cfg(any(target_os = "macos", target_os = "android"))]
    pub(crate) fn fed_by_waos(&self) -> Option<Arc<FedFrames>> {
        match self.source.get_or_init(|| Source::Waos(Arc::default())) {
            Source::Waos(fed) => Some(Arc::clone(fed)),
            Source::Waft(_) => None,
        }
    }
}

impl ShellVideoFrameReader for ReaderOnRequest {
    fn frame_for(&self, display_time: Instant) -> Option<VideoFrame> {
        match self.source.get()? {
            Source::Waft(reader) => engine_frame(reader.frame_for(display_time)?.as_ref()),
            #[cfg(any(target_os = "macos", target_os = "android"))]
            Source::Waos(fed) => fed.take_new(),
        }
    }
}

/// Waft's `frame` as the engine's: the pixel buffer behind Waft's Core Video frame, which it
/// holds too, with how it is shown; none, said so, for a frame of another kind.
#[cfg(target_vendor = "apple")]
pub(crate) fn engine_frame(frame: &dyn ui::VideoFrame) -> Option<VideoFrame> {
    let frame: &dyn std::any::Any = frame;
    let Some(frame) = frame.downcast_ref::<waft_embedder_apple::media::CoreVideoFrame>() else {
        tracing::warn!("a reader answered a frame that is not Core Video's; it is dropped");
        return None;
    };
    Some(shown_as(frame, VideoPicture::from(frame.pixel_buffer())))
}

/// Waft's `frame` as the engine's: the RGBA8 `AHardwareBuffer` behind Waft's Android frame, a
/// reference more on it, with how it is shown; none, said so, for a frame of another kind.
#[cfg(target_os = "android")]
pub(crate) fn engine_frame(frame: &dyn ui::VideoFrame) -> Option<VideoFrame> {
    let frame: &dyn std::any::Any = frame;
    let Some(frame) = frame.downcast_ref::<waft_embedder_android::AndroidVideoFrame>() else {
        tracing::warn!("a reader answered a frame that is not Android's; it is dropped");
        return None;
    };
    Some(shown_as(
        frame,
        VideoPicture::from(frame.buffer().buffer().clone()),
    ))
}

/// `picture`, shown as Waft's `frame` is: turned, then mirrored.
#[cfg(any(target_vendor = "apple", target_os = "android"))]
fn shown_as(frame: &dyn ui::VideoFrame, picture: VideoPicture) -> VideoFrame {
    VideoFrame {
        picture,
        rotation: frame.rotation(),
        flip: frame.flip(),
    }
}

/// A platform whose frames the engine does not take yet: none.
#[cfg(not(any(target_vendor = "apple", target_os = "android")))]
pub(crate) fn engine_frame(_frame: &dyn ui::VideoFrame) -> Option<VideoFrame> {
    None
}

#[cfg(test)]
mod tests {
    use waft_test::MockVideoFrameReaders;

    use super::*;

    #[test]
    fn a_reader_is_wafts_once_and_answers_no_frame_before() {
        let readers = MockVideoFrameReaders::new();
        let reader = ReaderOnRequest::default();
        assert!(reader.frame_for(Instant::now()).is_none(), "fed by nothing");
        let first = reader.waft_reader(&readers).unwrap();
        let again = reader.waft_reader(&readers).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(readers.made().len(), 1, "made once");
        assert!(Arc::ptr_eq(&first, &readers.made()[0]));
        assert!(
            reader.frame_for(Instant::now()).is_none(),
            "the mock has no frame"
        );
        #[cfg(any(target_os = "macos", target_os = "android"))]
        assert!(reader.fed_by_waos().is_none(), "Waft's already");
    }

    #[cfg(any(target_os = "macos", target_os = "android"))]
    #[test]
    fn a_reader_fed_by_waos_is_not_wafts() {
        let readers = MockVideoFrameReaders::new();
        let reader = ReaderOnRequest::default();
        assert!(reader.fed_by_waos().is_some());
        assert!(reader.waft_reader(&readers).is_none());
        assert!(readers.made().is_empty(), "no Waft reader made");
        assert!(
            reader.frame_for(Instant::now()).is_none(),
            "fed nothing yet"
        );
    }
}
