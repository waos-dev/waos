//! Waft's reading of media over `wasdk:media/media-assets`, where the system has one, one to one:
//! an asset of a URL is the WIT's asset of the source it names, as a player's is (`players`); each
//! read is the WIT's `async func` of the same name, a time in the nanoseconds of a WIT `duration`;
//! its tracks, metadata and failures are the player's in Waft's terms, and its picture the WIT's
//! frame as a reader's frames are (`video_frames`), so an app draws it as it draws any frame.
//!
//! The invariant: each read holds the WIT's asset until it answers, so an app that lets the asset
//! go while a read is under way still hears it.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use waft_embedder::{
    MediaAsset, MediaAssetFuture, MediaAssets, MediaMetadata, MediaTrack, SeekTolerance, Size,
    VideoFrame,
};
use wasdk::media_assets as platform;
use wasdk::media_players::SeekTolerance as WitTolerance;
use wasdk::windows;

use crate::players::{
    framework_error, framework_metadata, framework_track, nanoseconds, source_of,
};
use crate::video_frames::framework_frame;

/// The system's reading of media.
pub(crate) struct WasdkMediaAssets(platform::MediaAssets);

impl WasdkMediaAssets {
    /// The system's reading of media, where it has one.
    pub(crate) fn get() -> Option<WasdkMediaAssets> {
        platform::get_media_assets().map(WasdkMediaAssets)
    }
}

impl MediaAssets for WasdkMediaAssets {
    fn open(&self, url: &str) -> Box<dyn MediaAsset> {
        Box::new(WasdkMediaAsset(Rc::new(self.0.open(&source_of(url)))))
    }
}

/// One of the system's assets, shared with each of its reads under way.
struct WasdkMediaAsset(Rc<platform::MediaAsset>);

impl MediaAsset for WasdkMediaAsset {
    fn duration(&self) -> MediaAssetFuture<Option<Duration>> {
        let asset = Rc::clone(&self.0);
        Box::pin(async move {
            let read = asset.duration().await;
            read.map(|duration| duration.map(Duration::from_nanos))
                .map_err(framework_error)
        })
    }

    fn tracks(&self) -> MediaAssetFuture<Vec<MediaTrack>> {
        let asset = Rc::clone(&self.0);
        Box::pin(async move {
            let read = asset.tracks().await.map_err(framework_error)?;
            Ok(read.into_iter().map(framework_track).collect())
        })
    }

    fn metadata(&self) -> MediaAssetFuture<MediaMetadata> {
        let asset = Rc::clone(&self.0);
        Box::pin(async move {
            let read = asset.metadata().await;
            read.map(framework_metadata).map_err(framework_error)
        })
    }

    fn frame_at(
        &self,
        time: Duration,
        tolerance: SeekTolerance,
        max_size: Option<Size>,
    ) -> MediaAssetFuture<Arc<dyn VideoFrame>> {
        let asset = Rc::clone(&self.0);
        let tolerance = WitTolerance {
            before: nanoseconds(tolerance.before),
            after: nanoseconds(tolerance.after),
        };
        let max_size = max_size.map(|size| windows::Size {
            width: size.width(),
            height: size.height(),
        });
        Box::pin(async move {
            let read = asset.frame_at(nanoseconds(time), tolerance, max_size).await;
            read.map(framework_frame).map_err(framework_error)
        })
    }
}
