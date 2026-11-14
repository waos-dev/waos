//! Where the programs' players go: each opening and each call to the main thread, where Waft's
//! players are controlled (`state::media_players`). The engine opens a player from a program's
//! thread, or from the thread its link's host was looked up on, and calls it from the program's;
//! what Waft's player does comes back through the reports the engine handed with the opening.
//!
//! The invariant: a player's calls reach the main thread in the order made, its opening first and
//! its removal last; and an opening that cannot reach it, as WaOS quits, is reported failed at
//! once, so no program waits for a player that cannot load.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waft::ui::{self, CompositionLayer};
use waos::bindings::wasdk::media::media_players::SeekTolerance;
use waos::{MediaPlayerReports, RunningProgram, ShellMediaPlayer, ShellMediaPlayers, VideoOutput};
use waos_program_window::waft_layer;

use super::frame_readers::ReaderOnRequest;
use super::player_states::program_state;

/// The id the app gives each player a program opens, which its calls carry.
pub(crate) type PlayerKey = u64;

/// One call of the programs' players, on its way to the main thread.
pub(crate) enum PlayerCall {
    /// A program opens a player of `url`, whose state goes to `reports`.
    Open {
        player: PlayerKey,
        url: String,
        reports: MediaPlayerReports,
    },
    /// Something asked of one of the players.
    Player(PlayerKey, PlayerRequest),
    /// The program let one of its players go.
    Remove(PlayerKey),
}

/// What a program asks of one of its players.
pub(crate) enum PlayerRequest {
    Play,
    Pause,
    /// A seek to the position, landing within the tolerance.
    Seek(Duration, ui::SeekTolerance),
    SetRate(f64),
    SetVolume(f64),
    SetMuted(bool),
    SetLooping(bool),
    /// Where the picture goes; none for nowhere.
    SetVideoOutput(Option<VideoDestination>),
    SelectAudioTrack(String),
    SelectTextTrack(Option<String>),
}

/// Where a player's picture goes, as the main thread gives it to Waft's player.
pub(crate) enum VideoDestination {
    /// Waft's layer behind the program's.
    Layer(Arc<dyn CompositionLayer>),
    /// The program's reader, Waft's once a player is given it.
    Frames(Arc<ReaderOnRequest>),
}

/// The app's players, as the engine reaches them: every call goes to the main thread.
pub(crate) struct PlayersOnRequest {
    calls: UnboundedSender<PlayerCall>,
    last_player: AtomicU64,
}

impl PlayersOnRequest {
    /// The players, and where their calls arrive on the main thread.
    pub(crate) fn new() -> (PlayersOnRequest, UnboundedReceiver<PlayerCall>) {
        let (calls, arriving) = mpsc::unbounded();
        let players = PlayersOnRequest {
            calls,
            last_player: AtomicU64::default(),
        };
        (players, arriving)
    }
}

impl ShellMediaPlayers for PlayersOnRequest {
    fn open(
        &self,
        program: &RunningProgram,
        url: &str,
        reports: MediaPlayerReports,
    ) -> Box<dyn ShellMediaPlayer> {
        tracing::info!(program = program.name(), url, "a program opens a player");
        let player = self.last_player.fetch_add(1, Ordering::Relaxed) + 1;
        let opening = PlayerCall::Open {
            player,
            url: url.to_owned(),
            reports: reports.clone(),
        };
        if self.calls.unbounded_send(opening).is_err() {
            tracing::info!("WaOS is quitting: the player is not opened");
            reports.state_changed(program_state(ui::PlayerState::failed(ui::PlayerError {
                kind: ui::PlayerErrorKind::Other,
                message: "WaOS is quitting".to_owned(),
            })));
        }
        Box::new(PlayerOnRequest {
            player,
            calls: self.calls.clone(),
        })
    }
}

/// One player, as the engine holds it: its calls go to the main thread, and its removal as it
/// drops.
struct PlayerOnRequest {
    player: PlayerKey,
    calls: UnboundedSender<PlayerCall>,
}

impl PlayerOnRequest {
    fn send(&self, request: PlayerRequest) {
        self.send_call(PlayerCall::Player(self.player, request));
    }

    fn send_call(&self, call: PlayerCall) {
        if self.calls.unbounded_send(call).is_err() {
            tracing::info!("WaOS is quitting: a player's call goes nowhere");
        }
    }
}

impl ShellMediaPlayer for PlayerOnRequest {
    fn play(&self) {
        self.send(PlayerRequest::Play);
    }

    fn pause(&self) {
        self.send(PlayerRequest::Pause);
    }

    fn seek(&self, position: Duration, tolerance: SeekTolerance) {
        self.send(PlayerRequest::Seek(position, waft_tolerance(tolerance)));
    }

    fn set_rate(&self, rate: f64) {
        self.send(PlayerRequest::SetRate(rate));
    }

    fn set_volume(&self, volume: f32) {
        self.send(PlayerRequest::SetVolume(f64::from(volume)));
    }

    fn set_muted(&self, muted: bool) {
        self.send(PlayerRequest::SetMuted(muted));
    }

    fn set_looping(&self, looping: bool) {
        self.send(PlayerRequest::SetLooping(looping));
    }

    fn set_video_output(&self, output: Option<VideoOutput>) {
        self.send(PlayerRequest::SetVideoOutput(
            output.and_then(destination_of),
        ));
    }

    fn select_audio_track(&self, id: &str) {
        self.send(PlayerRequest::SelectAudioTrack(id.to_owned()));
    }

    fn select_text_track(&self, id: Option<&str>) {
        self.send(PlayerRequest::SelectTextTrack(id.map(str::to_owned)));
    }
}

impl Drop for PlayerOnRequest {
    fn drop(&mut self) {
        self.send_call(PlayerCall::Remove(self.player));
    }
}

/// Where `output` sends the picture, as the main thread gives it to Waft's player: Waft's layer
/// behind the program's, or the program's reader; none, said so, for a layer Waft's compositor did
/// not make or a reader the app's readers did not, which would show nothing.
fn destination_of(output: VideoOutput) -> Option<VideoDestination> {
    let destination = match output {
        VideoOutput::Layer(layer) => waft_layer(layer).map(VideoDestination::Layer),
        VideoOutput::Frames(reader) => {
            let reader: Arc<dyn Any + Send + Sync> = reader;
            reader.downcast().ok().map(VideoDestination::Frames)
        }
    };
    if destination.is_none() {
        tracing::warn!("a player was given an output the app did not make; it shows nowhere");
    }
    destination
}

/// A program's seek tolerance as Waft's player and media assets take it: the longest WIT duration
/// on a side is no bound there.
pub(crate) fn waft_tolerance(tolerance: SeekTolerance) -> ui::SeekTolerance {
    let side = |nanoseconds: u64| match nanoseconds {
        u64::MAX => Duration::MAX,
        nanoseconds => Duration::from_nanos(nanoseconds),
    };
    ui::SeekTolerance {
        before: side(tolerance.before),
        after: side(tolerance.after),
    }
}
