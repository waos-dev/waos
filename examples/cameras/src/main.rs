//! A program that runs a camera through the system's cameras: it says each camera listed, as
//! `camera front: FaceTime HD Camera (front)`, and from then on the ids of those listed each time a
//! camera comes or goes, as `cameras now: front, desk`; it opens the first, which asks its user at the first
//! use, saying `open front: not allowed` where the user refuses, says each of its formats, as
//! `format 1920x1080 at 15-30 fps, photos 1920x1080 1280x720`, and starts it in the last, its
//! preview on an opaque composition layer of its own and its frames in a frame reader of its own,
//! each where the system has them, and its photos at the format's first photo size, saying the
//! format it runs in, as `started in 640x480`, or why it did not start. Where its `/data/unlisted`
//! exists it starts it in a format of its own making instead, which the camera does not list. Then
//! it says the turns that keep its picture level, as `turned: preview 0, capture 0`, and again as
//! they change, takes a photo, saying `photo: image/jpeg, 3 bytes` or why not, and says the
//! session's state,
//! as `state: running`, and each state after, as `state: interrupted (background)`, or
//! `state: interrupted` where the system gives no reason, until the session ends, as `state: ended (unplugged)`, when it lets the session go and returns. Where the
//! system has no cameras it says `cameras: none`.
//!
//! ```text
//! cargo build -p cameras --target wasm32-wasip3
//! ```

use std::path::Path;

use wasdk::cameras::{
    self, CameraError, CameraFacing, CameraFormat, CameraInfo, CameraInterruption, CameraOutputs,
    Cameras, CaptureSession, CaptureState, VideoRotationAngles,
};
use wasdk::video_frames;
use wasdk::views;
use wasdk::windows::Size;

/// Whose being there starts the camera in a format it does not list.
const UNLISTED: &str = "/data/unlisted";

#[wasdk::main]
async fn main() {
    let Some(cameras) = cameras::get_cameras() else {
        println!("cameras: none");
        return;
    };
    let listed = cameras.list().await;
    for camera in &listed {
        let facing = facing_word(camera.facing);
        println!("camera {}: {} ({facing})", camera.id, camera.name);
    }
    say_each_change(&cameras);
    let Some(first) = listed.first() else {
        return;
    };
    let camera = match cameras.open(first.id.clone()).await {
        Ok(camera) => camera,
        Err(error) => {
            println!("open {}: {}", first.id, described_error(&error));
            return;
        }
    };
    let formats = camera.formats();
    for format in &formats {
        println!("format {}", described_format(format));
    }
    let Some(format) = picked(&formats) else {
        return;
    };
    let (outputs, _layer, _reader) = outputs(&format);
    let session = match camera.start(format, &outputs).await {
        Ok(session) => session,
        Err(error) => {
            println!("start: {}", described_error(&error));
            return;
        }
    };
    println!("started in {}", described_size(&session.format().size));
    say_turns(&session);
    say_photo_and_states(&session).await;
}

/// Says the ids of the cameras listed each time a camera comes or goes, in a task of its own.
fn say_each_change(cameras: &Cameras) {
    let mut changes = cameras.changes();
    wasdk::spawn(async move {
        while let Some(listed) = changes.next().await {
            println!("cameras now: {}", ids(&listed));
        }
    });
}

fn ids(cameras: &[CameraInfo]) -> String {
    let ids: Vec<&str> = cameras.iter().map(|camera| camera.id.as_str()).collect();
    ids.join(", ")
}

/// The last of `formats`, or one the camera does not list where [`UNLISTED`] exists; none where
/// the camera lists none.
fn picked(formats: &[CameraFormat]) -> Option<CameraFormat> {
    let last = formats.last()?.clone();
    if !Path::new(UNLISTED).exists() {
        return Some(last);
    }
    Some(CameraFormat {
        size: Size {
            width: 1.0,
            height: 1.0,
        },
        ..last
    })
}

