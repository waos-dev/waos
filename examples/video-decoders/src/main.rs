//! Hands the system's video decoder chunks and prints what comes of each.
//!
//! A decoder takes a key frame first, and again after a drain, a reset and a damaged chunk; any
//! other chunk is answered as bad data and the decoder goes on. Its frames are lent: holding as
//! many as the program said it would stops the decoder until one is dropped. And a decoder the
//! system takes away says so beside its frames. A frame's planes are read twice: by the GPU, each
//! through a texture, and copied into the program's memory.
//!
//! The chunks here are no real video: under WaOS's tests the decoder is the test shell's, which
//! puts a picture out for every chunk. A chunk whose first byte is [`DAMAGED`] it answers as
//! damaged.

use std::pin::pin;
use std::time::Duration;

use futures::future::{Either, select};
use wasdk::monotonic_clock;
use wasdk::video_decoders::{
    DecodingPurpose, VideoDecoder, VideoDecoderOptions, VideoStreamDescription,
};
use wasdk::video_frames::{PlaneFormat, VideoFrame};
use wasdk::video_streams::{
    ChromaSampling, CodecChoice, EncodedChunk, NalFormat, PictureSize, VideoCodec, VideoStreamSetup,
};

/// The first byte of a chunk the test shell's decoder answers as damaged.
const DAMAGED: u8 = 0xBD;

/// The bytes of a row a texture is copied out in: WebGPU's alignment for one.
const COPY_ROW: u32 = 256;

/// How long no third frame comes while two are held, before the program takes it that none
/// will.
const HELD_FOR: Duration = Duration::from_millis(300);

#[wasdk::main]
async fn main() -> Result<(), String> {
    let (device, queue) = gpu().await?;
    let Some(decoders) = wasdk_wgpu::video_decoders(&device) else {
        println!("video decoders: none");
        return Ok(());
    };
    let support = decoders
        .support(VideoStreamDescription {
            codec: VideoCodec::H264,
            profile: Some(100),
            level: Some(40),
            bit_depth: 8,
            chroma: ChromaSampling::Yuv420,
            size: PictureSize {
                width: 1920,
                height: 1080,
            },
            frame_rate: Some(30.0),
        })
        .await;
    println!(
        "support: hardware {:?}, software {:?}",
        support.hardware, support.software
    );

    let decoder = decoders
        .open(
            VideoStreamSetup::H264(NalFormat::StartCodes(None)),
            VideoDecoderOptions {
                decoder: CodecChoice::Any,
                purpose: DecodingPurpose::Playback,
                frame_limit: 2,
                largest_size: None,
                default_color: None,
            },
        )
        .await
        .map_err(|error| format!("no decoder: {error:?}"))?;
    println!("opened a {:?} decoder", decoder.kind());
    let (mut frames, ended) = decoder.frames();

    println!(
        "a chunk before any key frame: {:?}",
        decoder.decode(chunk(0, false)).await
    );
    decode(&decoder, chunk(0, true)).await?;
    for time in 1..4 {
        decode(&decoder, chunk(time, false)).await?;
    }

    let first = next(&mut frames).await?;
    let second = next(&mut frames).await?;
    let third = frame_once_one_is_dropped(&mut frames, first, &second).await?;
    println!("after one is dropped: frame {:?}", third.timestamp());
    drop((second, third));
    let fourth = next(&mut frames).await?;
    println!("{}", description(&fourth));
    for line in planes_read(&device, &queue, &fourth).await? {
        println!("{line}");
    }
    for line in planes_in_memory(&fourth).await? {
        println!("{line}");
    }
    drop(fourth);

    decoder
        .drain()
        .await
        .map_err(|error| format!("the decoder did not drain: {error:?}"))?;
    println!(
        "a chunk that is no key frame after a drain: {:?}",
        decoder.decode(chunk(4, false)).await
    );
    decode(&decoder, chunk(5, true)).await?;
    drop(next(&mut frames).await?);
    println!(
        "a damaged chunk: {:?}",
        decoder.decode(damaged_chunk(6)).await
    );
    println!(
        "the chunk after it: {:?}",
        decoder.decode(chunk(7, false)).await
    );
    decoder.reset();
    println!(
        "a chunk that is no key frame after a reset: {:?}",
        decoder.decode(chunk(8, false)).await
    );
    decode(&decoder, chunk(9, true)).await?;
    let last = next(&mut frames).await?;
    println!("a key frame goes on: frame {:?}", last.timestamp());

    println!("waiting for the decoder to end");
    println!("the decoder ended: {:?}", ended.await);
    Ok(())
}

/// A chunk shown at `time`, of bytes that are no video.
fn chunk(time: i64, key_frame: bool) -> EncodedChunk {
    EncodedChunk {
        data: vec![0, 0, 0, 1],
        timestamp: time,
        duration: Some(40),
        key_frame,
    }
}

/// A key frame the test shell's decoder answers as damaged.
fn damaged_chunk(time: i64) -> EncodedChunk {
    EncodedChunk {
        data: vec![DAMAGED],
        ..chunk(time, true)
    }
}

/// Hands `chunk` to `decoder`, which takes it.
async fn decode(decoder: &VideoDecoder, chunk: EncodedChunk) -> Result<(), String> {
    let time = chunk.timestamp;
    decoder
        .decode(chunk)
        .await
        .map_err(|error| format!("the chunk at {time} was refused: {error:?}"))
}

