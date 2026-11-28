//! A program's players: each the shell's from the moment its source passed while the program
//! holds its handle, stopped when the program drops it or ends, as its store's table drops it.
//! What the program asks of a player before the shell's is open, while its link's host is looked
//! up, waits for the shell's, and is asked of it in order once it opens. The shell reports the
//! player's state into its [`MediaPlayerReports`], as it reports a tray icon's events into the
//! icon's `TrayIconReports`, its metadata, its cues and where it was too; the engine keeps the
//! newest of each, which `state` and `metadata` read and every stream of their changes watches.
//!
//! The program reads its player's position on its own thread, where the shell's player is not, so
//! the engine answers it from the position the shell read last, moved on by the time since at the
//! rate the state says it plays at while it plays, as WebKit's page answers `currentTime` for a
//! player in its GPU process.
//!
//! The invariant: a player whose source was refused has no shell's player, ever, and its state is
//! `failed` for good; once the program lets a player go, its shell's player is dropped, nothing
//! more reaches it, and every stream of its state ends.

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use futures::stream::BoxStream;
use tokio::sync::watch;

use crate::bindings::wasdk::media::media_players::{
    Cue, MediaMetadata, PauseReason, PlayerActivity, PlayerError, PlayerErrorKind, PlayerState,
    PlayerStatus, SeekTolerance,
};
use crate::newest::changes_of;
use crate::shell::{ShellMediaPlayer, VideoOutput};
use crate::sync::locked;

/// `media-players.media-player`: one of the program's players.
pub struct MediaPlayer(Arc<Player>);

/// One player, shared by its handle and, weakly, by its reports.
struct Player {
    shell: Mutex<ShellPlayer>,
    /// The state as the shell reported it last: `loading` until it reports.
    state: watch::Sender<PlayerState>,
    /// What the media says about itself, as the shell reported it last: empty until it reports.
    metadata: watch::Sender<MediaMetadata>,
    /// The cues to show now, as the shell reported them last: none until it reports.
    cues: watch::Sender<Vec<Cue>>,
    /// Where the shell's player was when the shell last read it.
    position_read: Mutex<PositionRead>,
}

/// Where a player was, and when.
#[derive(Clone, Copy)]
struct PositionRead {
    position: Duration,
    at: Instant,
}

/// The shell's side of a player, as far as it got.
enum ShellPlayer {
    /// Not open yet: what the program asked meanwhile, in order.
    Waiting(Vec<Request>),
    Open(Box<dyn ShellMediaPlayer>),
    /// Its source was refused: none, and what the program asks goes nowhere.
    Refused,
}

/// Something the program asked of its player, for the shell's.
type Request = Box<dyn FnOnce(&dyn ShellMediaPlayer) + Send>;

impl MediaPlayer {
    /// A player loading, whose shell's player is not open yet.
    pub(super) fn loading() -> MediaPlayer {
        MediaPlayer::in_state(ShellPlayer::Waiting(Vec::new()), loading())
    }

    /// A player whose source was refused for `reason`: failed, with no shell's player.
    pub(super) fn refused(reason: String) -> MediaPlayer {
        tracing::info!("a player's source is refused: {reason}");
        MediaPlayer::in_state(ShellPlayer::Refused, refused_state(reason))
    }

    fn in_state(shell: ShellPlayer, state: PlayerState) -> MediaPlayer {
        MediaPlayer(Arc::new(Player {
            shell: Mutex::new(shell),
            state: watch::Sender::new(state),
            metadata: watch::Sender::new(no_metadata()),
            cues: watch::Sender::new(Vec::new()),
            position_read: Mutex::new(PositionRead {
                position: Duration::ZERO,
                at: Instant::now(),
            }),
        }))
    }

    /// Where what happens to this player is reported.
    pub(super) fn reports(&self) -> MediaPlayerReports {
        MediaPlayerReports(Arc::downgrade(&self.0))
    }

    pub(super) fn play(&self) {
        self.0.request(|shell| shell.play());
    }

