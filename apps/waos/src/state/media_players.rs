//! The programs' players, over Waft's: the engine opens one from a program's thread, or from the
//! thread its link's host was looked up on, and calls it from the program's; each call is posted
//! to the App's thread (`app_thread_bound`), where the player is opened on Waft's players, its
//! picture sent to the layer the program made, which is Waft's, or to the reader it made, Waft's
//! from the first time a player is given it, controlled as the program asks, its tracks picked
//! among them, and dropped as the program lets it go; its state, its media's metadata and its cues
//! reported to its program as it opens and at each change after, as Waft's own `Player` follows the
//! host's. The players are the engine's shell's only where Waft's platform has them.
//!
//! The invariant: a player's calls reach the App's thread in the order made, its opening first and
//! its removal last; an opening that cannot reach it, as WaOS quits, is reported failed at once, so
//! no program waits for a player that cannot load; each change of a player's state is reported
//! while Waft's player is kept, since the next change is asked for before the state is read; and a
//! player let go of is reported no more.

use std::any::Any;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use waft::ui::{self, MediaPlayer, Platform};
use waft::{App, AsyncApp};
use waos::bindings::wasdk::media::media_players::SeekTolerance;
use waos::{MediaPlayerReports, RunningProgram, ShellMediaPlayer, ShellMediaPlayers, VideoOutput};
use waos_program_window::waft_layer;

use super::app_thread_bound::{AppThreadBound, AppThreadCalls};
use crate::logic::cues::program_cue;
use crate::logic::frame_readers::ReaderOnRequest;
use crate::logic::player_states::{program_metadata, program_state, waft_tolerance};

/// The id the app gives each player a program opens, which its calls carry.
type PlayerKey = u64;

/// Waft's players, served to the engine as its shell's.
pub(crate) struct MediaPlayers {
    shell: Arc<PlayersOnRequest>,
    calls: AppThreadCalls<OpenPlayers>,
}

impl MediaPlayers {
    /// Waft's players on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<MediaPlayers> {
        platform.players()?;
        let (players, calls) = AppThreadBound::new();
        let shell = PlayersOnRequest {
            players,
            last_player: AtomicU64::default(),
        };
        Some(MediaPlayers {
            shell: Arc::new(shell),
            calls,
        })
    }

    /// The players as the engine's shell has them: each call is posted to the App's thread, where
    /// [`serve`](Self::serve) runs it.
    pub(crate) fn shell(&self) -> Arc<dyn ShellMediaPlayers> {
        Arc::clone(&self.shell) as Arc<dyn ShellMediaPlayers>
    }

    /// From now on, each player a program opens is one of Waft's, and does as the program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let open = OpenPlayers {
            app: app.to_async(),
            open: HashMap::new(),
        };
        self.calls.serve(app, open);
    }
}

/// The app's players, as the engine reaches them: every call is posted to the App's thread.
struct PlayersOnRequest {
    players: AppThreadBound<OpenPlayers>,
    last_player: AtomicU64,
}

impl ShellMediaPlayers for PlayersOnRequest {
    fn open(
        &self,
        program: &RunningProgram,
        url: &str,
        reports: MediaPlayerReports,
    ) -> Box<dyn ShellMediaPlayer> {
        tracing::info!(program = program.name(), url, "a program opens a player");
        let key = self.last_player.fetch_add(1, Ordering::Relaxed) + 1;
        let opening = self.players.post({
            let url = url.to_owned();
            let reports = reports.clone();
            move |players| players.open(key, &url, reports)
        });
        if opening.is_err() {
            reports.state_changed(failed("WaOS is quitting"));
        }
        Box::new(PlayerOnRequest {
            key,
            players: self.players.clone(),
        })
    }
}

/// One player, as the engine holds it: its calls are posted to the App's thread, and its removal
/// as it drops.
struct PlayerOnRequest {
    key: PlayerKey,
    players: AppThreadBound<OpenPlayers>,
}

impl PlayerOnRequest {
    /// Does `call` to Waft's player on the App's thread, if it opened; nothing once WaOS quits.
    fn on_player(&self, call: impl FnOnce(&dyn MediaPlayer) + Send + 'static) {
        let key = self.key;
        let _ = self.players.post(move |players| {
            if let Some(waft) = players.open.get(&key) {
                call(waft.as_ref());
            }
        });
    }
}