/// The decoder's next frame.
async fn next(frames: &mut wasdk::StreamReader<VideoFrame>) -> Result<VideoFrame, String> {
    frames
        .next()
        .await
        .ok_or_else(|| "the frames ended early".to_owned())
}

/// The frame that comes once `first` is dropped, having printed whether one came while `first`
/// and `second`, as many as the decoder lends, were both held.
async fn frame_once_one_is_dropped(
    frames: &mut wasdk::StreamReader<VideoFrame>,
    first: VideoFrame,
    second: &VideoFrame,
) -> Result<VideoFrame, String> {
    let mut third = pin!(frames.next());
    let held_for = pin!(monotonic_clock::wait_for(HELD_FOR.as_nanos() as u64));
    match select(&mut third, held_for).await {
        Either::Left((came, _)) => {
            println!("a third frame came while two were held");
            return came.ok_or_else(|| "the frames ended early".to_owned());
        }
        Either::Right(_) => println!(
            "frames {:?} and {:?} held: no third while they are",
            first.timestamp(),
            second.timestamp()
        ),
    }
    drop(first);
    third
        .await
        .ok_or_else(|| "the frames ended early".to_owned())
}

/// Each of `frame`'s planes, with its first sample as `device`'s GPU reads it through a texture
/// imported of the plane.
async fn planes_read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &VideoFrame,
) -> Result<Vec<String>, String> {
    let planes = frame.planes().ok_or("the frame has no planes")?;
    let buffer = frame.hardware_buffer();
    let mut read = Vec::new();
    for (index, plane) in planes.iter().enumerate() {
        let texture = wasdk_wgpu::import_plane(device, &buffer, index as u32);
        let sample = first_sample(device, queue, &texture).await?;
        read.push(format!(
            "plane {index}: {:?} as {:?}, {}x{} of {} bits, first sample {sample:?}",
            plane.content, plane.format, plane.width, plane.height, plane.bit_depth
        ));
    }
    Ok(read)
}

/// Each of `frame`'s planes as it is copied into the program's memory: how many rows it has, and
/// the one sample all of them hold, the test's picture being one colour.
async fn planes_in_memory(frame: &VideoFrame) -> Result<Vec<String>, String> {
    let planes = frame.planes().ok_or("the frame has no planes")?;
    let copied = frame
        .read_planes()
        .await
        .map_err(|error| format!("the planes were not read: {error:?}"))?;
    let mut read = Vec::new();
    for (index, (plane, pixels)) in planes.iter().zip(&copied).enumerate() {
        let sample_bytes = sample_bytes(plane.format);
        let rows = pixels.bytes.chunks(pixels.row_bytes as usize);
        let row_count = rows.len();
        let mut samples =
            rows.flat_map(|row| row[..plane.width as usize * sample_bytes].chunks(sample_bytes));
        let first = samples.next().ok_or("a plane with no samples")?;
        if samples.any(|sample| sample != first) {
            return Err(format!("plane {index}'s samples differ"));
        }
        read.push(format!(
            "plane {index} in memory: {row_count} rows, every sample {first:?}"
        ));
    }
    Ok(read)
}

/// The bytes one sample of a plane in `format` takes.
fn sample_bytes(format: PlaneFormat) -> usize {
    match format {
        PlaneFormat::R8 => 1,
        PlaneFormat::Rg8 | PlaneFormat::R16 => 2,
        PlaneFormat::Rg16 | PlaneFormat::Rgba8 | PlaneFormat::Bgra8 => 4,
    }
}

/// The engine's WebGPU device and its queue.
async fn gpu() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no WebGPU adapter: {error}"))?;
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|error| format!("no WebGPU device: {error}"))
}

/// The bytes of `texture`'s first sample, copied out on the GPU: one for each of its channels.
async fn first_sample(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<Vec<u8>, String> {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("first sample"),
        size: u64::from(COPY_ROW),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(COPY_ROW),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    queue.submit([encoder.finish()]);
    if let Some(error) = scope.pop().await {
        return Err(format!("the plane was not copied out: {error}"));
    }
    let (mapped, answered) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = mapped.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the device did not poll: {error}"))?;
    answered
        .try_recv()
        .map_err(|_| "the mapping was not answered")?
        .map_err(|error| format!("the buffer did not map: {error}"))?;
    // A mapped range is read four bytes at a time, as WebGPU aligns one.
    let bytes = buffer
        .slice(..4)
        .get_mapped_range()
        .map_err(|error| format!("the sample cannot be read: {error}"))?;
    let channels = usize::from(texture.format().components());
    Ok(bytes[..channels].to_vec())
}

/// What `frame` says of itself, as its decoder said it.
fn description(frame: &VideoFrame) -> String {
    let visible = frame.visible_rect();
    let aspect = frame.pixel_aspect();
    let buffer = frame.hardware_buffer();
    format!(
        "frame {:?} for {:?}: {}x{} of a {}x{} buffer from {},{}, pixels {} to {}, primaries {:?}",
        frame.timestamp(),
        frame.duration(),
        visible.width,
        visible.height,
        buffer.width(),
        buffer.height(),
        visible.x,
        visible.y,
        aspect.width,
        aspect.height,
        frame.color().primaries
    )
}
