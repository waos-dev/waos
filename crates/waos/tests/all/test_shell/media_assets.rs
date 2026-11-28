//! The system's reading of media as the test shell keeps it: each asset opened recorded with the
//! program that opened it and its URL, what was read of it and each picture asked of it, in order;
//! every asset answers what the test set, a picture as a reader's frame of one colour.

use std::future::ready;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use waos::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, PlayerError, PlayerErrorKind, SeekTolerance,
};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{AppInstance, MediaAssetFuture, ShellMediaAsset, ShellMediaAssets, VideoFrame};
use waos_headless::testing::Changes;

/// Assets that each answer what the test set, and are kept as they are opened.
#[derive(Default)]
pub struct TestMediaAssets {
    answers: Arc<Mutex<AssetAnswers>>,
    opened: Mutex<Vec<OpenedAsset>>,
    changes: Arc<Changes>,
}

/// What every asset answers: its length, none for a live stream, its tracks and tags, and the
/// colour of its pictures, as the bytes of a BGRA pixel, each or a failure. Each fails until the
/// test sets it.
#[derive(Clone)]
pub struct AssetAnswers {
    pub duration: Result<Option<Duration>, PlayerError>,
    pub tracks: Result<Vec<MediaTrack>, PlayerError>,
    pub metadata: Result<MediaMetadata, PlayerError>,
    pub picture: Result<[u8; 4], PlayerError>,
}

impl Default for AssetAnswers {
    fn default() -> AssetAnswers {
        AssetAnswers {
            duration: Err(unset()),
            tracks: Err(unset()),
            metadata: Err(unset()),
            picture: Err(unset()),
        }
    }
}

fn unset() -> PlayerError {
    PlayerError {
        kind: PlayerErrorKind::Other,
        message: "the test set no answer".to_owned(),
    }
}

/// One asset a program opened, as the test sees it.
#[derive(Clone)]
pub struct OpenedAsset {
    /// The name of the program that opened it.
    pub program: String,
    pub url: String,
    reads: Arc<Mutex<Vec<&'static str>>>,
    frames_asked: Arc<Mutex<Vec<FrameAsked>>>,
    dropped: Arc<AtomicBool>,
}

/// A picture the program asked an asset for: the time and the tolerance's nanoseconds before and
/// after it, and the size to fit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameAsked {
    pub time: Duration,
    pub before: u64,
    pub after: u64,
    pub max_size: Option<(f64, f64)>,
}

impl TestMediaAssets {
    /// Every asset answers `answers` from now on.
    pub fn answer_with(&self, answers: AssetAnswers) {
        *self.answers.lock().unwrap() = answers;
    }

    /// The assets opened so far, in order.
    pub fn opened(&self) -> Vec<OpenedAsset> {
        self.opened.lock().unwrap().clone()
    }
}

impl OpenedAsset {
    /// What was read of it so far, in order: `duration`, `tracks`, `metadata` or `frame-at`.
    pub fn reads(&self) -> Vec<&'static str> {
        self.reads.lock().unwrap().clone()
    }

    /// The pictures asked of it so far, in order.
    pub fn frames_asked(&self) -> Vec<FrameAsked> {
        self.frames_asked.lock().unwrap().clone()
    }

    /// Whether the engine let go of the asset.
    pub fn is_dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }
}

impl TestMediaAssets {
    /// Assets of which none is opened, announcing each opening, read and drop to `changes`.
    pub fn announcing(changes: &Arc<Changes>) -> TestMediaAssets {
        TestMediaAssets {
            changes: Arc::clone(changes),
            ..TestMediaAssets::default()
        }
    }
}

impl ShellMediaAssets for TestMediaAssets {
    fn open(&self, program: &AppInstance, url: &str) -> Box<dyn ShellMediaAsset> {
        let opened = OpenedAsset {
            program: program.name().to_owned(),
            url: url.to_owned(),
            reads: Arc::default(),
            frames_asked: Arc::default(),
            dropped: Arc::default(),
        };
        let asset = TestAsset {
            answers: Arc::clone(&self.answers),
            reads: Arc::clone(&opened.reads),
            frames_asked: Arc::clone(&opened.frames_asked),
            dropped: Arc::clone(&opened.dropped),
            changes: Arc::clone(&self.changes),
        };
        self.opened.lock().unwrap().push(opened);
        self.changes.announce();
        Box::new(asset)
    }
}

/// The asset the engine holds.
struct TestAsset {
    answers: Arc<Mutex<AssetAnswers>>,
    reads: Arc<Mutex<Vec<&'static str>>>,
    frames_asked: Arc<Mutex<Vec<FrameAsked>>>,
    dropped: Arc<AtomicBool>,
    changes: Arc<Changes>,
}

impl TestAsset {
    /// What the test set, as `read` asks it.
    fn answers(&self, read: &'static str) -> AssetAnswers {
        self.reads.lock().unwrap().push(read);
        self.changes.announce();
        self.answers.lock().unwrap().clone()
    }
}

impl ShellMediaAsset for TestAsset {
    fn duration(&self) -> MediaAssetFuture<Option<Duration>> {
        Box::pin(ready(self.answers("duration").duration))
    }

    fn tracks(&self) -> MediaAssetFuture<Vec<MediaTrack>> {
        Box::pin(ready(self.answers("tracks").tracks))
    }

    fn metadata(&self) -> MediaAssetFuture<MediaMetadata> {
        Box::pin(ready(self.answers("metadata").metadata))
    }

    fn frame_at(
        &self,
        time: Duration,
        tolerance: SeekTolerance,
        max_size: Option<Size>,
    ) -> MediaAssetFuture<VideoFrame> {
        self.frames_asked.lock().unwrap().push(FrameAsked {
            time,
            before: tolerance.before,
            after: tolerance.after,
            max_size: max_size.map(|size| (size.width, size.height)),
        });
        self.changes.announce();
        picture(self.answers("frame-at").picture)
    }
}

/// A frame of `picture`'s colour, or its failure.
#[cfg(target_vendor = "apple")]
fn picture(picture: Result<[u8; 4], PlayerError>) -> MediaAssetFuture<VideoFrame> {
    use super::video_frames::{HeldFrame, frame_of};

    match picture {
        Ok(bgra) => {
            let held = HeldFrame::new(frame_of(bgra));
            Box::pin(async move { Ok(held.into_frame()) })
        }
        // Not `ready`, which would hold a frame's type, which crosses threads only held.
        Err(error) => Box::pin(async move { Err(error) }),
    }
}

/// A platform whose frames the engine does not take yet: no picture.
#[cfg(not(target_vendor = "apple"))]
fn picture(_picture: Result<[u8; 4], PlayerError>) -> MediaAssetFuture<VideoFrame> {
    Box::pin(async move {
        Err(PlayerError {
            kind: PlayerErrorKind::Unsupported,
            message: "no frames on this system".to_owned(),
        })
    })
}

impl Drop for TestAsset {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
        self.changes.announce();
    }
}