impl ShellMediaPlayer for PlayerOnRequest {
    fn play(&self) {
        self.on_player(|waft| waft.play());
    }

    fn pause(&self) {
        self.on_player(|waft| waft.pause());
    }

    fn seek(&self, position: Duration, tolerance: SeekTolerance) {
        let tolerance = waft_tolerance(tolerance);
        self.on_player(move |waft| waft.seek(position, tolerance));
    }

    fn set_rate(&self, rate: f64) {
        self.on_player(move |waft| waft.set_rate(rate));
    }

    fn set_volume(&self, volume: f32) {
        self.on_player(move |waft| waft.set_volume(f64::from(volume)));
    }

    fn set_muted(&self, muted: bool) {
        self.on_player(move |waft| waft.set_muted(muted));
    }

    fn set_looping(&self, looping: bool) {
        self.on_player(move |waft| waft.set_looping(looping));
    }

    fn set_video_output(&self, output: Option<VideoOutput>) {
        let key = self.key;
        let _ = self
            .players
            .post(move |players| players.set_video_output(key, output));
    }

    fn select_audio_track(&self, id: &str) {
        let id = id.to_owned();
        self.on_player(move |waft| waft.select_audio_track(&id));
    }

    fn select_text_track(&self, id: Option<&str>) {
        let id = id.map(str::to_owned);
        self.on_player(move |waft| waft.select_text_track(id.as_deref()));
    }
}

impl Drop for PlayerOnRequest {
    fn drop(&mut self) {
        let key = self.key;
        let _ = self.players.post(move |players| {
            players.open.remove(&key);
        });
    }
}

/// Waft's players the programs opened, by the app's key for each, on the App's thread.
struct OpenPlayers {
    app: AsyncApp,
    open: HashMap<PlayerKey, Rc<dyn MediaPlayer>>,
}

impl OpenPlayers {
    /// Opens Waft's player of `url` as `key`, its state reported into `reports` from now on;
    /// reported failed where Waft's platform has no players.
    fn open(&mut self, key: PlayerKey, url: &str, reports: MediaPlayerReports) {
        let opened = self.app.update(|app| open_player(app, url, reports));
        if let Some(waft) = opened {
            self.open.insert(key, waft);
        }
    }

    /// Sends the picture of the player `key` to `output`, or nowhere for none.
    fn set_video_output(&self, key: PlayerKey, output: Option<VideoOutput>) {
        let Some(waft) = self.open.get(&key) else {
            return;
        };
        let platform = self.app.update(|app| app.platform());
        waft.set_video_output(output.and_then(|output| waft_output(platform.as_ref(), output)));
    }
}

/// Waft's player of `url`, its state reported into `reports` from now on; none, reported failed,
/// where Waft's platform has no players.
fn open_player(
    app: &mut App,
    url: &str,
    reports: MediaPlayerReports,
) -> Option<Rc<dyn MediaPlayer>> {
    let platform = app.platform();
    let Some(players) = platform.players() else {
        reports.state_changed(failed("the system has no players"));
        return None;
    };
    let waft: Rc<dyn MediaPlayer> = Rc::from(players.open(url));
    report_changes(app, &waft, reports);
    Some(waft)
}

/// A player's state that failed for `message`, as the program reads it.
fn failed(message: &str) -> waos::bindings::wasdk::media::media_players::PlayerState {
    program_state(ui::PlayerState::failed(ui::PlayerError {
        kind: ui::PlayerErrorKind::Other,
        message: message.to_owned(),
    }))
}

/// Reports `waft`'s state, its media's metadata, its cues and its position into `reports` now,
/// and again at each of its changes, until the player goes: the position at every change, a jump
/// of it among them, and before the state, so that the engine never moves a position read long
/// ago on at a rate the player has only now taken.
fn report_changes(app: &mut App, waft: &Rc<dyn MediaPlayer>, reports: MediaPlayerReports) {
    let changed = waft.changed();
    let mut reported = Reported::of(waft.as_ref());
    reported.report_all(&reports);
    let waft: Weak<dyn MediaPlayer> = Rc::downgrade(waft);
    app.spawn(async move |_cx| {
        let mut changed = changed;
        loop {
            changed.await;
            let Some(player) = waft.upgrade() else {
                return;
            };
            changed = player.changed();
            reported.report_changes(Reported::of(player.as_ref()), &reports);
        }
    });
}

