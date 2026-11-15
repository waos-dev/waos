//! A program that reads media without playing it, through the system's reading of media: for each
//! source listed in `/data/sources`, one a line, in order, it says the media's length, as
//! `source: length 60s`, or `length none` for a live stream; its tracks, each by its kind, its
//! language where it has one, and a `*` where a player would play it, as
//! `source: tracks video*, audio en*`; its title, as `source: title Bip Bop` or `title none`; and
//! the size of its picture a tenth of the way in, or at its start where it has no length, at the
//! nearest keyframe and no larger than 160 by 160, as `source: picture 64x48`; or why one is not,
//! as `source: length failed (refused: why)`. Then it ends. Where the system has no reading of
//! media it says `media assets: none`.
//!
//! ```text
//! cargo build -p media-assets --target wasm32-wasip3
//! ```

use std::fs;

use wasdk::media_assets::{self, MediaAsset};
use wasdk::media_players::{
    MediaTrack, MediaTrackKind, PlayerError, PlayerErrorKind, SeekTolerance,
};
use wasdk::windows::Size;

/// What lists the sources it reads.
const SOURCES: &str = "/data/sources";

/// The largest picture it asks for.
const PICTURE_SIZE: Size = Size {
    width: 160.0,
    height: 160.0,
};

/// Wherever the system lands soonest, as the nearest keyframe.
const NEAREST_KEYFRAME: SeekTolerance = SeekTolerance {
    before: u64::MAX,
    after: u64::MAX,
};

#[wasdk::main]
async fn main() {
    let Some(assets) = media_assets::get_media_assets() else {
        println!("media assets: none");
        return;
    };
    let sources = fs::read_to_string(SOURCES).unwrap_or_default();
    for source in sources.lines() {
        say_what_it_reads(source, &assets.open(source)).await;
    }
}

/// Says the length, tracks, title and picture size `asset` reads, each as `source: ...`.
async fn say_what_it_reads(source: &str, asset: &MediaAsset) {
    let length = asset.duration().await;
    let say = |what: &str, answer: String| println!("{source}: {what} {answer}");
    say(
        "length",
        described(&length, |length| match length {
            Some(length) => format!("{}s", length / 1_000_000_000),
            None => "none".to_owned(),
        }),
    );
    say(
        "tracks",
        described(&asset.tracks().await, |tracks| described_tracks(tracks)),
    );
    let metadata = asset.metadata().await;
    say(
        "title",
        described(&metadata, |metadata| {
            metadata.title.clone().unwrap_or("none".to_owned())
        }),
    );
    let tenth = length.ok().flatten().map_or(0, |length| length / 10);
    let picture = asset
        .frame_at(tenth, NEAREST_KEYFRAME, Some(PICTURE_SIZE))
        .await;
    say(
        "picture",
        described(&picture, |frame| {
            let buffer = frame.hardware_buffer();
            format!("{}x{}", buffer.width(), buffer.height())
        }),
    );
}

/// `answer` in a few words: as `say` words it, or why it failed.
fn described<T>(answer: &Result<T, PlayerError>, say: impl Fn(&T) -> String) -> String {
    match answer {
        Ok(value) => say(value),
        Err(error) => format!("failed ({}: {})", kind_name(error.kind), error.message),
    }
}

/// A failure's kind as the WIT names it.
fn kind_name(kind: PlayerErrorKind) -> &'static str {
    match kind {
        PlayerErrorKind::Network => "network",
        PlayerErrorKind::Decode => "decode",
        PlayerErrorKind::Unsupported => "unsupported",
        PlayerErrorKind::Protected => "protected",
        PlayerErrorKind::Refused => "refused",
        PlayerErrorKind::Other => "other",
    }
}

/// `tracks` by their kinds, their languages and whether a player would play them.
fn described_tracks(tracks: &[MediaTrack]) -> String {
    let described: Vec<String> = tracks
        .iter()
        .map(|track| {
            let kind = match track.kind {
                MediaTrackKind::Audio => "audio",
                MediaTrackKind::Video => "video",
                MediaTrackKind::Text => "text",
            };
            let language = if track.language.is_empty() {
                String::new()
            } else {
                format!(" {}", track.language)
            };
            let selected = if track.selected { "*" } else { "" };
            format!("{kind}{language}{selected}")
        })
        .collect();
    described.join(", ")
}
