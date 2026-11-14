//! Waft's players over `wasdk:media/media-players`, where the system has them: a player of a
//! URL is the WIT's player of the source it names, a picked file's `file:` URL as the file's path
//! inside the program and a link as it is; what the app asks of a player is the WIT's call of the
//! same name, a position in the nanoseconds of a WIT `duration`; its picture goes to the WIT's
//! layer behind a composition layer this program's compositor made (`composition`), or to the WIT's
//! reader behind a reader this program's readers made (`video_frames`), and nowhere for any other
//! output, as Waft says of an output this host did not make. Its state, with its tracks, and its
//! metadata are the WIT's; its cues are those the WIT's `cue-changes` said last, which a task of the
//! program's own keeps, as Waft reads them at once. A media asset's source, tracks, metadata and
//! failures are a player's, and go over so too (`media_assets`).
//!
//! The invariant: a `changed` future resolves at the first change of the state, the metadata or the
//! cues after it was asked for, or once the player is gone: the state's stream is opened as it is
//! asked for, before the app reads the state, and the tasks reading the cues and the metadata wake
//! every future waiting when either changes, and let them all go as their streams end with the
//! player.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use futures::channel::oneshot;
use futures::future;
use url::Url;
use waft_embedder::{
    Cue, EncodedImage, MediaChangeFuture, MediaMetadata, MediaPlayer, MediaTrack, MediaTrackKind,
    PauseReason, PlayerActivity, PlayerError, PlayerErrorKind, PlayerState, PlayerStatus, Players,
    SeekTolerance, Size, TimeRange, VideoOutput, WaitReason,
};
use wasdk::media_players as platform;
use wasdk::video_frames;

use crate::composition::wit_layer;
use crate::cues::framework_cue;
use crate::video_frames::wit_reader;

/// The system's players.
pub(crate) struct WasdkPlayers(platform::MediaPlayers);

impl WasdkPlayers {
    /// The system's players, where it has them.
    pub(crate) fn get() -> Option<WasdkPlayers> {
        platform::get_media_players().map(WasdkPlayers)
    }
}

impl Players for WasdkPlayers {
    fn open(&self, url: &str) -> Box<dyn MediaPlayer> {
        Box::new(WasdkPlayer::new(self.0.open(&source_of(url))))
    }
}

/// One of the system's players, with what it said last of its cues.
struct WasdkPlayer {
    player: platform::MediaPlayer,
    heard: Rc<Heard>,
}

/// What a player's streams of cues and metadata said, as their tasks keep it.
#[derive(Default)]
struct Heard {
    /// The cues to show now, as the WIT said last.
    cues: RefCell<Vec<Cue>>,
    /// The `changed` futures waiting for the next change of the cues or the metadata.
    waiting: RefCell<Vec<oneshot::Sender<()>>>,
}

impl WasdkPlayer {
    /// `player`, its cues and metadata followed from now on by tasks of the program's own.
    fn new(player: platform::MediaPlayer) -> WasdkPlayer {
        let heard = Rc::new(Heard::default());
        follow_cues(&player, &heard);
        follow_metadata(&player, &heard);
        WasdkPlayer { player, heard }
    }
}

impl Heard {
    /// The cues or the metadata changed: every `changed` waiting resolves.
    fn changed(&self) {
        for waiting in self.waiting.borrow_mut().drain(..) {
            // A future dropped meanwhile hears nothing.
            let _ = waiting.send(());
        }
    }
}

/// Keeps `player`'s cues in `heard` as its `cue-changes` says them, until the stream ends with the
/// player.
fn follow_cues(player: &platform::MediaPlayer, heard: &Rc<Heard>) {
    let mut cues = player.cue_changes();
    let heard = Rc::clone(heard);
    wasdk::spawn(async move {
        while let Some(shown) = cues.next().await {
            *heard.cues.borrow_mut() = shown.into_iter().map(framework_cue).collect();
            heard.changed();
        }
        heard.waiting.borrow_mut().clear();
    });
}

/// Tells `heard` of each change of `player`'s metadata, until the stream ends with the player.
fn follow_metadata(player: &platform::MediaPlayer, heard: &Rc<Heard>) {
    let mut metadata = player.metadata_changes();
    let heard = Rc::clone(heard);
    wasdk::spawn(async move {
        while metadata.next().await.is_some() {
            heard.changed();
        }
        heard.waiting.borrow_mut().clear();
    });
}

impl MediaPlayer for WasdkPlayer {
    fn play(&self) {
        self.player.play();
    }

    fn pause(&self) {
        self.player.pause();
    }

    fn seek(&self, position: Duration, tolerance: SeekTolerance) {
        let tolerance = platform::SeekTolerance {
            before: nanoseconds(tolerance.before),
            after: nanoseconds(tolerance.after),
        };
        self.player.seek(nanoseconds(position), tolerance);
    }

    fn set_rate(&self, rate: f64) {
        self.player.set_rate(rate);
    }

    fn set_volume(&self, volume: f64) {
        // From 0.0 to 1.0, which an f32 holds.
        self.player.set_volume(volume as f32);
    }

