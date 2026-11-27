//! Where the programs' media assets go: each opening and each read to the main thread, where
//! Waft's reading of media is (`state::media_assets`), and each answer back, a picture as the
//! platform's own frame. The engine opens an asset from a program's thread, or from the thread its
//! link's host was looked up on, and reads it from the program's.
//!
//! The invariant: an asset's calls reach the main thread in the order made, its opening first and
//! its removal last, sent as the engine lets go of it; and a read whose answer cannot come, as
//! WaOS quits, is answered at once, so no program waits for it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waft::ui;
use waos::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, PlayerError, PlayerErrorKind, SeekTolerance,
};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{MediaAssetFuture, RunningProgram, ShellMediaAsset, ShellMediaAssets, VideoFrame};

use super::camera_values::waft_size;
use super::frame_readers::engine_frame;
use super::player_states::waft_tolerance;

/// The id the app gives each asset a program opens, which its calls carry.
pub(crate) type AssetKey = u64;

/// One call of the programs' media assets, on its way to the main thread.
pub(crate) enum AssetCall {
    /// A program opens an asset of `url`.
    Open { asset: AssetKey, url: String },
    /// Something read of one of the assets.
    Read(AssetKey, AssetRead),
    /// The program let one of its assets go.
    Remove(AssetKey),
}

/// What a program reads of one of its assets, with where the answer goes.
pub(crate) enum AssetRead {
    /// How long it is; none for a live stream.
    Duration(oneshot::Sender<Result<Option<Duration>, PlayerError>>),
    Tracks(oneshot::Sender<Result<Vec<MediaTrack>, PlayerError>>),
    Metadata(oneshot::Sender<Result<MediaMetadata, PlayerError>>),
    /// The picture at the time, within the tolerance, fitting the size where given.
    FrameAt {
        time: Duration,
        tolerance: ui::SeekTolerance,
        max_size: Option<ui::Size>,
        answer: oneshot::Sender<Result<Arc<dyn ui::VideoFrame>, PlayerError>>,
    },
}

impl AssetRead {
    /// Answers `error`, whatever it reads.
    pub(crate) fn fail(self, error: PlayerError) {
        // A program that ended meanwhile hears nothing.
        match self {
            AssetRead::Duration(answer) => drop(answer.send(Err(error))),
            AssetRead::Tracks(answer) => drop(answer.send(Err(error))),
            AssetRead::Metadata(answer) => drop(answer.send(Err(error))),
            AssetRead::FrameAt { answer, .. } => drop(answer.send(Err(error))),
        }
    }
}

/// The app's media assets, as the engine reaches them: every call goes to the main thread.
pub(crate) struct AssetsOnRequest {
    calls: UnboundedSender<AssetCall>,
    last_asset: AtomicU64,
}

impl AssetsOnRequest {
    /// The assets, and where their calls arrive on the main thread.
    pub(crate) fn new() -> (AssetsOnRequest, UnboundedReceiver<AssetCall>) {
        let (calls, arriving) = mpsc::unbounded();
        let assets = AssetsOnRequest {
            calls,
            last_asset: AtomicU64::default(),
        };
        (assets, arriving)
    }
}

impl ShellMediaAssets for AssetsOnRequest {
    fn open(&self, program: &RunningProgram, url: &str) -> Box<dyn ShellMediaAsset> {
        tracing::info!(
            program = program.name(),
            url,
            "a program opens a media asset"
        );
        let asset = self.last_asset.fetch_add(1, Ordering::Relaxed) + 1;
        let held = AssetOnRequest {
            asset,
            calls: self.calls.clone(),
        };
        held.send(AssetCall::Open {
            asset,
            url: url.to_owned(),
        });
        Box::new(held)
    }
}

/// One asset, as the engine holds it: its reads go to the main thread, and its removal as it
/// drops.
struct AssetOnRequest {
    asset: AssetKey,
    calls: UnboundedSender<AssetCall>,
}

impl AssetOnRequest {
    fn send(&self, call: AssetCall) {
        if self.calls.unbounded_send(call).is_err() {
            tracing::info!("WaOS is quitting: a media asset's call goes nowhere");
        }
    }

    /// Asks `read` of the asset on the main thread; the answer comes on the channel it carries,
    /// which a call that goes nowhere drops.
    fn read(&self, read: AssetRead) {
        self.send(AssetCall::Read(self.asset, read));
    }
}

impl ShellMediaAsset for AssetOnRequest {
    fn duration(&self) -> MediaAssetFuture<Option<Duration>> {
        let (answer, answered) = oneshot::channel();
        self.read(AssetRead::Duration(answer));
        Box::pin(async move { answered.await.unwrap_or_else(|_| Err(quitting())) })
    }

    fn tracks(&self) -> MediaAssetFuture<Vec<MediaTrack>> {
        let (answer, answered) = oneshot::channel();
        self.read(AssetRead::Tracks(answer));
        Box::pin(async move { answered.await.unwrap_or_else(|_| Err(quitting())) })
    }

    fn metadata(&self) -> MediaAssetFuture<MediaMetadata> {
        let (answer, answered) = oneshot::channel();
        self.read(AssetRead::Metadata(answer));
        Box::pin(async move { answered.await.unwrap_or_else(|_| Err(quitting())) })
    }

    /// Waft's frame, as the engine's: the platform's own behind it.
    fn frame_at(
        &self,
        time: Duration,
        tolerance: SeekTolerance,
        max_size: Option<Size>,
    ) -> MediaAssetFuture<VideoFrame> {
        let (answer, answered) = oneshot::channel();
        self.read(AssetRead::FrameAt {
            time,
            tolerance: waft_tolerance(tolerance),
            max_size: max_size.map(waft_size),
            answer,
        });
        Box::pin(async move {
            let frame = answered.await.unwrap_or_else(|_| Err(quitting()))?;
            engine_frame(frame.as_ref()).ok_or_else(|| PlayerError {
                kind: PlayerErrorKind::Unsupported,
                message: "the system's picture is of a kind WaOS does not hand out".to_owned(),
            })
        })
    }
}

impl Drop for AssetOnRequest {
    fn drop(&mut self) {
        self.send(AssetCall::Remove(self.asset));
    }
}

/// What a read answers once the main thread no longer takes calls.
fn quitting() -> PlayerError {
    PlayerError {
        kind: PlayerErrorKind::Other,
        message: "WaOS is quitting".to_owned(),
    }
}
