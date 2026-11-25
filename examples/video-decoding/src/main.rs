//! Decodes an H.264 stream with the system's decoder and prints each frame as it comes out.
//!
//! The program reads the stream itself, as a player reads a file: `/data/clip.h264`, in Annex B,
//! with a delimiter before each picture's units, as x264 writes one (`aud=1`; `clip.h264` beside
//! this crate is such a stream). An elementary stream carries no times, so when each picture is
//! shown comes beside it, in `/data/shown`: a number a line, in the order the pictures are
//! decoded, counted in pictures.
//!
//! The pictures are handed over in the order they are decoded and come out in the order they are
//! shown. The decoder lends two frames at once and holds back while they wait unread, so the
//! chunks are handed over and the frames read at the same time.

use std::fs;

use futures::join;
use wasdk::video_decoders::{self, DecodingPurpose, VideoDecoder, VideoDecoderOptions};
use wasdk::video_frames::{PlaneFormat, VideoFrame};
use wasdk::video_streams::{CodecChoice, EncodedChunk, NalFormat, VideoStreamSetup};

const STREAM: &str = "/data/clip.h264";
const SHOWN: &str = "/data/shown";

/// How long each picture is shown, in nanoseconds: thirty a second.
const PICTURE_TIME: u64 = 33_333_333;

/// The kinds of unit the program tells apart, as a unit's first byte names them.
const KEY_PICTURE: u8 = 5;
const DELIMITER: u8 = 9;

/// The bytes of a row a texture is copied out in: WebGPU's alignment for one.
const COPY_ROW: u32 = 256;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let Some(decoders) = video_decoders::get_video_decoders() else {
        println!("video decoders: none");
        return Ok(());
    };
    let chunks = chunks()?;
    let count = chunks.len();
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
    println!("decoding {count} pictures");
    let (mut frames, _ended) = decoder.frames();
    let handing_over = hand_over(&decoder, chunks);
    let reading = async {
        for number in 0..count {
            let frame = frames.next().await.ok_or("the frames ended early")?;
            println!("{}", description(&frame));
            if number == 0 {
                println!("its first samples: {}", first_samples(&frame).await?);
                println!("and in memory: {}", first_samples_in_memory(&frame).await?);
            }
        }
        Ok::<(), String>(())
    };
    let (handed_over, read) = join!(handing_over, reading);
    handed_over?;
    read
}

/// The stream's pictures as chunks, in the order they are decoded, each shown when `SHOWN`
/// says.
fn chunks() -> Result<Vec<EncodedChunk>, String> {
    let stream = fs::read(STREAM).map_err(|error| format!("{STREAM}: {error}"))?;
    let shown = fs::read_to_string(SHOWN).map_err(|error| format!("{SHOWN}: {error}"))?;
    let shown: Vec<i64> = shown
        .lines()
        .map(|line| line.trim().parse())
        .collect::<Result<_, _>>()
        .map_err(|error| format!("{SHOWN}: {error}"))?;
    let pictures = pictures(&stream);
    if pictures.len() != shown.len() {
        return Err(format!(
            "{} pictures, and {} times for them",
            pictures.len(),
            shown.len()
        ));
    }
    Ok(pictures
        .into_iter()
        .zip(shown)
        .map(|(picture, shown)| EncodedChunk {
            key_frame: units(picture).any(|unit| kind(unit) == Some(KEY_PICTURE)),
            data: picture.to_vec(),
            timestamp: shown * PICTURE_TIME as i64,
            duration: Some(PICTURE_TIME),
        })
        .collect())
}

/// `stream` cut into its pictures: each from a delimiter's start code to the next's.
fn pictures(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts: Vec<usize> = start_codes(stream)
        .filter(|(_, unit_at)| stream.get(*unit_at).map(|header| header & 0x1F) == Some(DELIMITER))
        .map(|(start_code_at, _)| start_code_at)
        .collect();
    starts.push(stream.len());
    starts
        .windows(2)
        .map(|picture| &stream[picture[0]..picture[1]])
        .collect()
}

/// The units of `picture`, each without its start code.
fn units(picture: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut unit_starts: Vec<usize> = start_codes(picture).map(|(_, unit_at)| unit_at).collect();
    unit_starts.push(picture.len());
    (0..unit_starts.len() - 1).map(move |unit| &picture[unit_starts[unit]..unit_starts[unit + 1]])
}

/// What kind of unit `unit` is, as its first byte says.
fn kind(unit: &[u8]) -> Option<u8> {
    unit.first().map(|header| header & 0x1F)
}

/// Where each start code of `stream` begins, the zero before a three-byte one counted in, and
/// where the unit after it does.
fn start_codes(stream: &[u8]) -> impl Iterator<Item = (usize, usize)> {
    (0..stream.len().saturating_sub(2))
        .filter(|at| stream[*at..*at + 3] == [0, 0, 1])
        .map(|at| {
            let begins = if at > 0 && stream[at - 1] == 0 {
                at - 1
            } else {
                at
            };
            (begins, at + 3)
        })
}

/// Hands each of `chunks` to `decoder` as it has room, then waits for every picture to come out.
async fn hand_over(decoder: &VideoDecoder, chunks: Vec<EncodedChunk>) -> Result<(), String> {
    for chunk in chunks {
        let time = chunk.timestamp;
        decoder
            .decode(chunk)
            .await
            .map_err(|error| format!("the chunk at {time} was refused: {error:?}"))?;
    }
    decoder
        .drain()
        .await
        .map_err(|error| format!("the decoder did not drain: {error:?}"))
}

/// What `frame` says of itself.
fn description(frame: &VideoFrame) -> String {
    let visible = frame.visible_rect();
    let planes = frame.planes().map_or(0, |planes| planes.len());
    let shown = frame.timestamp().unwrap_or(-1) / PICTURE_TIME as i64;
    format!(
        "picture {shown}: {}x{} in {planes} planes",
        visible.width, visible.height
    )
}

/// The first sample of each of `frame`'s planes, as the GPU reads it through a texture imported
/// of the plane.
async fn first_samples(frame: &VideoFrame) -> Result<String, String> {
    let planes = frame.planes().ok_or("the frame has no planes")?;
    let (device, queue) = gpu().await?;
    let buffer = frame.hardware_buffer();
    let mut read = Vec::new();
    for (index, plane) in planes.iter().enumerate() {
        let texture = wasdk_wgpu::import_plane(&device, &buffer, index as u32);
        let sample = first_sample(&device, &queue, &texture)?;
        read.push(format!("{:?} {sample:?}", plane.content));
    }
    Ok(read.join(", "))
}

/// The first sample of each of `frame`'s planes, as the planes are copied into the program's
/// memory.
async fn first_samples_in_memory(frame: &VideoFrame) -> Result<String, String> {
    let planes = frame.planes().ok_or("the frame has no planes")?;
    let copied = frame
        .read_planes()
        .await
        .map_err(|error| format!("the planes were not read: {error:?}"))?;
    let read: Vec<String> = planes
        .iter()
        .zip(&copied)
        .map(|(plane, pixels)| {
            let sample = &pixels.bytes[..sample_bytes(plane.format)];
            format!("{:?} {sample:?}", plane.content)
        })
        .collect();
    Ok(read.join(", "))
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
fn first_sample(
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
    queue.submit([encoder.finish()]);
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
