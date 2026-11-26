//! The code serving `wasdk:media/video-decoders` to a program: the decoders of its device's GPU,
//! what they say of a stream, a decoder opened on them, the chunks it is handed, and its frames,
//! each entering the program's table as the program reads it, with why the decoder ended beside
//! them.

use std::sync::Arc;

use waos_gpu_host::Device;
use wasmtime::component::{
    Access, Accessor, FutureReader, HasData, Linker, Resource, ResourceTable, StreamReader,
};
use wasmtime::{AsContextMut, bail};

use super::{VideoDecoder, VideoDecoders};
use crate::bindings::wasdk::media::video_decoders::{
    self, DecoderError, HostVideoDecoder, HostVideoDecoderWithStore, HostVideoDecoders,
    HostVideoDecodersWithStore, VideoDecodeSupport, VideoDecoderOptions, VideoStreamDescription,
};
use crate::bindings::wasdk::media::video_streams::{CodecKind, EncodedChunk, VideoStreamSetup};
use crate::gpu::GpuInstance;
use crate::shell::ShellGpus;
use crate::stream::stream_of;
use crate::video_frames::ProgramVideoFrame;

/// What the video decoders interface reads and writes in a program's store.
pub(crate) struct VideoDecodersCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    /// The instance the program's devices are on, which names each device's GPU.
    pub(crate) gpu: &'a GpuInstance,
    /// The system's GPUs, whose decoders a device asks for; none where the system decodes no
    /// video for programs.
    pub(crate) gpus: Option<&'a Arc<dyn ShellGpus>>,
}

pub(crate) trait VideoDecodersCtxView: Send {
    fn video_decoders_ctx(&mut self) -> VideoDecodersCtx<'_>;
}

struct HasVideoDecoders;

impl HasData for HasVideoDecoders {
    type Data<'a> = VideoDecodersCtx<'a>;
}

pub(crate) fn add_to_linker<T: VideoDecodersCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    video_decoders::add_to_linker::<_, HasVideoDecoders>(linker, T::video_decoders_ctx)
}

impl video_decoders::Host for VideoDecodersCtx<'_> {
    /// The decoders of the GPU `device` is on, as the system names it, the system's default where
    /// it names none, as Apple's systems and Android.
    fn get_video_decoders(
        &mut self,
        device: Resource<Device>,
    ) -> wasmtime::Result<Option<Resource<VideoDecoders>>> {
        let Some(gpus) = self.gpus else {
            return Ok(None);
        };
        let adapter = self.table.get(&device)?.adapter_id();
        let gpu = crate::os::adapter_gpu(self.gpu, adapter);
        let decoders = gpus.video_decoders(gpu).map(VideoDecoders::new);
        Ok(decoders
            .map(|decoders| self.table.push(decoders))
            .transpose()?)
    }
}

impl HostVideoDecoders for VideoDecodersCtx<'_> {
    fn drop(&mut self, decoders: Resource<VideoDecoders>) -> wasmtime::Result<()> {
        self.table.delete(decoders)?;
        Ok(())
    }
}

impl<T: VideoDecodersCtxView + 'static> HostVideoDecodersWithStore<T> for HasVideoDecoders {
    async fn support(
        accessor: &Accessor<T, Self>,
        decoders: Resource<VideoDecoders>,
        description: VideoStreamDescription,
    ) -> wasmtime::Result<VideoDecodeSupport> {
        let asked = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&decoders)?.support(description))
        })?;
        Ok(asked.await)
    }

    /// A decoder that lends no frame could give the program none: a limit of none is the
    /// program's mistake, and traps it.
    async fn open(
        accessor: &Accessor<T, Self>,
        decoders: Resource<VideoDecoders>,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> wasmtime::Result<Result<Resource<VideoDecoder>, DecoderError>> {
        if options.frame_limit == 0 {
            bail!("a video decoder lends at least one frame at a time, not none")
        }
        let opening = accessor.with(|mut access| {
            let decoders = access.get().table.get(&decoders)?;
            wasmtime::Result::<_>::Ok(decoders.open(setup, options))
        })?;
        match opening.await {
            Ok(decoder) => accessor.with(|mut access| Ok(Ok(access.get().table.push(decoder)?))),
            Err(error) => Ok(Err(error)),
        }
    }
}

impl HostVideoDecoder for VideoDecodersCtx<'_> {
    fn kind(&mut self, decoder: Resource<VideoDecoder>) -> wasmtime::Result<CodecKind> {
        Ok(self.table.get(&decoder)?.kind())
    }

    fn reset(&mut self, decoder: Resource<VideoDecoder>) -> wasmtime::Result<()> {
        self.table.get(&decoder)?.reset();
        Ok(())
    }

    /// The program lets go of the decoder, which ends its frames; the frames it still holds stay
    /// the system's until it drops them.
    fn drop(&mut self, decoder: Resource<VideoDecoder>) -> wasmtime::Result<()> {
        self.table.delete(decoder)?;
        Ok(())
    }
}

impl<T: VideoDecodersCtxView + 'static> HostVideoDecoderWithStore<T> for HasVideoDecoders {
    async fn decode(
        accessor: &Accessor<T, Self>,
        decoder: Resource<VideoDecoder>,
        chunk: EncodedChunk,
    ) -> wasmtime::Result<Result<(), DecoderError>> {
        let decoding = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&decoder)?.decode(chunk))
        })?;
        Ok(decoding.await)
    }

    /// Each frame enters the program's table as the program reads it. The frames are taken once:
    /// asking again is the program's mistake, and traps it.
    fn frames(
        mut access: Access<'_, T, Self>,
        decoder: Resource<VideoDecoder>,
    ) -> wasmtime::Result<(
        StreamReader<Resource<ProgramVideoFrame>>,
        FutureReader<Result<(), DecoderError>>,
    )> {
        let Some(output) = access.get().table.get(&decoder)?.take_output() else {
            bail!("a video decoder's frames are taken once, and these were taken already")
        };
        let ended = output.ended;
        let ended = FutureReader::new(access.as_context_mut(), async move {
            wasmtime::error::Ok(ended.await)
        })?;
        let frames = stream_of(access, output.frames, |data: &mut T, (frame, slot)| {
            let frame = ProgramVideoFrame::of_decoder(frame, slot);
            Ok(data.video_decoders_ctx().table.push(frame)?)
        })?;
        Ok((frames, ended))
    }

    async fn drain(
        accessor: &Accessor<T, Self>,
        decoder: Resource<VideoDecoder>,
    ) -> wasmtime::Result<Result<(), DecoderError>> {
        let draining = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&decoder)?.drain())
        })?;
        Ok(draining.await)
    }

    async fn configure(
        accessor: &Accessor<T, Self>,
        decoder: Resource<VideoDecoder>,
        setup: VideoStreamSetup,
    ) -> wasmtime::Result<Result<(), DecoderError>> {
        let configuring = accessor.with(|mut access| {
            wasmtime::Result::<_>::Ok(access.get().table.get(&decoder)?.configure(setup))
        })?;
        Ok(configuring.await)
    }
}