    pub(super) fn pause(&self) {
        self.0.request(|shell| shell.pause());
    }

    pub(super) fn seek(&self, position: Duration, tolerance: SeekTolerance) {
        self.0.request(move |shell| shell.seek(position, tolerance));
    }

    pub(super) fn set_rate(&self, rate: f64) {
        self.0.request(move |shell| shell.set_rate(rate));
    }

    pub(super) fn set_volume(&self, volume: f32) {
        self.0.request(move |shell| shell.set_volume(volume));
    }

    pub(super) fn set_muted(&self, muted: bool) {
        self.0.request(move |shell| shell.set_muted(muted));
    }

    pub(super) fn set_looping(&self, looping: bool) {
        self.0.request(move |shell| shell.set_looping(looping));
    }

    /// Sends the picture to `output` from now on; none sends it nowhere.
    pub(super) fn set_video_output(&self, output: Option<VideoOutput>) {
        self.0.request(move |shell| shell.set_video_output(output));
    }

    /// Plays the audio track `id` in place of the one playing.
    pub(super) fn select_audio_track(&self, id: String) {
        self.0.request(move |shell| shell.select_audio_track(&id));
    }

    /// Shows the text track `id`, or none.
    pub(super) fn select_text_track(&self, id: Option<String>) {
        self.0
            .request(move |shell| shell.select_text_track(id.as_deref()));
    }

    /// What the media says about itself, as the shell reported it last.
    pub(super) fn metadata(&self) -> MediaMetadata {
        self.0.metadata.borrow().clone()
    }

    /// The metadata the shell reports from now on, on a stream of its own: one read late hears
    /// the newest; it ends once the player is let go.
    pub(super) fn metadata_changes(&self) -> BoxStream<'static, MediaMetadata> {
        changes_of(self.0.metadata.subscribe())
    }

    /// The cues to show now, then again each time the shell reports others, on a stream of its
    /// own: one read late hears the newest; it ends once the player is let go.
    pub(super) fn cue_changes(&self) -> BoxStream<'static, Vec<Cue>> {
        let mut changes = self.0.cues.subscribe();
        changes.mark_changed();
        changes_of(changes)
    }

    /// The state as the shell reported it last.
    pub(super) fn state(&self) -> PlayerState {
        self.0.state.borrow().clone()
    }

    /// Where the player is now: the target while a seek is under way, and otherwise where the
    /// shell read it last, moved on by the time since at the rate it plays at while it plays,
    /// never before the start nor past the media's end.
    pub(super) fn position(&self) -> Duration {
        let state = self.0.state.borrow();
        if let Some(target) = state.seeking {
            return Duration::from_nanos(target);
        }
        let read = *locked(&self.0.position_read);
        let rate = match state.activity {
            PlayerActivity::Playing => state.rate,
            PlayerActivity::Paused(_) | PlayerActivity::Waiting(_) => 0.0,
        };
        let seconds = read.position.as_secs_f64() + read.at.elapsed().as_secs_f64() * rate;
        let position = Duration::try_from_secs_f64(seconds.max(0.0)).unwrap_or(read.position);
        match state.duration {
            Some(duration) => position.min(Duration::from_nanos(duration)),
            None => position,
        }
    }

    /// Each state the shell reports from now on, on a stream of its own: one read late hears the
    /// newest, passing over those between; it ends once the player is let go.
    pub(super) fn state_changes(&self) -> BoxStream<'static, PlayerState> {
        changes_of(self.0.state.subscribe())
    }
}

impl Player {
    /// Asks `request` of the shell's player: now, once it is open, and never once refused.
    fn request(&self, request: impl FnOnce(&dyn ShellMediaPlayer) + Send + 'static) {
        match &mut *locked(&self.shell) {
            ShellPlayer::Waiting(waiting) => waiting.push(Box::new(request)),
            ShellPlayer::Open(shell) => request(shell.as_ref()),
            ShellPlayer::Refused => {}
        }
    }

