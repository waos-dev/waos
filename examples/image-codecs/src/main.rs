//! A program that decodes and encodes images through the system's codecs: it says the formats they
//! decode and encode, as `formats: decodes image/jpeg, image/png; encodes image/png`, and
//! `hardware codecs: none` where the system has none; then, for each image listed in
//! `/data/sources`, one a line, a path opened as a file or, after `bytes `, read and opened from its
//! bytes, it says what the image says of itself, as `source: image/jpeg 32x48, frames: still,
//! turned quarter-turn`, when and with what it was taken and where, as `source: taken 2024-05-01
//! 10:20:30 +02:00`, `source: camera Waft Test Camera, lens Test Lens`, `source: exposure 1/120 s,
//! f/1.8, ISO 200` and `source: location 48.85083, -2.29500, 35 m`; its first frame decoded into
//! its own memory, no larger than 16 by 16, by its size, its layout and the colours at its top and
//! its bottom, as `source: pixels 11x16 rgba8 premultiplied, red above blue`; where the system can,
//! decoded whole into a hardware buffer, saying what the buffer can be used for, and read back, as
//! `source: hardware buffer 32x48 for gpu-sampled, composer-overlay, cpu-read, read 32x48 bgra8
//! premultiplied, red above blue` on Apple's systems; and both encoded again, the pixels as PNG and the hardware buffer
//! as JPEG, each opened again by its size, as `source: encoded image/png, opens 11x16`; or why a
//! step failed, as `source: open failed (not-found: why)`. Then it ends. Where the system has no
//! image codecs it says `image codecs: none`.
//!
//! ```text
//! cargo build -p image-codecs --target wasm32-wasip3
//! ```

use std::fs;

use wasdk::hardware_buffers::{
    AlphaType, HardwareBuffer, HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer,
    PixelFormat,
};
use wasdk::image_codecs::{
    self, HardwareImageCodecs, ImageCodecs, ImageFrame, ImageMetadata, ImageRotation, ImageSource,
};
use wasdk::windows::Size;

/// What lists the images it opens.
const SOURCES: &str = "/data/sources";

/// The largest its pixels are decoded at.
const PIXELS_SIZE: Size = Size {
    width: 16.0,
    height: 16.0,
};

#[wasdk::main]
async fn main() {
    let Some(codecs) = image_codecs::get_image_codecs() else {
        println!("image codecs: none");
        return;
    };
    println!(
        "formats: decodes {}; encodes {}",
        codecs.decoded_formats().join(", "),
        codecs.encoded_formats().join(", ")
    );
    let hardware = codecs.hardware_image_codecs();
    if hardware.is_none() {
        println!("hardware codecs: none");
    }
    let sources = fs::read_to_string(SOURCES).unwrap_or_default();
    for source in sources.lines() {
        let say = |what: String| println!("{source}: {what}");
        match opened(&codecs, source).await {
            Ok(image) => go_through(&codecs, hardware.as_ref(), &image, say).await,
            Err(error) => say(format!("open failed ({})", described_error(&error))),
        }
    }
}

/// The image `source` names: a file's path, or after `bytes `, the file's bytes.
async fn opened(codecs: &ImageCodecs, source: &str) -> Result<ImageSource, ImageError> {
    match source.strip_prefix("bytes ") {
        Some(path) => {
            let bytes = fs::read(path).map_err(|error| ImageError {
                kind: ImageErrorKind::NotFound,
                message: error.to_string(),
            })?;
            codecs.open(bytes).await
        }
        None => codecs.open_file(source.to_owned()).await,
    }
}

/// Says what `image` says of itself, then decodes it into both memories and encodes each again.
async fn go_through(
    codecs: &ImageCodecs,
    hardware: Option<&HardwareImageCodecs>,
    image: &ImageSource,
    say: impl Fn(String),
) {
    say(described_image(image));
    for line in described_metadata(&image.metadata()) {
        say(line);
    }
    let pixels = image.decode_pixels(0, Some(PIXELS_SIZE)).await;
    say(answered("pixels", &pixels, described_pixels));
    if let Ok(pixels) = pixels {
        let encoded = codecs
            .encode_pixels(pixels, "image/png".to_owned(), None)
            .await;
        say(reopened(codecs, "encoded image/png", encoded).await);
    }
    let Some(hardware) = hardware else {
        return;
    };
    let buffer = hardware.decode(image, 0, None).await;
    let Ok(buffer) = buffer else {
        say(answered("hardware buffer", &buffer, |_| String::new()));
        return;
    };
    say(described_buffer(&buffer).await);
    let encoded = hardware
        .encode(&buffer, "image/jpeg".to_owned(), Some(0.8))
        .await;
    say(reopened(codecs, "hardware encoded image/jpeg", encoded).await);
}