/// Outputs feeding a preview on an opaque layer and frames to a reader, each where the system has
/// them, and photos at `format`'s first photo size; with the layer and the reader, held while the
/// session runs.
fn outputs(
    format: &CameraFormat,
) -> (
    CameraOutputs,
    Option<views::CompositionLayer>,
    Option<video_frames::VideoFrameReader>,
) {
    let outputs = CameraOutputs::new();
    let layer = views::get_compositor().map(|compositor| compositor.make_layer(true));
    if let Some(layer) = &layer {
        outputs.set_preview(layer);
    }
    let reader = video_frames::get_video_frame_readers().map(|readers| readers.make_reader());
    if let Some(reader) = &reader {
        outputs.set_frames(reader);
    }
    if let Some(size) = format.photo_sizes.first() {
        outputs.set_photos(*size);
    }
    (outputs, layer, reader)
}

/// Says the turns that keep the session's picture level, as `turned: preview 0, capture 0`, and
/// again each time they change, while the session runs.
fn say_turns(session: &CaptureSession) {
    let mut changes = session.video_rotation_angle_changes();
    println!(
        "turned: {}",
        described_turns(&session.video_rotation_angles())
    );
    wasdk::spawn(async move {
        while let Some(turns) = changes.next().await {
            println!("turned: {}", described_turns(&turns));
        }
    });
}

fn described_turns(turns: &VideoRotationAngles) -> String {
    format!(
        "preview {}, capture {}",
        turns.for_horizon_level_preview, turns.for_horizon_level_capture
    )
}

/// Takes a photo and says it, then says the session's state and each after, until it ends.
async fn say_photo_and_states(session: &CaptureSession) {
    let mut changes = session.state_changes();
    match session.take_photo().await {
        Ok(photo) => println!("photo: {}, {} bytes", photo.mime_type, photo.data.len()),
        Err(error) => println!("photo: {}", described_error(&error)),
    }
    let mut state = session.state();
    loop {
        println!("state: {}", described_state(&state));
        if matches!(state, CaptureState::Ended(_)) {
            return;
        }
        match changes.next().await {
            Some(next) => state = next,
            None => return,
        }
    }
}

fn facing_word(facing: CameraFacing) -> &'static str {
    match facing {
        CameraFacing::Front => "front",
        CameraFacing::Back => "back",
        CameraFacing::External => "external",
    }
}

/// `format` in a few words: its size, its frame rates and its photo sizes.
fn described_format(format: &CameraFormat) -> String {
    let photos: Vec<String> = format.photo_sizes.iter().map(described_size).collect();
    format!(
        "{} at {}-{} fps, photos {}",
        described_size(&format.size),
        format.min_frame_rate,
        format.max_frame_rate,
        photos.join(" ")
    )
}

fn described_size(size: &Size) -> String {
    format!("{}x{}", size.width, size.height)
}

fn described_state(state: &CaptureState) -> String {
    match state {
        CaptureState::Running => "running".to_owned(),
        CaptureState::Interrupted(Some(why)) => {
            format!("interrupted ({})", interruption_words(*why))
        }
        CaptureState::Interrupted(None) => "interrupted".to_owned(),
        CaptureState::Ended(why) => format!("ended ({})", described_error(why)),
    }
}

fn interruption_words(interruption: CameraInterruption) -> &'static str {
    match interruption {
        CameraInterruption::InUseByAnotherApp => "in use by another app",
        CameraInterruption::Background => "background",
        CameraInterruption::SystemPressure => "system pressure",
    }
}

fn described_error(error: &CameraError) -> String {
    match error {
        CameraError::NotAllowed => "not allowed".to_owned(),
        CameraError::InUse => "in use".to_owned(),
        CameraError::Unplugged => "unplugged".to_owned(),
        CameraError::NotSupported(why) => format!("not supported ({why})"),
        CameraError::Failed(why) => format!("failed ({why})"),
    }
}