/// What was reported last of a player, in Waft's terms, to tell what changed, and where it was.
struct Reported {
    position: Duration,
    state: ui::PlayerState,
    metadata: ui::MediaMetadata,
    cues: Vec<ui::Cue>,
}

impl Reported {
    /// `waft`'s state, metadata and cues now.
    fn of(waft: &dyn MediaPlayer) -> Reported {
        Reported {
            position: waft.position(),
            state: waft.state(),
            metadata: waft.metadata(),
            cues: waft.cues(),
        }
    }

    /// Reports everything, as a player opens.
    fn report_all(&self, reports: &MediaPlayerReports) {
        reports.position_read(self.position);
        reports.state_changed(program_state(self.state.clone()));
        reports.metadata_changed(program_metadata(self.metadata.clone()));
        reports.cues_changed(self.cues.iter().cloned().map(program_cue).collect());
    }

    /// Reports what of `now` differs from what was reported, and keeps `now`.
    fn report_changes(&mut self, now: Reported, reports: &MediaPlayerReports) {
        reports.position_read(now.position);
        if now.state != self.state {
            reports.state_changed(program_state(now.state.clone()));
        }
        if now.metadata != self.metadata {
            reports.metadata_changed(program_metadata(now.metadata.clone()));
        }
        if now.cues != self.cues {
            reports.cues_changed(now.cues.iter().cloned().map(program_cue).collect());
        }
        *self = now;
    }
}

/// `output` as Waft's player is given it: Waft's layer behind the program's, or the program's
/// reader as Waft's, made on `platform`'s readers; none where the platform has no readers or the
/// reader is WaOS's simulated camera's, and none, said so, for a layer Waft's compositor did not
/// make or a reader the app's readers did not, which would show nothing.
fn waft_output(platform: &dyn Platform, output: VideoOutput) -> Option<ui::VideoOutput> {
    match output {
        VideoOutput::Layer(layer) => match waft_layer(layer) {
            Some(layer) => Some(ui::VideoOutput::Layer(layer)),
            None => not_made_by_the_app(),
        },
        VideoOutput::Frames(reader) => {
            let reader: Arc<dyn Any + Send + Sync> = reader;
            match reader.downcast::<ReaderOnRequest>() {
                Ok(reader) => {
                    let readers = platform.video_frame_readers()?;
                    reader.waft_reader(readers).map(ui::VideoOutput::Frames)
                }
                Err(_) => not_made_by_the_app(),
            }
        }
    }
}

