//! What the system reads of a video without playing it, for its tile in the library: its length
//! and its title, through Waft's media assets, and its thumbnail, a small picture a tenth of the
//! way in, at the nearest keyframe, which the system lands on soonest.

use std::sync::Arc;
use std::time::Duration;

use waft_embedder::{MediaAsset, SeekTolerance, Size, VideoFrame};

/// The largest a thumbnail is asked for, in pixels: a tile's picture at twice its size.
const THUMBNAIL_SIZE: Size = Size::new(480.0, 480.0);

/// How far into a video its thumbnail is taken, as a fraction of its length.
const THUMBNAIL_AT: f64 = 0.1;

/// What a video's tile shows of it besides its thumbnail; each none where the system could not
/// read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Facts {
    /// None for a live stream.
    pub(crate) duration: Option<Duration>,
    /// The title its tags give; none where they give none.
    pub(crate) title: Option<String>,
}

/// The length and title `asset` reads, once the system has read them; a failure is said and left
/// out.
pub(crate) async fn facts_of(asset: &dyn MediaAsset) -> Facts {
    let duration = asset.duration().await;
    let metadata = asset.metadata().await;
    if let Err(error) = &duration {
        eprintln!("a video's length is not read: {}", error.message);
    }
    Facts {
        duration: duration.ok().flatten(),
        title: metadata
            .ok()
            .and_then(|metadata| metadata.title)
            .filter(|title| !title.is_empty()),
    }
}

/// `asset`'s thumbnail: its picture [`THUMBNAIL_AT`] of the way into its `duration`, at its start
/// where that is unknown, at the nearest keyframe and no larger than [`THUMBNAIL_SIZE`]; none, said
/// so, where the system has no picture of it.
pub(crate) async fn thumbnail_of(
    asset: &dyn MediaAsset,
    duration: Option<Duration>,
) -> Option<Arc<dyn VideoFrame>> {
    let at = duration.map_or(Duration::ZERO, |duration| duration.mul_f64(THUMBNAIL_AT));
    match asset
        .frame_at(at, SeekTolerance::ANY, Some(THUMBNAIL_SIZE))
        .await
    {
        Ok(frame) => Some(frame),
        Err(error) => {
            eprintln!("a video's thumbnail is not read: {}", error.message);
            None
        }
    }
}