    fn set_muted(&self, muted: bool) {
        self.player.set_muted(muted);
    }

    fn set_looping(&self, looping: bool) {
        self.player.set_looping(looping);
    }

    fn state(&self) -> PlayerState {
        framework_state(self.player.state())
    }

    fn position(&self) -> Duration {
        Duration::from_nanos(self.player.position())
    }

    fn changed(&self) -> MediaChangeFuture {
        let mut changes = self.player.state_changes();
        let (heard, cues_or_metadata) = oneshot::channel();
        self.heard.waiting.borrow_mut().push(heard);
        Box::pin(async move {
            let state = Box::pin(async move { changes.next().await });
            // A change of either, or the player gone: the answers may differ either way.
            future::select(state, cues_or_metadata).await;
        })
    }

    fn set_video_output(&self, output: Option<VideoOutput>) {
        let output = match &output {
            Some(VideoOutput::Layer(layer)) => {
                wit_layer(layer).map(video_frames::VideoOutput::Layer)
            }
            Some(VideoOutput::Frames(reader)) => {
                wit_reader(reader).map(video_frames::VideoOutput::Frames)
            }
            None => None,
        };
        self.player.set_video_output(output.as_ref());
    }

    fn select_audio_track(&self, id: &str) {
        self.player.select_audio_track(id);
    }

    fn select_text_track(&self, id: Option<&str>) {
        self.player.select_text_track(id);
    }

    fn metadata(&self) -> MediaMetadata {
        framework_metadata(self.player.metadata())
    }

    fn cues(&self) -> Vec<Cue> {
        self.heard.cues.borrow().clone()
    }
}

/// What the WIT's `open` takes for `url`: a `file:` URL's path inside the program, and any other
/// URL as it is, which the platform judges.
pub(crate) fn source_of(url: &str) -> String {
    let path = Url::parse(url)
        .ok()
        .filter(|parsed| parsed.scheme() == "file")
        .and_then(|parsed| parsed.to_file_path().ok());
    match path {
        Some(path) => path.to_string_lossy().into_owned(),
        None => url.to_owned(),
    }
}

/// `duration` in nanoseconds, a WIT `duration`; the longest it holds for one past it, which a
/// seek's tolerance reads as no bound.
pub(crate) fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// The WIT's `state`, in Waft's terms.
fn framework_state(state: platform::PlayerState) -> PlayerState {
    PlayerState {
        status: match state.status {
            platform::PlayerStatus::Loading => PlayerStatus::Loading,
            platform::PlayerStatus::Ready => PlayerStatus::Ready,
            platform::PlayerStatus::Failed(error) => PlayerStatus::Failed(framework_error(error)),
        },
        activity: framework_activity(state.activity),
        seeking: state.seeking.map(Duration::from_nanos),
        rate: state.rate,
        volume: f64::from(state.volume),
        muted: state.muted,
        duration: state.duration.map(Duration::from_nanos),
        loaded: state.loaded.into_iter().map(framework_range).collect(),
        seekable: state.seekable.into_iter().map(framework_range).collect(),
        size: state.size.map(|size| Size::new(size.width, size.height)),
        tracks: state.tracks.into_iter().map(framework_track).collect(),
    }
}

fn framework_activity(activity: platform::PlayerActivity) -> PlayerActivity {
    match activity {
        platform::PlayerActivity::Paused(reason) => {
            PlayerActivity::Paused(reason.map(framework_pause_reason))
        }
        platform::PlayerActivity::Waiting(reason) => {
            PlayerActivity::Waiting(reason.map(framework_wait_reason))
        }
        platform::PlayerActivity::Playing => PlayerActivity::Playing,
    }
}

fn framework_pause_reason(reason: platform::PauseReason) -> PauseReason {
    match reason {
        platform::PauseReason::Requested => PauseReason::Requested,
        platform::PauseReason::End => PauseReason::End,
        platform::PauseReason::Interrupted => PauseReason::Interrupted,
        platform::PauseReason::OutputRemoved => PauseReason::OutputRemoved,
        platform::PauseReason::Backgrounded => PauseReason::Backgrounded,
    }
}

fn framework_wait_reason(reason: platform::WaitReason) -> WaitReason {
    match reason {
        platform::WaitReason::Data => WaitReason::Data,
        platform::WaitReason::EvaluatingData => WaitReason::EvaluatingData,
        platform::WaitReason::HeldBySystem => WaitReason::HeldBySystem,
    }
}

pub(crate) fn framework_error(error: platform::PlayerError) -> PlayerError {
    PlayerError {
        kind: match error.kind {
            platform::PlayerErrorKind::Network => PlayerErrorKind::Network,
            platform::PlayerErrorKind::Decode => PlayerErrorKind::Decode,
            platform::PlayerErrorKind::Unsupported => PlayerErrorKind::Unsupported,
            platform::PlayerErrorKind::Protected => PlayerErrorKind::Protected,
            platform::PlayerErrorKind::Refused => PlayerErrorKind::Refused,
            platform::PlayerErrorKind::Other => PlayerErrorKind::Other,
        },
        message: error.message,
    }
}