/// No output, said so, for one the app did not make.
fn not_made_by_the_app() -> Option<ui::VideoOutput> {
    tracing::warn!("a player was given an output the app did not make; it shows nowhere");
    None
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use waft_test::{
        MockCompositor, MockPlayers, MockVideoFrameReaders, PlayerCall, TestPlatform,
        VideoOutputKind,
    };
    use waos::ProgramEnd;
    use waos_package::ProgramId;

    use super::*;
    use crate::state::test_system::{Lines, TestSystem, real_path};

    const MEDIA_PLAYERS: &str = "dev.example.mediaplayers";
    const VIDEO_FRAMES: &str = "dev.example.videoframes";

    #[test]
    fn a_programs_player_is_wafts_on_the_programs_layer_and_its_state_reaches_the_program() {
        let players = MockPlayers::new();
        let compositor = MockCompositor::new();
        let platform = TestPlatform::new()
            .with_players(Rc::new(players.clone()))
            .with_compositor(Arc::new(compositor.clone()));
        let system = TestSystem::new(platform);
        let id: ProgramId = MEDIA_PLAYERS.parse().unwrap();
        let data = system.store.folders(&id).unwrap().data;
        std::fs::write(data.join("sources"), "/data/clip.mp4\n").unwrap();
        std::fs::write(data.join("clip.mp4"), "a clip").unwrap();
        let lines = Arc::new(Lines::default());
        let running = system.run_as(waos_test_programs::MEDIA_PLAYERS, MEDIA_PLAYERS, &lines);

        system.run_tasks_until("the program plays its clip on its layer", || {
            players
                .opened()
                .first()
                .is_some_and(|opened| opened.calls().len() == 3)
        });
        let opened = &players.opened()[0];
        let clip = real_path(&data).join("clip.mp4");
        assert_eq!(
            waft::ui::file_uri_path(&opened.url()).as_deref(),
            Some(clip.as_path()),
            "where the clip is on the machine"
        );
        assert_eq!(
            opened.calls(),
            [
                PlayerCall::SetVideoOutput(Some(VideoOutputKind::Layer)),
                PlayerCall::Seek(Duration::from_secs(2), ui::SeekTolerance::EXACT),
                PlayerCall::Play
            ]
        );
        let layer = opened.video_layer().expect("on the program's layer");
        assert!(std::ptr::addr_eq(
            Arc::as_ptr(&layer),
            Arc::as_ptr(&compositor.made()[0])
        ));

        opened.set_state(ui::PlayerState {
            status: ui::PlayerStatus::Ready,
            activity: ui::PlayerActivity::Playing,
            duration: Some(Duration::from_secs(10)),
            size: Some(ui::Size::new(640.0, 360.0)),
            tracks: vec![ui::MediaTrack {
                id: "text-en".to_owned(),
                kind: ui::MediaTrackKind::Text,
                language: "en".to_owned(),
                label: "English".to_owned(),
                selected: false,
            }],
            ..ui::PlayerState::loading()
        });
        system.run_tasks_until("the program hears its player is ready", || {
            lines.has("/data/clip.mp4: ready 640x360, playing")
        });
        system.run_tasks_until("the program shows the text track", || {
            opened.calls().last() == Some(&PlayerCall::SelectTextTrack(Some("text-en".to_owned())))
        });
        system.run_tasks_until("the program hears no cues yet", || lines.has("cues: none"));
        opened.set_cues(vec![ui::Cue::new(ui::CueContent::Text(vec![
            ui::CueTextRun::new("Hello"),
        ]))]);
        system.run_tasks_until("the program hears the cue", || lines.has("cues: Hello"));
        opened.set_metadata(ui::MediaMetadata {
            title: Some("Bip Bop".to_owned()),
            ..ui::MediaMetadata::default()
        });
        system.run_tasks_until("the program hears the title", || {
            lines.has("title: Bip Bop")
        });

        running.stop();
        let end = futures::executor::block_on(running.ended());
        assert!(matches!(end, ProgramEnd::Stopped), "{end:?}");
        system.run_tasks_until("Waft's player is let go", || opened.is_dropped());
    }

    /// `examples/video-frames` gives a player the reader it made, which is Waft's once the player
    /// is given it, made on Waft's readers.
    #[test]
    fn a_programs_reader_is_wafts_once_a_player_is_given_it() {
        let players = MockPlayers::new();
        let readers = MockVideoFrameReaders::new();
        let platform = TestPlatform::new()
            .with_players(Rc::new(players.clone()))
            .with_video_frame_readers(Rc::new(readers.clone()));
        let system = TestSystem::new(platform);
        let id: ProgramId = VIDEO_FRAMES.parse().unwrap();
        let data = system.store.folders(&id).unwrap().data;
        std::fs::write(data.join("clip.mp4"), "a clip").unwrap();
        let lines = Arc::new(Lines::default());
        let running = system.run_as(waos_test_programs::VIDEO_FRAMES, VIDEO_FRAMES, &lines);

        system.run_tasks_until("the player plays into the program's reader", || {
            players
                .opened()
                .first()
                .is_some_and(|opened| opened.calls().len() == 2)
        });
        let opened = &players.opened()[0];
        assert_eq!(
            opened.calls(),
            [
                PlayerCall::SetVideoOutput(Some(VideoOutputKind::Frames)),
                PlayerCall::Play
            ]
        );
        let made = readers.made();
        assert_eq!(made.len(), 1, "Waft's reader, made once");
        let reader = opened
            .frame_reader()
            .expect("the player's frames go to a reader");
        assert!(Arc::ptr_eq(&reader, &made[0]));
        running.stop();
        futures::executor::block_on(running.ended());
    }

    #[test]
    fn a_platform_without_players_gives_the_engine_none() {
        assert!(MediaPlayers::of(&TestPlatform::new()).is_none());
        assert!(
            MediaPlayers::of(&TestPlatform::new().with_players(Rc::new(MockPlayers::new())))
                .is_some()
        );
    }
}
