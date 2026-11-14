//! `wasdk:media/media-assets`: a program's media read by the system without playing it, its length,
//! its tracks, its tags and a picture at a time, for a library before anything plays (decision 32
//! of `docs/proposals/2026-11-02-program-media.md`).
//!
//! The shell reads ([`ShellMediaAssets`], each asset a `ShellMediaAsset`); the engine keeps the
//! rules, a source checked as a player's is (`media_sources`): a link passes the program's network
//! rules before the shell is handed it, its host looked up first by the program's own lookup, off
//! the program's thread; a path is where its mount puts it on the machine. An asset's picture is
//! the system's frame, held by the program as a reader's frames are (`video_frames`). The interface
//! is served to a program by `host`.
//!
//! The invariant: nothing the rules refuse reaches the shell, neither a link whose host the program
//! may not reach nor a path outside its files: such an asset answers every question `refused`, with
//! why, and no shell's asset is opened for it.

pub(crate) mod host;

use std::future::{Future, ready};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use url::Url;

use crate::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, PlayerError, PlayerErrorKind, SeekTolerance,
};
use crate::bindings::wasdk::platform::windows::Size;
use crate::files::ProgramFiles;
use crate::media_sources::{self, Named};
use crate::network::ProgramNetwork;
use crate::running::RunningProgram;
use crate::shell::{MediaAssetFuture, ShellMediaAsset, ShellMediaAssets, VideoFrame};

/// `media-assets.media-assets`: the shell's reading of media, as the program reaches it.
pub struct MediaAssets(Arc<dyn ShellMediaAssets>);

/// `media-assets.media-asset`: one of the program's assets, the shell's once its source passed.
pub struct MediaAsset(Opening);

/// The shell's asset once its source passed, or why it did not: shared by the asset and each of
/// its reads under way, which the shell's asset outlives.
type Opening = Shared<BoxFuture<'static, Result<Arc<dyn ShellMediaAsset>, PlayerError>>>;

impl MediaAssets {
    /// An asset of `source` for `program`: of a path inside `files`, open on the shell at once; of
    /// a link whose host `network` lets the program reach, open once that is known; and of
    /// anything else, refused.
    fn open(
        &self,
        program: &RunningProgram,
        files: &mut ProgramFiles,
        network: &ProgramNetwork,
        source: &str,
    ) -> MediaAsset {
        match media_sources::named(source) {
            Ok(Named::Path(path)) => self.open_file(program, files, &path),
            Ok(Named::Link(link)) => self.open_once_reachable(program, network, link),
            Err(reason) => MediaAsset::refused(reason),
        }
    }

    /// An asset of the file at the program's `path`, where it is on the machine; refused for a
    /// path outside the program's files.
    fn open_file(
        &self,
        program: &RunningProgram,
        files: &mut ProgramFiles,
        path: &Path,
    ) -> MediaAsset {
        match media_sources::file_url(files, path) {
            Ok(url) => MediaAsset::of(Ok(Arc::from(self.0.open(program, &url)))),
            Err(reason) => MediaAsset::refused(reason),
        }
    }

    /// An asset of `link`, open on the shell once the program's lookup of its host, made off the
    /// program's thread, lets it reach the host, and refused otherwise.
    fn open_once_reachable(
        &self,
        program: &RunningProgram,
        network: &ProgramNetwork,
        link: Url,
    ) -> MediaAsset {
        let (shell, program, network) = (Arc::clone(&self.0), program.clone(), network.clone());
        // Detached: an asset let go of meanwhile, with no read under way, is never opened.
        let checked = tokio::task::spawn_blocking(move || {
            media_sources::check_link(&network, &link).map(|()| link)
        });
        let opening = async move {
            match checked.await {
                Ok(Ok(link)) => Ok(Arc::from(shell.open(&program, link.as_str()))),
                Ok(Err(reason)) => Err(refused(reason)),
                Err(ended) => Err(PlayerError {
                    kind: PlayerErrorKind::Other,
                    message: format!("the lookup of the link's host ended: {ended}"),
                }),
            }
        };
        MediaAsset(opening.boxed().shared())
    }
}

impl MediaAsset {
    fn of(opened: Result<Arc<dyn ShellMediaAsset>, PlayerError>) -> MediaAsset {
        MediaAsset(ready(opened).boxed().shared())
    }

    /// An asset whose source was refused for `reason`: every answer says so, and no shell's asset
    /// is opened.
    fn refused(reason: String) -> MediaAsset {
        tracing::info!("a media asset's source is refused: {reason}");
        MediaAsset::of(Err(refused(reason)))
    }

    pub(super) fn duration(
        &self,
    ) -> impl Future<Output = Result<Option<Duration>, PlayerError>> + use<> {
        read(&self.0, |shell| shell.duration())
    }

    pub(super) fn tracks(
        &self,
    ) -> impl Future<Output = Result<Vec<MediaTrack>, PlayerError>> + use<> {
        read(&self.0, |shell| shell.tracks())
    }

    pub(super) fn metadata(
        &self,
    ) -> impl Future<Output = Result<MediaMetadata, PlayerError>> + use<> {
        read(&self.0, |shell| shell.metadata())
    }

    /// The picture at `time`, within `tolerance`, scaled to fit `max_size` where given.
    pub(super) fn frame_at(
        &self,
        time: Duration,
        tolerance: SeekTolerance,
        max_size: Option<Size>,
    ) -> impl Future<Output = Result<VideoFrame, PlayerError>> + use<> {
        read(&self.0, move |shell| {
            shell.frame_at(time, tolerance, max_size)
        })
    }
}

/// What `ask` reads of the shell's asset, once `opening` has it; why not, where its source was
/// refused.
fn read<T, Ask>(
    opening: &Opening,
    ask: Ask,
) -> impl Future<Output = Result<T, PlayerError>> + Send + use<T, Ask>
where
    Ask: FnOnce(&dyn ShellMediaAsset) -> MediaAssetFuture<T> + Send + 'static,
{
    let opening = opening.clone();
    async move {
        let shell = opening.await?;
        ask(shell.as_ref()).await
    }
}

/// The failure of an asset whose source was refused for `reason`.
fn refused(reason: String) -> PlayerError {
    PlayerError {
        kind: PlayerErrorKind::Refused,
        message: reason,
    }
}
