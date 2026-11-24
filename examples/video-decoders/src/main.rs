//! Hands the system's video decoder chunks and prints what comes of each.
//!
//! A decoder takes a key frame first, and again after a drain, a reset and a damaged chunk; any
//! other chunk is answered as bad data and the decoder goes on. Its frames are lent: holding as
//! many as the program said it would stops the decoder until one is dropped. And a decoder the
//! system takes away says so beside its frames.
//!
//! The chunks here are no real video: under WaOS's tests the decoder is the test shell's, which
//! puts a picture out for every chunk. A chunk whose first byte is [`DAMAGED`] it answers as
//! damaged.

use std::pin::pin;
use std::time::Duration;

use futures::future::{Either, select};
use wasdk::monotonic_clock;
use wasdk::video_decoders::{
    self, DecodingPurpose, VideoDecoder, VideoDecoderOptions, VideoStreamDescription,
};
use wasdk::video_frames::VideoFrame;
use wasdk::video_streams::{
    ChromaSampling, CodecChoice, EncodedChunk, NalFormat, PictureSize, VideoCodec, VideoStreamSetup,
};

/// The first byte of a chunk the test shell's decoder answers as damaged.
const DAMAGED: u8 = 0xBD;

/// How long no third frame comes while two are held, before the program takes it that none
/// will.
const HELD_FOR: Duration = Duration::from_millis(300);

#[wasdk::main]
async fn main() -> Result<(), String> {
    let Some(decoders) = video_decoders::get_video_decoders() else {
        println!("video decoders: none");
        return Ok(());
    };
    let support = decoders.support(VideoStreamDescription {
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
    });
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