    /// The shell's player is `opened`: what the program asked meanwhile is asked of it, in order,
    /// before anything it asks next.
    fn opened(&self, opened: Box<dyn ShellMediaPlayer>) {
        let mut shell = locked(&self.shell);
        if let ShellPlayer::Waiting(waiting) = &mut *shell {
            for request in waiting.drain(..) {
                request(opened.as_ref());
            }
        }
        *shell = ShellPlayer::Open(opened);
    }

    /// Its source was refused for `reason`: it failed, and what the program asked goes nowhere.
    fn refused(&self, reason: String) {
        tracing::info!("a player's source is refused: {reason}");
        *locked(&self.shell) = ShellPlayer::Refused;
        self.state.send_replace(refused_state(reason));
    }
}

/// Where what happens to one player is reported, from any thread: its state, by the shell, and
/// whether its source passed, by the engine. Reports on a player the program has let go of are
/// dropped.
#[derive(Clone)]
pub struct MediaPlayerReports(Weak<Player>);

impl MediaPlayerReports {
    /// The player's state is `state` from now on: what `state` answers, and what every stream of
    /// its changes hears next.
    pub fn state_changed(&self, state: PlayerState) {
        if let Some(player) = self.0.upgrade() {
            player.state.send_replace(state);
        }
    }

    /// What the media says about itself is `metadata` from now on: what `metadata` answers, and
    /// what every stream of its changes hears next.
    pub fn metadata_changed(&self, metadata: MediaMetadata) {
        if let Some(player) = self.0.upgrade() {
            player.metadata.send_replace(metadata);
        }
    }

    /// The cues to show are `cues` from now on, as the media styles them: what every stream of
    /// the player's cues hears next.
    pub fn cues_changed(&self, cues: Vec<Cue>) {
        if let Some(player) = self.0.upgrade() {
            player.cues.send_replace(cues);
        }
    }

    /// The shell's player is at `position` now, as the shell read it at a change of the player,
    /// before the state it reports with it: where the program's position moves on from. Every
    /// stream of the state hears it, as the change may have been a jump of the position, a seek
    /// landing or a loop, which leaves the state as it was.
    pub fn position_read(&self, position: Duration) {
        if let Some(player) = self.0.upgrade() {
            *locked(&player.position_read) = PositionRead {
                position,
                at: Instant::now(),
            };
            player.state.send_modify(|_| {});
        }
    }

    /// Opens the shell's player with `open`, handed where it reports, unless the program has let
    /// the player go; what the program asked meanwhile is asked of it.
    pub(super) fn open_with(
        &self,
        open: impl FnOnce(MediaPlayerReports) -> Box<dyn ShellMediaPlayer>,
    ) {
        if let Some(player) = self.0.upgrade() {
            player.opened(open(self.clone()));
        }
    }

    /// The player's source was refused for `reason`.
    pub(super) fn refused(&self, reason: String) {
        if let Some(player) = self.0.upgrade() {
            player.refused(reason);
        }
    }
}

/// A player's state before the shell reports any: nothing known yet, and not played.
fn loading() -> PlayerState {
    PlayerState {
        status: PlayerStatus::Loading,
        activity: PlayerActivity::Paused(Some(PauseReason::Requested)),
        seeking: None,
        rate: 1.0,
        volume: 1.0,
        muted: false,
        duration: None,
        loaded: Vec::new(),
        seekable: Vec::new(),
        size: None,
        tracks: Vec::new(),
    }
}

/// What a player's media says about itself before the shell reports anything: nothing.
fn no_metadata() -> MediaMetadata {
    MediaMetadata {
        title: None,
        artist: None,
        album: None,
        artwork: None,
    }
}

