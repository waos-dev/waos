//! The code serving `wasdk:media/video-frames` to a program: the shell's readers, where the system
//! hands out frames, a reader made on them, the frame a reader answers for a display time on the
//! program's clock, and a frame's hardware buffer.

use std::sync::Arc;

use wasmtime::component::{HasData, Linker, Resource, ResourceTable};

use super::{ProgramVideoFrame, VideoFrameReader, VideoFrameReaders};
use crate::bindings::wasdk::media::video_frames::{
    self, HdrMetadata, HostVideoFrame, HostVideoFrameReader, HostVideoFrameReaders, Instant,
    PixelAspect, PixelRect, VideoColor, VideoPlane,
};
use crate::clock::Clock;
use crate::hardware_buffers::ProgramHardwareBuffer;
use crate::shell::ShellVideoFrameReaders;

/// What the video frames interface reads and writes in a program's store.
pub(crate) struct VideoFramesCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    /// The shell's readers; none where the system hands out no frames.
    pub(crate) readers: Option<&'a Arc<dyn ShellVideoFrameReaders>>,
    /// The program's clock, which the display times it names are on.
    pub(crate) clock: Clock,
}

pub(crate) trait VideoFramesCtxView: Send {
    fn video_frames_ctx(&mut self) -> VideoFramesCtx<'_>;
}

struct HasVideoFrames;

impl HasData for HasVideoFrames {
    type Data<'a> = VideoFramesCtx<'a>;
}

pub(crate) fn add_to_linker<T: VideoFramesCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    video_frames::add_to_linker::<_, HasVideoFrames>(linker, T::video_frames_ctx)
}

impl video_frames::Host for VideoFramesCtx<'_> {
    fn get_video_frame_readers(&mut self) -> wasmtime::Result<Option<Resource<VideoFrameReaders>>> {
        let readers = self.readers.cloned().map(VideoFrameReaders);
        Ok(readers
            .map(|readers| self.table.push(readers))
            .transpose()?)
    }
}

impl HostVideoFrameReaders for VideoFramesCtx<'_> {
    fn make_reader(
        &mut self,
        readers: Resource<VideoFrameReaders>,
    ) -> wasmtime::Result<Resource<VideoFrameReader>> {
        let reader = self.table.get(&readers)?.0.make_reader();
        Ok(self.table.push(VideoFrameReader(reader))?)
    }

    fn drop(&mut self, readers: Resource<VideoFrameReaders>) -> wasmtime::Result<()> {
        self.table.delete(readers)?;
        Ok(())
    }
}

impl HostVideoFrameReader for VideoFramesCtx<'_> {
    /// The shell's reader is asked for the frame of `display_time`, on the program's clock, as the
    /// system's moment.
    fn frame_for(
        &mut self,
        reader: Resource<VideoFrameReader>,
        display_time: Instant,
    ) -> wasmtime::Result<Option<Resource<ProgramVideoFrame>>> {
        let display_time = self.clock.instant_of(display_time);
        let frame = self.table.get(&reader)?.0.frame_for(display_time);
        let frame = frame.map(ProgramVideoFrame::of);
        Ok(frame.map(|frame| self.table.push(frame)).transpose()?)
    }

    /// The program lets go of the reader; a player sending it frames still holds it.
    fn drop(&mut self, reader: Resource<VideoFrameReader>) -> wasmtime::Result<()> {
        self.table.delete(reader)?;
        Ok(())
    }
}

impl HostVideoFrame for VideoFramesCtx<'_> {
    /// The frame's buffer, which keeps it. A frame of a kind the engine does not take is the
    /// shell's failure, and traps the program.
    fn hardware_buffer(
        &mut self,
        frame: Resource<ProgramVideoFrame>,
    ) -> wasmtime::Result<Resource<ProgramHardwareBuffer>> {
        let frame = self.table.get(&frame)?.memory();
        let buffer =
            ProgramHardwareBuffer::of_frame(frame).map_err(wasmtime::Error::from_anyhow)?;
        Ok(self.table.push(buffer)?)
    }

    fn rotation(&mut self, frame: Resource<ProgramVideoFrame>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&frame)?.rotation())
    }

    fn flip(&mut self, frame: Resource<ProgramVideoFrame>) -> wasmtime::Result<bool> {
        Ok(self.table.get(&frame)?.flip())
    }

    /// None: a reader's frame comes for a display time, and carries no time of its own.
    fn timestamp(&mut self, _frame: Resource<ProgramVideoFrame>) -> wasmtime::Result<Option<i64>> {
        Ok(None)
    }

    fn duration(&mut self, _frame: Resource<ProgramVideoFrame>) -> wasmtime::Result<Option<u64>> {
        Ok(None)
    }

    fn visible_rect(&mut self, frame: Resource<ProgramVideoFrame>) -> wasmtime::Result<PixelRect> {
        let frame = self.table.get(&frame)?;
        frame.visible_rect().map_err(wasmtime::Error::from_anyhow)
    }

    /// Square: a reader's frame is drawn for showing as it is.
    fn pixel_aspect(
        &mut self,
        _frame: Resource<ProgramVideoFrame>,
    ) -> wasmtime::Result<PixelAspect> {
        Ok(PixelAspect {
            width: 1,
            height: 1,
        })
    }

    fn planes(
        &mut self,
        frame: Resource<ProgramVideoFrame>,
    ) -> wasmtime::Result<Option<Vec<VideoPlane>>> {
        let frame = self.table.get(&frame)?;
        let planes = frame.planes().map_err(wasmtime::Error::from_anyhow)?;
        Ok(Some(planes))
    }

    fn color(&mut self, frame: Resource<ProgramVideoFrame>) -> wasmtime::Result<VideoColor> {
        Ok(self.table.get(&frame)?.color())
    }

    /// None: the shell is not told what a reader's frame was mastered for.
    fn hdr_metadata(
        &mut self,
        _frame: Resource<ProgramVideoFrame>,
    ) -> wasmtime::Result<Option<HdrMetadata>> {
        Ok(None)
    }

    /// The program lets go of the frame; its hardware buffer and their textures still hold it.
    fn drop(&mut self, frame: Resource<ProgramVideoFrame>) -> wasmtime::Result<()> {
        self.table.delete(frame)?;
        Ok(())
    }
}