fn framework_range(range: platform::TimeRange) -> TimeRange {
    TimeRange {
        start: Duration::from_nanos(range.start),
        end: Duration::from_nanos(range.end),
    }
}

pub(crate) fn framework_track(track: platform::MediaTrack) -> MediaTrack {
    MediaTrack {
        id: track.id,
        kind: match track.kind {
            platform::MediaTrackKind::Audio => MediaTrackKind::Audio,
            platform::MediaTrackKind::Video => MediaTrackKind::Video,
            platform::MediaTrackKind::Text => MediaTrackKind::Text,
        },
        language: track.language,
        label: track.label,
        selected: track.selected,
    }
}

/// The WIT's `metadata`, in Waft's terms.
pub(crate) fn framework_metadata(metadata: platform::MediaMetadata) -> MediaMetadata {
    MediaMetadata {
        title: metadata.title,
        artist: metadata.artist,
        album: metadata.album,
        artwork: metadata.artwork.map(|artwork| EncodedImage {
            mime_type: artwork.mime_type,
            data: artwork.data.into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::PickedFile;
    use wasdk::windows;

    use super::*;

    #[test]
    fn a_picked_files_url_is_its_path_and_a_link_is_itself() {
        let path = std::env::temp_dir().join("a b").join("café.mov");
        let picked = PickedFile::Path(path.clone());
        assert_eq!(source_of(&picked.url()), path.to_string_lossy());
        let link = "https://devstreaming-cdn.apple.com/videos/master.m3u8";
        assert_eq!(source_of(link), link);
        assert_eq!(source_of("not a url"), "not a url");
    }

    #[test]
    fn a_position_is_in_nanoseconds_as_far_as_a_duration_holds() {
        assert_eq!(nanoseconds(Duration::from_millis(1500)), 1_500_000_000);
        assert_eq!(nanoseconds(Duration::MAX), u64::MAX);
    }

    #[test]
    fn the_wits_state_is_wafts_field_for_field() {
        let state = framework_state(platform::PlayerState {
            status: platform::PlayerStatus::Ready,
            activity: platform::PlayerActivity::Paused(Some(platform::PauseReason::Interrupted)),
            seeking: Some(1_500_000_000),
            rate: 1.5,
            volume: 0.5,
            muted: true,
            duration: Some(10_000_000_000),
            loaded: vec![platform::TimeRange {
                start: 0,
                end: 4_000_000_000,
            }],
            seekable: vec![platform::TimeRange {
                start: 0,
                end: 10_000_000_000,
            }],
            size: Some(windows::Size {
                width: 1920.0,
                height: 1080.0,
            }),
            tracks: vec![platform::MediaTrack {
                id: "text-en".to_owned(),
                kind: platform::MediaTrackKind::Text,
                language: "en".to_owned(),
                label: "English".to_owned(),
                selected: true,
            }],
        });
        assert_eq!(
            state,
            PlayerState {
                status: PlayerStatus::Ready,
                activity: PlayerActivity::Paused(Some(PauseReason::Interrupted)),
                seeking: Some(Duration::from_millis(1500)),
                rate: 1.5,
                volume: 0.5,
                muted: true,
                duration: Some(Duration::from_secs(10)),
                loaded: vec![TimeRange {
                    start: Duration::ZERO,
                    end: Duration::from_secs(4),
                }],
                seekable: vec![TimeRange {
                    start: Duration::ZERO,
                    end: Duration::from_secs(10),
                }],
                size: Some(Size::new(1920.0, 1080.0)),
                tracks: vec![MediaTrack {
                    id: "text-en".to_owned(),
                    kind: MediaTrackKind::Text,
                    language: "en".to_owned(),
                    label: "English".to_owned(),
                    selected: true,
                }],
            }
        );
        let refused = PlayerError {
            kind: PlayerErrorKind::Refused,
            message: "refused".to_owned(),
        };
        let failed = framework_state(platform::PlayerState {
            status: platform::PlayerStatus::Failed(platform::PlayerError {
                kind: platform::PlayerErrorKind::Refused,
                message: "refused".to_owned(),
            }),
            activity: platform::PlayerActivity::Paused(None),
            seeking: None,
            rate: 1.0,
            volume: 1.0,
            muted: false,
            duration: None,
            loaded: Vec::new(),
            seekable: Vec::new(),
            size: None,
            tracks: Vec::new(),
        });
        assert_eq!(failed, PlayerState::failed(refused));
    }

    #[test]
    fn the_wits_metadata_is_wafts_its_artwork_shared() {
        let metadata = framework_metadata(platform::MediaMetadata {
            title: Some("Bip Bop".to_owned()),
            artist: None,
            album: Some("Examples".to_owned()),
            artwork: Some(platform::EncodedImage {
                mime_type: "image/jpeg".to_owned(),
                data: vec![0xff, 0xd8],
            }),
        });
        assert_eq!(
            metadata,
            MediaMetadata {
                title: Some("Bip Bop".to_owned()),
                artist: None,
                album: Some("Examples".to_owned()),
                artwork: Some(EncodedImage {
                    mime_type: "image/jpeg".to_owned(),
                    data: vec![0xff, 0xd8].into(),
                }),
            }
        );
    }
}