/// `what` and the size the bytes `encoded` open at, or why not.
async fn reopened(
    codecs: &ImageCodecs,
    what: &str,
    encoded: Result<Vec<u8>, ImageError>,
) -> String {
    let reopened = match encoded {
        Ok(bytes) => codecs.open(bytes).await,
        Err(error) => Err(error),
    };
    answered(what, &reopened, |image| {
        format!("opens {}", sized(image.size()))
    })
}

/// The image's format, size, frames and how it is turned.
fn described_image(image: &ImageSource) -> String {
    let orientation = image.orientation();
    let turned = match orientation.rotation {
        ImageRotation::None => "upright",
        ImageRotation::QuarterTurn => "turned quarter-turn",
        ImageRotation::HalfTurn => "turned half-turn",
        ImageRotation::ThreeQuarterTurn => "turned three-quarter-turn",
    };
    let mirrored = if orientation.mirrored {
        ", mirrored"
    } else {
        ""
    };
    format!(
        "{} {}, frames: {}, {turned}{mirrored}",
        image.mime_type(),
        sized(image.size()),
        described_frames(&image.frames())
    )
}

/// `still` for one frame without a duration, else each frame's, in milliseconds, or `-`.
fn described_frames(frames: &[ImageFrame]) -> String {
    if let [ImageFrame { duration: None }] = frames {
        return "still".to_owned();
    }
    let durations: Vec<String> = frames
        .iter()
        .map(|frame| match frame.duration {
            Some(nanoseconds) => format!("{}ms", nanoseconds / 1_000_000),
            None => "-".to_owned(),
        })
        .collect();
    durations.join(" ")
}

/// When, with what, how and where it was taken, a line each, each saying `none` where the file says
/// nothing of it.
fn described_metadata(metadata: &ImageMetadata) -> Vec<String> {
    let taken = metadata.taken.map_or("none".to_owned(), |taken| {
        let offset = taken.utc_offset.map_or(String::new(), |minutes| {
            let sign = if minutes < 0 { '-' } else { '+' };
            let minutes = minutes.unsigned_abs();
            format!(" {sign}{:02}:{:02}", minutes / 60, minutes % 60)
        });
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{offset}",
            taken.year, taken.month, taken.day, taken.hour, taken.minute, taken.second
        )
    });
    let camera: Vec<String> = [metadata.camera_make.clone(), metadata.camera_model.clone()]
        .into_iter()
        .flatten()
        .collect();
    let camera = match (camera.is_empty(), &metadata.lens_model) {
        (true, None) => "none".to_owned(),
        (_, Some(lens)) => format!("{}, lens {lens}", camera.join(" ")),
        (false, None) => camera.join(" "),
    };
    let location = metadata.location.map_or("none".to_owned(), |place| {
        let altitude = place
            .altitude
            .map_or(String::new(), |metres| format!(", {metres:.0} m"));
        format!("{:.5}, {:.5}{altitude}", place.latitude, place.longitude)
    });
    vec![
        format!("taken {taken}"),
        format!("camera {camera}"),
        format!("exposure {}", described_exposure(metadata)),
        format!("location {location}"),
    ]
}

/// The exposure as a photo app shows it, each part the file gives.
fn described_exposure(metadata: &ImageMetadata) -> String {
    let mut parts = Vec::new();
    if let Some(seconds) = metadata.exposure_time {
        parts.push(if seconds < 1.0 {
            format!("1/{:.0} s", 1.0 / seconds)
        } else {
            format!("{seconds} s")
        });
    }
    if let Some(f_number) = metadata.f_number {
        parts.push(format!("f/{f_number}"));
    }
    if let Some(iso) = metadata.iso {
        parts.push(format!("ISO {iso}"));
    }
    if let Some(focal_length) = metadata.focal_length {
        let equivalent = metadata
            .equivalent_focal_length
            .map_or(String::new(), |equivalent| {
                format!(" ({equivalent} mm equivalent)")
            });
        parts.push(format!("{focal_length} mm{equivalent}"));
    }
    if let Some(stops) = metadata.exposure_bias {
        parts.push(format!("{stops:+.2} EV"));
    }
    if let Some(fired) = metadata.flash_fired {
        parts.push(if fired { "flash fired" } else { "no flash" }.to_owned());
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join(", ")
    }
}

