//! The code serving `wasdk:media/image-codecs` to a program: the shell's codecs, where it has them,
//! the formats they read and write, an image opened from the program's bytes or from a file of its
//! own, what the image said of itself as it opened, its pictures decoded into the program's memory
//! or into a hardware buffer it holds as a video frame's, and pixels or a hardware buffer encoded,
//! each answered once the shell has, off the program's store, so the program does other things
//! meanwhile.

use std::sync::Arc;

use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable};
use wasmtime_wasi::runtime::spawn_blocking;

use super::{HardwareImageCodecs, ImageCodecs, ImageSource};
use crate::bindings::wasdk::media::image_codecs::{
    self, HostHardwareImageCodecs, HostHardwareImageCodecsWithStore, HostImageCodecs,
    HostImageCodecsWithStore, HostImageSource, HostImageSourceWithStore, ImageMetadata,
    ImageOrientation, ImagePictures,
};
use crate::bindings::wasdk::platform::hardware_buffers::{ImageError, ImageErrorKind, PixelBuffer};
use crate::bindings::wasdk::platform::windows::Size;
use crate::files::ProgramFiles;
use crate::gpu::GpuInstance;
use crate::hardware_buffers::ProgramHardwareBuffer;
use crate::running::RunningProgram;
use crate::shell::ShellImageCodecs;

/// What the image codecs interface reads and writes in a program's store.
pub(crate) struct ImageCodecsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a RunningProgram,
    /// The shell's image codecs; none where the shell has none.
    pub(crate) image_codecs: Option<&'a Arc<dyn ShellImageCodecs>>,
    /// The program's mounts, where a path it names is looked up.
    pub(crate) files: &'a mut ProgramFiles,
    /// The instance the program's devices are on, whose work on a buffer an encoding waits for.
    pub(crate) gpu: &'a GpuInstance,
}

pub(crate) trait ImageCodecsCtxView: Send {
    fn image_codecs_ctx(&mut self) -> ImageCodecsCtx<'_>;
}

struct HasImageCodecs;

impl HasData for HasImageCodecs {
    type Data<'a> = ImageCodecsCtx<'a>;
}

pub(crate) fn add_to_linker<T: ImageCodecsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    image_codecs::add_to_linker::<_, HasImageCodecs>(linker, T::image_codecs_ctx)
}

impl image_codecs::Host for ImageCodecsCtx<'_> {
    fn get_image_codecs(&mut self) -> wasmtime::Result<Option<Resource<ImageCodecs>>> {
        let codecs = self.image_codecs.cloned().map(ImageCodecs::of);
        Ok(codecs.map(|codecs| self.table.push(codecs)).transpose()?)
    }
}

impl HostImageCodecs for ImageCodecsCtx<'_> {
    fn decoded_formats(&mut self, codecs: Resource<ImageCodecs>) -> wasmtime::Result<Vec<String>> {
        Ok(self.table.get(&codecs)?.decoded_formats())
    }

    fn encoded_formats(&mut self, codecs: Resource<ImageCodecs>) -> wasmtime::Result<Vec<String>> {
        Ok(self.table.get(&codecs)?.encoded_formats())
    }

    fn hardware_image_codecs(
        &mut self,
        codecs: Resource<ImageCodecs>,
    ) -> wasmtime::Result<Option<Resource<HardwareImageCodecs>>> {
        let hardware = self.table.get(&codecs)?.hardware_image_codecs();
        Ok(hardware
            .map(|hardware| self.table.push(hardware))
            .transpose()?)
    }

    fn drop(&mut self, codecs: Resource<ImageCodecs>) -> wasmtime::Result<()> {
        self.table.delete(codecs)?;
        Ok(())
    }
}

impl<T: ImageCodecsCtxView + 'static> HostImageCodecsWithStore<T> for HasImageCodecs {
    async fn open(
        accessor: &Accessor<T, Self>,
        codecs: Resource<ImageCodecs>,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<Resource<ImageSource>, ImageError>> {
        let opening = accessor.with(|mut access| {
            let context = access.get();
            wasmtime::Result::<_>::Ok(context.table.get(&codecs)?.open(context.program, data))
        })?;
        pushed(accessor, opening.await)
    }

    async fn open_file(
        accessor: &Accessor<T, Self>,
        codecs: Resource<ImageCodecs>,
        path: String,
    ) -> wasmtime::Result<Result<Resource<ImageSource>, ImageError>> {
        let opening = accessor.with(|mut access| {
            let context = access.get();
            let codecs = context.table.get(&codecs)?;
            wasmtime::Result::<_>::Ok(codecs.open_file(context.program, context.files, &path))
        })?;
        pushed(accessor, opening.await)
    }

    async fn encode_pixels(
        accessor: &Accessor<T, Self>,
        codecs: Resource<ImageCodecs>,
        pixels: PixelBuffer,
        format: String,
        quality: Option<f32>,
    ) -> wasmtime::Result<Result<Vec<u8>, ImageError>> {
        let encoding = accessor.with(|mut access| {
            let codecs = access.get().table.get(&codecs)?;
            codecs.encode_pixels(pixels, &format, quality)
        })?;
        Ok(encoding.await.and_then(fitting_a_list))
    }
}

