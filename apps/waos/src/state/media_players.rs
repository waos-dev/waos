//! The programs' players on the main thread, over Waft's: each opened on Waft's players as the
//! engine asks, its picture sent to the layer the program made, which is Waft's, or to the reader
//! it made, Waft's from the first time a player is given it, controlled as the program asks, its
//! tracks picked among them, and dropped as the program lets it go; its state, its media's
//! metadata and its cues reported to its program as it opens and at each change after, as Waft's
//! own `Player` follows the host's. The players are the engine's shell's only where Waft's
//! platform has them.
//!
//! The invariant: each change of a player's state is reported while Waft's player is kept, since
//! the next change is asked for before the state is read; and a player let go of is reported no
//! more.

use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::App;
use waft::ui::{self, MediaPlayer, Platform, VideoOutput};
use waos::{MediaPlayerReports, ShellMediaPlayers};

use crate::logic::cues::program_cue;
use crate::logic::media_player_calls::{
    PlayerCall, PlayerKey, PlayerRequest, PlayersOnRequest, VideoDestination,
};
use crate::logic::player_states::{program_metadata, program_state};

/// Waft's players, served to the engine as its shell's.
pub(crate) struct MediaPlayers {
    shell: Arc<PlayersOnRequest>,
    arriving: UnboundedReceiver<PlayerCall>,
}

/// Waft's players the programs opened, by the app's key for each.
type OpenPlayers = HashMap<PlayerKey, Rc<dyn MediaPlayer>>;

impl MediaPlayers {
    /// Waft's players on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<MediaPlayers> {
        platform.players()?;
        let (shell, arriving) = PlayersOnRequest::new();
        Some(MediaPlayers {
            shell: Arc::new(shell),
            arriving,
        })
    }

    /// The players as the engine's shell has them: each call goes to the main thread, where
    /// [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellMediaPlayers> {
        Arc::clone(&self.shell) as Arc<dyn ShellMediaPlayers>
    }

    /// From now on, each player a program opens is one of Waft's, and does as the program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            let mut open = OpenPlayers::new();
            while let Some(call) = arriving.next().await {
                cx.update(|app| take(app, &mut open, call));
            }
        });
    }
}

/// Carries out `call`, from the engine.
fn take(app: &mut App, open: &mut OpenPlayers, call: PlayerCall) {
    match call {
        PlayerCall::Open {
            player,
            url,
            reports,
        } => {
            if let Some(waft) = open_player(app, &url, reports) {
                open.insert(player, waft);
            }
        }
        PlayerCall::Player(player, request) => {
            if let Some(waft) = open.get(&player) {
                carry_out(app.platform().as_ref(), waft.as_ref(), request);
            }
        }
        PlayerCall::Remove(player) => {
            open.remove(&player);
        }
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
        reports.state_changed(program_state(ui::PlayerState::failed(ui::PlayerError {
            kind: ui::PlayerErrorKind::Other,
            message: "the system has no players".to_owned(),
        })));
        return None;
    };
    let waft: Rc<dyn MediaPlayer> = Rc::from(players.open(url));
    report_changes(app, &waft, reports);
    Some(waft)
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

/// Asks `request` of Waft's player, a frame reader made on `platform`'s readers.
fn carry_out(platform: &dyn Platform, waft: &dyn MediaPlayer, request: PlayerRequest) {
    match request {
        PlayerRequest::Play => waft.play(),
        PlayerRequest::Pause => waft.pause(),
        PlayerRequest::Seek(position, tolerance) => waft.seek(position, tolerance),
        PlayerRequest::SetRate(rate) => waft.set_rate(rate),
        PlayerRequest::SetVolume(volume) => waft.set_volume(volume),
        PlayerRequest::SetMuted(muted) => waft.set_muted(muted),
        PlayerRequest::SetLooping(looping) => waft.set_looping(looping),
        PlayerRequest::SetVideoOutput(destination) => {
            let output = destination.and_then(|destination| waft_output(platform, destination));
            waft.set_video_output(output);
        }
        PlayerRequest::SelectAudioTrack(id) => waft.select_audio_track(&id),
        PlayerRequest::SelectTextTrack(id) => waft.select_text_track(id.as_deref()),
    }
}

/// `destination` as Waft's player is given it: its layer, or the program's reader as Waft's,
/// made on `platform`'s readers; none where the platform has none, or the reader is WaOS's
/// simulated camera's.
fn waft_output(platform: &dyn Platform, destination: VideoDestination) -> Option<VideoOutput> {
    match destination {
        VideoDestination::Layer(layer) => Some(VideoOutput::Layer(layer)),
        VideoDestination::Frames(reader) => {
            let readers = platform.video_frame_readers()?;
            Some(VideoOutput::Frames(reader.waft_reader(readers)?))
        }
    }
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
        assert!(Arc::ptr_eq(&layer, &compositor.made()[0].layer));

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