/// The state of a player whose source was refused for `reason`: failed, doing nothing more.
fn refused_state(reason: String) -> PlayerState {
    PlayerState {
        status: PlayerStatus::Failed(PlayerError {
            kind: PlayerErrorKind::Refused,
            message: reason,
        }),
        activity: PlayerActivity::Paused(None),
        ..loading()
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use futures::executor::block_on;

    use super::*;
    use crate::bindings::wasdk::media::media_players::{
        CueAnchor, CueContent, CueTextAlignment, CueTextRun,
    };

    /// A shell's player that keeps what it is asked, by name, in order.
    #[derive(Clone, Default)]
    struct Asked(Arc<Mutex<Vec<&'static str>>>);

    impl Asked {
        fn all(&self) -> Vec<&'static str> {
            self.0.lock().unwrap().clone()
        }

        fn ask(&self, what: &'static str) {
            self.0.lock().unwrap().push(what);
        }
    }

    impl ShellMediaPlayer for Asked {
        fn play(&self) {
            self.ask("play");
        }

        fn pause(&self) {
            self.ask("pause");
        }

        fn seek(&self, _position: Duration, _tolerance: SeekTolerance) {
            self.ask("seek");
        }

        fn set_rate(&self, _rate: f64) {
            self.ask("rate");
        }

        fn set_volume(&self, _volume: f32) {
            self.ask("volume");
        }

        fn set_muted(&self, _muted: bool) {
            self.ask("muted");
        }

        fn set_looping(&self, _looping: bool) {
            self.ask("looping");
        }

        fn set_video_output(&self, output: Option<VideoOutput>) {
            self.ask(if output.is_some() {
                "output"
            } else {
                "no output"
            });
        }

        fn select_audio_track(&self, _id: &str) {
            self.ask("audio track");
        }

        fn select_text_track(&self, id: Option<&str>) {
            self.ask(if id.is_some() {
                "text track"
            } else {
                "no text track"
            });
        }
    }

    const EXACT: SeekTolerance = SeekTolerance {
        before: 0,
        after: 0,
    };

    fn ready() -> PlayerState {
        PlayerState {
            status: PlayerStatus::Ready,
            ..loading()
        }
    }

    /// A ready player's state, doing `activity` at `rate`, a minute long.
    fn doing(activity: PlayerActivity, rate: f64) -> PlayerState {
        PlayerState {
            activity,
            rate,
            duration: Some(Duration::from_secs(60).as_nanos() as u64),
            ..ready()
        }
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "the position moves with real time since the shell's last read; exact would take \
                  the player reading a clock the test moves"
    )]
    fn the_position_moves_on_from_the_shells_last_read_only_while_playing() {
        let player = MediaPlayer::loading();
        let reports = player.reports();
        reports.state_changed(doing(PlayerActivity::Paused(None), 1.0));
        reports.position_read(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            player.position(),
            Duration::from_secs(10),
            "paused: it stands"
        );

        reports.state_changed(doing(PlayerActivity::Playing, 2.0));
        reports.position_read(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(50));
        let moved = player.position();
        assert!(
            moved >= Duration::from_millis(10_100) && moved < Duration::from_secs(11),
            "twice as fast as time: {moved:?}"
        );

        reports.position_read(Duration::from_secs(59) + Duration::from_millis(990));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            player.position(),
            Duration::from_secs(60),
            "never past the end"
        );
    }

    #[test]
    fn the_position_is_the_target_while_a_seek_is_under_way() {
        let player = MediaPlayer::loading();
        let reports = player.reports();
        reports.position_read(Duration::from_secs(10));
        reports.state_changed(PlayerState {
            seeking: Some(Duration::from_secs(30).as_nanos() as u64),
            ..doing(PlayerActivity::Playing, 1.0)
        });
        assert_eq!(player.position(), Duration::from_secs(30));
    }

    #[test]
    fn what_is_asked_before_the_shells_player_opens_reaches_it_in_order() {
        let player = MediaPlayer::loading();
        player.set_looping(true);
        player.seek(Duration::from_secs(2), EXACT);
        player.select_text_track(Some("en".to_owned()));
        player.play();
        let asked = Asked::default();
        let shell = asked.clone();
        player.reports().open_with(|_reports| Box::new(shell));
        player.pause();
        player.select_audio_track("audio-2".to_owned());
        assert_eq!(
            asked.all(),
            [
                "looping",
                "seek",
                "text track",
                "play",
                "pause",
                "audio track"
            ]
        );
    }

    #[test]
    fn a_refused_player_failed_and_asks_nothing_of_any_shell() {
        let player = MediaPlayer::loading();
        player.play();
        player
            .reports()
            .refused("the program may not reach example.com".to_owned());
        player.pause();
        assert!(matches!(
            player.state().status,
            PlayerStatus::Failed(PlayerError { kind: PlayerErrorKind::Refused, message })
                if message == "the program may not reach example.com"
        ));
        let refused = MediaPlayer::refused("no file at /x among the program's".to_owned());
        assert!(matches!(refused.state().status, PlayerStatus::Failed(_)));
    }

    #[test]
    fn the_shells_reports_reach_the_state_and_every_stream_of_its_changes() {
        let player = MediaPlayer::loading();
        let mut changes = player.state_changes();
        let reports = player.reports();
        assert!(matches!(player.state().status, PlayerStatus::Loading));
        reports.state_changed(ready());
        assert!(matches!(player.state().status, PlayerStatus::Ready));
        let heard = block_on(changes.next()).unwrap();
        assert!(matches!(heard.status, PlayerStatus::Ready));
    }

    #[test]
    fn a_stream_read_late_hears_the_newest_state() {
        let player = MediaPlayer::loading();
        let mut changes = player.state_changes();
        player.reports().state_changed(ready());
        player
            .reports()
            .state_changed(refused_state("gone".to_owned()));
        let heard = block_on(changes.next()).unwrap();
        assert!(matches!(heard.status, PlayerStatus::Failed(_)));
    }

    #[test]
    fn the_cues_stream_hears_the_cues_now_then_each_change_and_metadata_its_changes() {
        let player = MediaPlayer::loading();
        let mut cues = player.cue_changes();
        let mut metadata = player.metadata_changes();
        assert_eq!(
            block_on(cues.next()).map(|cues| cues.len()),
            Some(0),
            "none now"
        );
        let reports = player.reports();
        reports.cues_changed(vec![cue("Hello")]);
        let heard = block_on(cues.next()).unwrap();
        assert!(matches!(&heard[0].content, CueContent::Text(runs) if runs[0].text == "Hello"));
        reports.metadata_changed(MediaMetadata {
            title: Some("Bip Bop".to_owned()),
            ..no_metadata()
        });
        assert_eq!(
            block_on(metadata.next()).unwrap().title.as_deref(),
            Some("Bip Bop")
        );
        assert_eq!(player.metadata().title.as_deref(), Some("Bip Bop"));
    }

    /// A cue of `text`, as Media3 places one by default.
    fn cue(text: &str) -> Cue {
        Cue {
            content: CueContent::Text(vec![CueTextRun {
                text: text.to_owned(),
                bold: false,
                italic: false,
                underline: false,
                color: None,
                background: None,
                font_size: None,
                font_family: None,
                edge: None,
                edge_color: None,
            }]),
            line: None,
            line_anchor: CueAnchor::Start,
            position: None,
            position_anchor: CueAnchor::Start,
            size: None,
            text_alignment: CueTextAlignment::Center,
            vertical: None,
            window_color: None,
        }
    }

    #[test]
    fn a_player_let_go_drops_its_shells_player_and_ends_its_streams() {
        let player = MediaPlayer::loading();
        let mut changes = player.state_changes();
        let reports = player.reports();
        let asked = Asked::default();
        let shell = asked.clone();
        reports.open_with(|_reports| Box::new(shell));
        drop(player);
        assert_eq!(
            Arc::strong_count(&asked.0),
            1,
            "the shell's player is dropped"
        );
        assert!(block_on(changes.next()).is_none());
        let mut opened_again = false;
        reports.open_with(|_reports| {
            opened_again = true;
            Box::new(Asked::default())
        });
        assert!(!opened_again, "nothing opens for a player let go of");
    }
}