impl HostImageSource for ImageCodecsCtx<'_> {
    fn mime_type(&mut self, image: Resource<ImageSource>) -> wasmtime::Result<String> {
        Ok(self.table.get(&image)?.mime_type())
    }

    fn size(&mut self, image: Resource<ImageSource>) -> wasmtime::Result<Size> {
        Ok(self.table.get(&image)?.size())
    }

    fn pictures(&mut self, image: Resource<ImageSource>) -> wasmtime::Result<ImagePictures> {
        Ok(self.table.get(&image)?.pictures())
    }

    fn orientation(&mut self, image: Resource<ImageSource>) -> wasmtime::Result<ImageOrientation> {
        Ok(self.table.get(&image)?.orientation())
    }

    fn metadata(&mut self, image: Resource<ImageSource>) -> wasmtime::Result<ImageMetadata> {
        Ok(self.table.get(&image)?.metadata())
    }

    /// The program lets go of the image; its decodings under way still answer.
    fn drop(&mut self, image: Resource<ImageSource>) -> wasmtime::Result<()> {
        self.table.delete(image)?;
        Ok(())
    }
}

impl<T: ImageCodecsCtxView + 'static> HostImageSourceWithStore<T> for HasImageCodecs {
    async fn decode_pixels(
        accessor: &Accessor<T, Self>,
        image: Resource<ImageSource>,
        index: u32,
        max_size: Option<Size>,
    ) -> wasmtime::Result<Result<PixelBuffer, ImageError>> {
        let decoding = accessor.with(|mut access| {
            access
                .get()
                .table
                .get(&image)?
                .decode_pixels(index, max_size)
        })?;
        Ok(decoding.await.and_then(pixels_fitting_a_list))
    }
}

impl HostHardwareImageCodecs for ImageCodecsCtx<'_> {
    fn drop(&mut self, codecs: Resource<HardwareImageCodecs>) -> wasmtime::Result<()> {
        self.table.delete(codecs)?;
        Ok(())
    }
}

impl<T: ImageCodecsCtxView + 'static> HostHardwareImageCodecsWithStore<T> for HasImageCodecs {
    /// A hardware buffer of the program's own, counted in its stats.
    async fn decode(
        accessor: &Accessor<T, Self>,
        codecs: Resource<HardwareImageCodecs>,
        image: Resource<ImageSource>,
        index: u32,
        max_size: Option<Size>,
    ) -> wasmtime::Result<Result<Resource<ProgramHardwareBuffer>, ImageError>> {
        let (decoding, stats) = accessor.with(|mut access| {
            let context = access.get();
            let table = &context.table;
            let decoding = table
                .get(&codecs)?
                .decode(table.get(&image)?, index, max_size)?;
            wasmtime::Result::<_>::Ok((decoding, context.program.shared_stats()))
        })?;
        let decoded = decoding
            .await
            .map(|decoded| ProgramHardwareBuffer::decoded(stats, decoded));
        match decoded {
            Ok(buffer) => accessor.with(|mut access| Ok(Ok(access.get().table.push(buffer)?))),
            Err(error) => Ok(Err(error)),
        }
    }

    async fn encode(
        accessor: &Accessor<T, Self>,
        codecs: Resource<HardwareImageCodecs>,
        buffer: Resource<ProgramHardwareBuffer>,
        format: String,
        quality: Option<f32>,
    ) -> wasmtime::Result<Result<Vec<u8>, ImageError>> {
        let (codecs, buffer, lifecycle, gpu) = accessor.with(|mut access| {
            let context = access.get();
            let table = &context.table;
            let codecs = table.get(&codecs)?.encoder(quality)?;
            let buffer = table.get(&buffer)?.clone();
            let lifecycle = context.program.lifecycle().clone();
            wasmtime::Result::<_>::Ok((codecs, buffer, lifecycle, Arc::clone(context.gpu)))
        })?;
        let work = buffer.hand_out(&gpu, &lifecycle).await;
        if !work.is_empty() {
            let waited = Arc::clone(&gpu);
            spawn_blocking(move || work.wait(&waited)).await;
        }
        let encoding = codecs.encode(&buffer.shell_buffer(), &format, quality);
        let encoded = encoding.await;
        buffer.take_back(&gpu, &lifecycle).await;
        // The buffer is held until the shell has encoded it, so its memory, a frame's among it,
        // is not handed out again meanwhile.
        drop(buffer);
        Ok(encoded.and_then(fitting_a_list))
    }
}

/// The image `opened`, pushed into the program's table, or why it did not open.
fn pushed<T: ImageCodecsCtxView + 'static>(
    accessor: &Accessor<T, HasImageCodecs>,
    opened: Result<ImageSource, ImageError>,
) -> wasmtime::Result<Result<Resource<ImageSource>, ImageError>> {
    match opened {
        Ok(image) => accessor.with(|mut access| Ok(Ok(access.get().table.push(image)?))),
        Err(error) => Ok(Err(error)),
    }
}

/// `pixels`, whose bytes a WIT list can carry; `too-large` where they are more than it can.
fn pixels_fitting_a_list(pixels: PixelBuffer) -> Result<PixelBuffer, ImageError> {
    fits_a_list(&pixels.bytes)?;
    Ok(pixels)
}

/// `bytes`, which a WIT list can carry; `too-large` where they are more than it can.
fn fitting_a_list(bytes: Vec<u8>) -> Result<Vec<u8>, ImageError> {
    fits_a_list(&bytes)?;
    Ok(bytes)
}

fn fits_a_list(bytes: &[u8]) -> Result<(), ImageError> {
    if u32::try_from(bytes.len()).is_ok() {
        return Ok(());
    }
    Err(ImageError {
        kind: ImageErrorKind::TooLarge,
        message: format!("{} bytes, more than a program's list holds", bytes.len()),
    })
}
