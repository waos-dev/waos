//! `examples/media-players` under the test shell, with the network's grants of each test's own: a
//! player opens on a file of the program's at the file's `file:` URL on the machine, its picture
//! goes to the layer the program made, the shell's own, and the state the shell reports reaches
//! the program, which moves it and plays it, and shows a text track its state lists, whose cues,
//! with the media's metadata, reach the program as the shell reports them; a player is stopped as
//! the program ends. A link opens once its host passes the
//! program's network rules, what the program asked meanwhile reaching the shell's player in order;
//! a link whose host the rules refuse, a path outside the program's files and a link of another
//! scheme give players that failed, and reach no shell. A shell without players gives the program
//! none.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use url::Url;
use waos::bindings::wasdk::media::media_players::{
    Cue, CueAnchor, CueContent, CueTextAlignment, CueTextRun, MediaMetadata, MediaTrack,
    MediaTrackKind, PauseReason, PlayerActivity, PlayerState, PlayerStatus,
};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, RunningProgram, WaOS};
use waos_capabilities::ProgramFolders;

use crate::reaching::{TestGrants, off};
use crate::shell::media_players::{Destination, OpenedPlayer, PlayerCall};
use crate::shell::{self, Lines, TestShell, wait_until};

/// Where the program plays each source from.
const TWO_SECONDS: Duration = Duration::from_secs(2);

#[test]
fn a_player_of_a_file_shows_on_the_programs_layer_and_its_state_reaches_the_program() {
    let (engine, shell, _arrivals) = shell::engine();
    let run = Run::start(&engine, &TestGrants::new(off(), false), "/data/clip.mp4\n");
    let player = opened(&shell, 0);
    let clip = fs::canonicalize(run.data()).unwrap().join("clip.mp4");
    assert_eq!(player.program, "media-players");
    assert_eq!(player.url, Url::from_file_path(&clip).unwrap().as_str());

    player.report(PlayerState {
        tracks: vec![
            track("audio-1", MediaTrackKind::Audio, true),
            track("text-en", MediaTrackKind::Text, false),
        ],
        ..ready(Some((640.0, 360.0)))
    });
    run.lines.wait_for("/data/clip.mp4: ready 640x360, playing");
    run.lines.wait_for("text track text-en on");
    assert_eq!(
        player.calls(),
        [
            PlayerCall::SetVideoOutput(Some(Destination::Layer(0))),
            PlayerCall::Seek {
                position: TWO_SECONDS,
                before: 0,
                after: 0,
            },
            PlayerCall::Play,
            PlayerCall::SelectTextTrack(Some("text-en".to_owned())),
        ],
        "the picture on the layer the program made, played from where it moved to, the text \
         track it picked from the state's tracks shown"
    );
    assert!(!shell.compositor().layers_made()[0].opaque);
    run.lines.wait_for("cues: none");
    player.report_cues(vec![cue("Hello"), cue("again")]);
    run.lines.wait_for("cues: Hello / again");
    player.report_metadata(MediaMetadata {
        title: Some("Bip Bop".to_owned()),
        artist: None,
        album: None,
        artwork: None,
    });
    run.lines.wait_for("title: Bip Bop");
    player.report(PlayerState {
        activity: PlayerActivity::Paused(Some(PauseReason::Interrupted)),
        ..ready(Some((640.0, 360.0)))
    });
    run.lines.wait_for("/data/clip.mp4: ready 640x360");

    run.running.stop();
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Stopped
    ));
    assert!(player.is_dropped(), "stopped as the program ends");
}

