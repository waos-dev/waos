//! The system's players as the test shell keeps them: each player opened recorded with the program
//! that opened it, its URL and where it reports, and what it is asked, in order, its picture's
//! layer or reader by the test's number for it; a test reports a player's state, metadata and cues
//! as the system would.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use waos::bindings::wasdk::media::media_players::{Cue, MediaMetadata, PlayerState, SeekTolerance};
use waos::{MediaPlayerReports, RunningProgram, ShellMediaPlayer, ShellMediaPlayers, VideoOutput};

use super::composition::TestLayer;
use super::video_frames::number_of;

/// Players that keep each one opened.
#[derive(Default)]
pub struct TestMediaPlayers {
    opened: Mutex<Vec<OpenedPlayer>>,
}

/// One player a program opened, as the test sees it.
#[derive(Clone)]
pub struct OpenedPlayer {
    /// The name of the program that opened it.
    pub program: String,
    pub url: String,
    reports: MediaPlayerReports,
    calls: Arc<Mutex<Vec<PlayerCall>>>,
    dropped: Arc<AtomicBool>,
}

/// What a player was asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerCall {
    Play,
    Pause,
    /// A seek to `position`, landing within the tolerance's nanoseconds before and after it.
    Seek {
        position: Duration,
        before: u64,
        after: u64,
    },
    SetRate(f64),
    SetVolume(f32),
    SetMuted(bool),
    SetLooping(bool),
    /// Where the picture goes, or none.
    SetVideoOutput(Option<Destination>),
    SelectAudioTrack(String),
    SelectTextTrack(Option<String>),
}

/// Where a player's picture goes, as a test reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// The test compositor's layer of this number.
    Layer(usize),
    /// The test readers' reader of this number.
    Frames(usize),
}

impl TestMediaPlayers {
    /// The players opened so far, in order.
    pub fn opened(&self) -> Vec<OpenedPlayer> {
        self.opened.lock().unwrap().clone()
    }
}

impl OpenedPlayer {
    /// What the player was asked so far, in order.
    pub fn calls(&self) -> Vec<PlayerCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Whether the engine let go of the player.
    pub fn is_dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }

    /// Reports `state`, as the system would have changed it.
    pub fn report(&self, state: PlayerState) {
        self.reports.state_changed(state);
    }

    /// Reports that the player stood at `position` just now, as the system's own player tells it.
    pub fn report_position(&self, position: Duration) {
        self.reports.position_read(position);
    }

    /// Reports `metadata`, as the system would have read it from the media.
    pub fn report_metadata(&self, metadata: MediaMetadata) {
        self.reports.metadata_changed(metadata);
    }

    /// Reports `cues`, as the system would have reached them in the text track shown.
    pub fn report_cues(&self, cues: Vec<Cue>) {
        self.reports.cues_changed(cues);
    }
}

impl ShellMediaPlayers for TestMediaPlayers {
    fn open(
        &self,
        program: &RunningProgram,
        url: &str,
        reports: MediaPlayerReports,
    ) -> Box<dyn ShellMediaPlayer> {
        let opened = OpenedPlayer {
            program: program.name().to_owned(),
            url: url.to_owned(),
            reports,
            calls: Arc::default(),
            dropped: Arc::default(),
        };
        let player = TestPlayer {
            calls: Arc::clone(&opened.calls),
            dropped: Arc::clone(&opened.dropped),
        };
        self.opened.lock().unwrap().push(opened);
        Box::new(player)
    }
}

/// The player the engine holds.
struct TestPlayer {
    calls: Arc<Mutex<Vec<PlayerCall>>>,
    dropped: Arc<AtomicBool>,
}

impl TestPlayer {
    fn call(&self, call: PlayerCall) {
        self.calls.lock().unwrap().push(call);
    }
}

impl ShellMediaPlayer for TestPlayer {
    fn play(&self) {
        self.call(PlayerCall::Play);
    }

    fn pause(&self) {
        self.call(PlayerCall::Pause);
    }

    fn seek(&self, position: Duration, tolerance: SeekTolerance) {
        self.call(PlayerCall::Seek {
            position,
            before: tolerance.before,
            after: tolerance.after,
        });
    }

    fn set_rate(&self, rate: f64) {
        self.call(PlayerCall::SetRate(rate));
    }

    fn set_volume(&self, volume: f32) {
        self.call(PlayerCall::SetVolume(volume));
    }

    fn set_muted(&self, muted: bool) {
        self.call(PlayerCall::SetMuted(muted));
    }

    fn set_looping(&self, looping: bool) {
        self.call(PlayerCall::SetLooping(looping));
    }

    fn set_video_output(&self, output: Option<VideoOutput>) {
        self.call(PlayerCall::SetVideoOutput(output.map(destination_of)));
    }

    fn select_audio_track(&self, id: &str) {
        self.call(PlayerCall::SelectAudioTrack(id.to_owned()));
    }

    fn select_text_track(&self, id: Option<&str>) {
        self.call(PlayerCall::SelectTextTrack(id.map(str::to_owned)));
    }
}

/// Where `output` sends the picture, by the number of the test's layer or reader.
fn destination_of(output: VideoOutput) -> Destination {
    match output {
        VideoOutput::Layer(layer) => {
            let layer: Arc<dyn Any + Send + Sync> = layer;
            let layer = layer
                .downcast::<TestLayer>()
                .expect("a layer the test compositor made");
            Destination::Layer(layer.number)
        }
        VideoOutput::Frames(reader) => Destination::Frames(number_of(reader)),
    }
}

impl Drop for TestPlayer {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}
