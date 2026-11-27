//! A program that draws a player's frames itself: it makes a frame reader on the system's readers,
//! gives it to a player of `/data/clip.mp4` as its picture's destination and plays it, saying
//! `frames to the reader`; then, in a view of its own, asks the reader at each frame for the frame
//! of that frame's display time, until one comes, and says the frame's size and its first pixel,
//! read back through a texture it imports of its hardware buffer on its own device, as
//! `frame 64x48, first pixel [0, 0, 255, 255]`, the bytes as they are in memory. Where the system
//! hands out no frames it says `video frame readers: none`, and where it has no players
//! `media players: none`.
//!
//! ```text
//! cargo build -p video-frames --target wasm32-wasip3
//! ```

use wasdk::media_players::{self, MediaPlayer};
use wasdk::video_frames::{self, VideoFrame, VideoFrameReader, VideoOutput};
use wasdk::views::{self, View, ViewChange, ViewEvent};
use wasdk::windows::WindowConfig;

/// What the player plays.
const CLIP: &str = "/data/clip.mp4";

/// Bytes a row of a texture copy takes: WebGPU's alignment, enough for the one pixel read.
const COPY_ROW: u32 = 256;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let Some(readers) = video_frames::get_video_frame_readers() else {
        println!("video frame readers: none");
        return Ok(());
    };
    let reader = readers.make_reader();
    let Some(_player) = play_into(&reader) else {
        println!("media players: none");
        return Ok(());
    };
    println!("frames to the reader");
    let (device, queue) = gpu().await?;
    let view = first_view().await?;
    let frame = first_frame(&view, &reader).await?;
    let buffer = frame.hardware_buffer();
    let texture = wasdk_wgpu::import_texture(&device, &buffer);
    let pixel = first_pixel(&device, &queue, &texture, wgpu::TextureAspect::All)?;
    println!(
        "frame {}x{}, first pixel {pixel:?}",
        buffer.width(),
        buffer.height()
    );
    println!("{}", description(&frame)?);
    let planes = wasdk_wgpu::import_planes(&device, &buffer);
    let plane = planes.first().ok_or("the frame imported no planes")?;
    let pixel = first_pixel(&device, &queue, &plane.texture, plane.aspect)?;
    println!("first pixel through its plane {pixel:?}");
    let copied = frame
        .read_planes()
        .await
        .map_err(|error| format!("the planes were not read: {error:?}"))?;
    println!("first pixel in memory {:?}", &copied[0].bytes[..4]);
    Ok(())
}

/// What `frame` says of itself: its planes, how much of its buffer is picture, and its time.
fn description(frame: &VideoFrame) -> Result<String, String> {
    let planes = frame.planes().ok_or("the frame has no planes")?;
    let planes: Vec<String> = planes
        .iter()
        .map(|plane| {
            format!(
                "{:?} {:?} {}x{} of {} bits",
                plane.content, plane.format, plane.width, plane.height, plane.bit_depth
            )
        })
        .collect();
    let visible = frame.visible_rect();
    let color = frame.color();
    Ok(format!(
        "planes [{}], visible {}x{} from {},{}, matrix {:?} over {:?} range, time {:?}",
        planes.join(", "),
        visible.width,
        visible.height,
        visible.x,
        visible.y,
        color.matrix,
        color.range,
        frame.timestamp()
    ))
}

/// A player of [`CLIP`], playing into `reader`; none where the system has no players.
fn play_into(reader: &VideoFrameReader) -> Option<MediaPlayer> {
    let players = media_players::get_media_players()?;
    let player = players.open(CLIP);
    player.set_video_output(Some(&VideoOutput::Frames(reader)));
    player.play();
    Some(player)
}

/// The engine's WebGPU device and its queue.
async fn gpu() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wasdk_wgpu::instance();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| format!("no WebGPU adapter: {error}"))?;
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            // wgpu's defaults within what the adapter has: some fall below them, as the
            // iOS simulator's does.
            required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
            ..Default::default()
        })
        .await
        .map_err(|error| format!("no WebGPU device: {error}"))
}

/// The first frame `reader` answers, asked at each of `view`'s frames for that frame's display
/// time.
async fn first_frame(view: &View, reader: &VideoFrameReader) -> Result<VideoFrame, String> {
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    view.request_frame();
    while let Some(event) = events.next().await {
        if let ViewEvent::Frame(frame) = event {
            if let Some(video_frame) = reader.frame_for(frame.display_time) {
                return Ok(video_frame);
            }
            view.request_frame();
        }
    }
    Err("the view's events ended before a frame came".into())
}

/// The bytes of the first pixel of `texture`'s `aspect`, copied out on the GPU.
fn first_pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    aspect: wgpu::TextureAspect,
) -> Result<[u8; 4], String> {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("first pixel"),
        size: u64::from(COPY_ROW),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            aspect,
            ..texture.as_image_copy()
        },
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
    let bytes = buffer
        .slice(..4)
        .get_mapped_range()
        .map_err(|error| format!("the mapping cannot be read: {error}"))?;
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The view that arrives for the program's request.
async fn first_view() -> Result<View, String> {
    // The stream is opened before the request, so the view that arrives for it is not missed.
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("Video frames");
    views::request_view(&config, None)
        .map_err(|error| format!("no view for this program: {error}"))?;
    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(view) => return Ok(view),
            ViewChange::RequestFailed(failure) => {
                return Err(format!(
                    "request {} failed: {}",
                    failure.request, failure.error
                ));
            }
            ViewChange::Removed(_) => {}
        }
    }
    Err("the views ended before one arrived".into())
}