#[test]
fn a_link_opens_once_its_host_passes_the_rules_and_refused_sources_reach_no_shell() {
    let (engine, shell, _arrivals) = shell::engine();
    let sources = [
        "/elsewhere/clip.mp4",
        "http://192.0.2.1/a.m3u8",
        "ftp://example.com/a.mp4",
        "http://localhost/a.m3u8",
    ];
    let run = Run::start(&engine, &TestGrants::new(off(), true), &sources.join("\n"));
    run.lines.wait_for(
        "/elsewhere/clip.mp4: failed (no file at /elsewhere/clip.mp4 among the program's)",
    );
    run.lines
        .wait_for("http://192.0.2.1/a.m3u8: failed (the program may not reach 192.0.2.1)");
    run.lines.wait_for(
        "ftp://example.com/a.mp4: failed (\"ftp://example.com/a.mp4\" is neither a path inside the program nor an http or https link)",
    );
    let player = opened(&shell, 0);
    assert_eq!(player.url, "http://localhost/a.m3u8");
    player.report(ready(None));
    run.lines
        .wait_for("http://localhost/a.m3u8: ready, playing");
    assert_eq!(
        shell.media_players().opened().len(),
        1,
        "nothing refused reaches the shell"
    );
    assert_eq!(
        player.calls(),
        [
            PlayerCall::SetVideoOutput(Some(Destination::Layer(3))),
            PlayerCall::Seek {
                position: TWO_SECONDS,
                before: 0,
                after: 0,
            },
            PlayerCall::Play
        ],
        "what the program asked while the host was looked up, in order"
    );
    run.running.stop();
    shell::wait_for_end(&run.running);
}

#[test]
fn a_shell_without_players_gives_the_program_none() {
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.media_players = None;
    });
    let run = Run::start(&engine, &TestGrants::new(off(), false), "/data/clip.mp4\n");
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(run.lines.all(), ["media players: none"]);
}

/// The player opened after `before` others, once the shell has it.
fn opened(shell: &TestShell, before: usize) -> OpenedPlayer {
    wait_until("the shell opens a player", || {
        shell.media_players().opened().len() > before
    });
    shell.media_players().opened()[before].clone()
}

/// A track of `kind`, in English, named `id`.
fn track(id: &str, kind: MediaTrackKind, selected: bool) -> MediaTrack {
    MediaTrack {
        id: id.to_owned(),
        kind,
        language: "en".to_owned(),
        label: "English".to_owned(),
        selected,
    }
}

/// A cue of `text`, in no style of the media's, where Media3 places one by default.
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

/// A player's state once it can play, playing, with its picture's size where it has a picture.
fn ready(size: Option<(f64, f64)>) -> PlayerState {
    PlayerState {
        status: PlayerStatus::Ready,
        activity: PlayerActivity::Playing,
        seeking: None,
        rate: 1.0,
        volume: 1.0,
        muted: false,
        duration: Some(10_000_000_000),
        loaded: Vec::new(),
        seekable: Vec::new(),
        size: size.map(|(width, height)| Size { width, height }),
        tracks: Vec::new(),
    }
}

/// `examples/media-players` running with grants of the test's, the sources it plays written in
/// its `/data/sources`, beside a clip of its own, `/data/clip.mp4`.
struct Run {
    running: RunningProgram,
    lines: Arc<Lines>,
    folders: TempDir,
}

impl Run {
    fn start(engine: &WaOS, grants: &Arc<TestGrants>, sources: &str) -> Run {
        let folders = TempDir::new().unwrap();
        let own = ProgramFolders {
            data: folders.path().join("data"),
            cache: folders.path().join("cache"),
        };
        fs::create_dir_all(&own.data).unwrap();
        fs::create_dir_all(&own.cache).unwrap();
        fs::write(own.data.join("sources"), sources).unwrap();
        fs::write(own.data.join("clip.mp4"), "a clip").unwrap();
        let lines = Arc::new(Lines::default());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
            grants: Some(Arc::clone(grants) as Arc<dyn ProgramGrants>),
            folders: Some(own),
            ..ProgramEnvironment::default()
        };
        let program = shell::load(engine, waos_test_programs::MEDIA_PLAYERS);
        let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
        Run {
            running,
            lines,
            folders,
        }
    }

    /// The program's own `/data`, on the machine.
    fn data(&self) -> std::path::PathBuf {
        self.folders.path().join("data")
    }
}
