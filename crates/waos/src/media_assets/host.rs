//! The code serving `wasdk:media/media-assets` to a program: the shell's reading of media, where it
//! has one, an asset opened on what the program's source names, and its length, tracks, tags and
//! pictures, each answered once the shell has read it, a picture as the program's video frame.

use std::sync::Arc;
use std::time::Duration;

use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable};

use super::{MediaAsset, MediaAssets};
use crate::bindings::wasdk::media::media_assets::{
    self, HostMediaAsset, HostMediaAssetWithStore, HostMediaAssets,
};
use crate::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, PlayerError, SeekTolerance,
};
use crate::bindings::wasdk::platform::windows::Size;
use crate::files::ProgramFiles;
use crate::network::ProgramNetwork;
use crate::running::AppInstance;
use crate::shell::ShellMediaAssets;
use crate::video_frames::ProgramVideoFrame;

/// What the media assets interface reads and writes in a program's store.
pub(crate) struct MediaAssetsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a AppInstance,
    /// The shell's reading of media; none where the shell has none.
    pub(crate) media_assets: Option<&'a Arc<dyn ShellMediaAssets>>,
    /// The program's mounts, where a path it names is looked up.
    pub(crate) files: &'a mut ProgramFiles,
    /// What the program may reach of the network, which a link's host is looked up by.
    pub(crate) network: &'a ProgramNetwork,
}

pub(crate) trait MediaAssetsCtxView: Send {
    fn media_assets_ctx(&mut self) -> MediaAssetsCtx<'_>;
}

struct HasMediaAssets;

impl HasData for HasMediaAssets {
    type Data<'a> = MediaAssetsCtx<'a>;
}

pub(crate) fn add_to_linker<T: MediaAssetsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    media_assets::add_to_linker::<_, HasMediaAssets>(linker, T::media_assets_ctx)
}

impl media_assets::Host for MediaAssetsCtx<'_> {
    fn get_media_assets(&mut self) -> wasmtime::Result<Option<Resource<MediaAssets>>> {
        let assets = self.media_assets.cloned().map(MediaAssets);
        Ok(assets.map(|assets| self.table.push(assets)).transpose()?)
    }
}

impl HostMediaAssets for MediaAssetsCtx<'_> {
    fn open(
        &mut self,
        assets: Resource<MediaAssets>,
        source: String,
    ) -> wasmtime::Result<Resource<MediaAsset>> {
        let asset = self
            .table
            .get(&assets)?
            .open(self.program, self.files, self.network, &source);
        Ok(self.table.push(asset)?)
    }

    fn drop(&mut self, assets: Resource<MediaAssets>) -> wasmtime::Result<()> {
        self.table.delete(assets)?;
        Ok(())
    }
}

impl HostMediaAsset for MediaAssetsCtx<'_> {
    /// The program lets go of the asset; its reads under way still answer.
    fn drop(&mut self, asset: Resource<MediaAsset>) -> wasmtime::Result<()> {
        self.table.delete(asset)?;
        Ok(())
    }
}

impl<T: MediaAssetsCtxView + 'static> HostMediaAssetWithStore<T> for HasMediaAssets {
    /// None for a live stream.
    async fn duration(
        accessor: &Accessor<T, Self>,
        asset: Resource<MediaAsset>,
    ) -> wasmtime::Result<Result<Option<u64>, PlayerError>> {
        let reading = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&asset)?.duration())
        })?;
        Ok(reading.await.map(|duration| duration.map(nanoseconds)))
    }

    async fn tracks(
        accessor: &Accessor<T, Self>,
        asset: Resource<MediaAsset>,
    ) -> wasmtime::Result<Result<Vec<MediaTrack>, PlayerError>> {
        let reading = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&asset)?.tracks())
        })?;
        Ok(reading.await)
    }

    async fn metadata(
        accessor: &Accessor<T, Self>,
        asset: Resource<MediaAsset>,
    ) -> wasmtime::Result<Result<MediaMetadata, PlayerError>> {
        let reading = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&asset)?.metadata())
        })?;
        Ok(reading.await)
    }

    /// The system's frame, held by the program as a reader's frames are.
    async fn frame_at(
        accessor: &Accessor<T, Self>,
        asset: Resource<MediaAsset>,
        time: u64,
        tolerance: SeekTolerance,
        max_size: Option<Size>,
    ) -> wasmtime::Result<Result<Resource<ProgramVideoFrame>, PlayerError>> {
        let reading = accessor.with(|mut access| {
            let asset = access.get().table.get(&asset)?;
            wasmtime::Result::<_>::Ok(asset.frame_at(
                Duration::from_nanos(time),
                tolerance,
                max_size,
            ))
        })?;
        match reading.await {
            Ok(frame) => accessor.with(|mut access| {
                let frame = ProgramVideoFrame::of(frame);
                Ok(Ok(access.get().table.push(frame)?))
            }),
            Err(error) => Ok(Err(error)),
        }
    }
}

/// `duration` in a WIT duration's nanoseconds; the longest one holds for a longer one.
fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