/// The hardware buffer's size, and its pixels read back.
async fn described_buffer(buffer: &HardwareBuffer) -> String {
    let read = buffer.read_pixels().await;
    let size = format!("{}x{}", buffer.width(), buffer.height());
    let usage = described_usage(buffer.usage());
    answered(
        &format!("hardware buffer {size} for {usage}, read"),
        &read,
        described_pixels,
    )
}

/// The flags of `usage`, as the WIT names them, in its order.
fn described_usage(usage: HardwareBufferUsage) -> String {
    let named = [
        (HardwareBufferUsage::GPU_SAMPLED, "gpu-sampled"),
        (HardwareBufferUsage::GPU_COLOR_OUTPUT, "gpu-color-output"),
        (HardwareBufferUsage::COMPOSER_OVERLAY, "composer-overlay"),
        (HardwareBufferUsage::CPU_READ, "cpu-read"),
    ];
    let names: Vec<&str> = named
        .into_iter()
        .filter(|(flag, _)| usage.contains(*flag))
        .map(|(_, name)| name)
        .collect();
    names.join(", ")
}

/// The pixels' size, layout, and the colours at their top and bottom.
fn described_pixels(pixels: &PixelBuffer) -> String {
    let format = match pixels.format {
        PixelFormat::Rgba8 => "rgba8",
        PixelFormat::Bgra8 => "bgra8",
    };
    let alpha = match pixels.alpha {
        AlphaType::Premultiplied => "premultiplied",
        AlphaType::Straight => "straight",
    };
    let top = colour_at(pixels, 1.min(pixels.height - 1));
    let bottom = colour_at(pixels, pixels.height.saturating_sub(2));
    let colours = if top == bottom {
        format!("all {top}")
    } else {
        format!("{top} above {bottom}")
    };
    format!(
        "{}x{} {format} {alpha}, {colours}",
        pixels.width, pixels.height
    )
}

/// The colour of the pixel at the middle of `row`, by name.
fn colour_at(pixels: &PixelBuffer, row: u32) -> &'static str {
    let start = (row * pixels.row_bytes + pixels.width / 2 * 4) as usize;
    let [first, green, third, alpha] = [0, 1, 2, 3].map(|channel| pixels.bytes[start + channel]);
    let (red, blue) = match pixels.format {
        PixelFormat::Rgba8 => (first, third),
        PixelFormat::Bgra8 => (third, first),
    };
    match (red, green, blue, alpha) {
        (_, _, _, alpha) if alpha < 128 => "clear",
        (red, green, blue, _) if red > 150 && green < 100 && blue < 100 => "red",
        (red, green, blue, _) if green > 150 && red < 100 && blue < 100 => "green",
        (red, green, blue, _) if blue > 150 && red < 100 && green < 100 => "blue",
        (red, green, blue, _) if red > 200 && green > 200 && blue > 200 => "white",
        (red, green, blue, _) if red < 60 && green < 60 && blue < 60 => "black",
        _ => "mixed",
    }
}

fn sized(size: Size) -> String {
    format!("{}x{}", size.width, size.height)
}

/// `what` and `answer` in a few words: as `say` words it, or why it failed.
fn answered<T>(what: &str, answer: &Result<T, ImageError>, say: impl Fn(&T) -> String) -> String {
    match answer {
        Ok(value) => format!("{what} {}", say(value)).trim_end().to_owned(),
        Err(error) => format!("{what} failed ({})", described_error(error)),
    }
}

/// A failure's kind as the WIT names it, and the system's words.
fn described_error(error: &ImageError) -> String {
    let kind = match error.kind {
        ImageErrorKind::Unsupported => "unsupported",
        ImageErrorKind::Damaged => "damaged",
        ImageErrorKind::TooLarge => "too-large",
        ImageErrorKind::NotFound => "not-found",
        ImageErrorKind::OutOfMemory => "out-of-memory",
        ImageErrorKind::Other => "other",
    };
    format!("{kind}: {}", error.message)
}
