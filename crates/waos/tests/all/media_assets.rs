//! `examples/media-assets` under the test shell, with the network's grants of each test's own: an
//! asset opens on a file of the program's at the file's `file:` URL on the machine, and what the
//! shell reads of it reaches the program, its length, its tracks, its title and a picture a tenth
//! of the way in, at the nearest keyframe and fitting the size asked, as a frame of the shell's
//! whose hardware buffer the program reads; the asset goes as the program ends. A link opens once
//! its host passes the program's network rules; a link whose host the rules refuse, a path outside
//! the program's files and a link of another scheme give assets that answer `refused`, and reach
//! no shell; a live stream has no length, and its picture is asked at its start. A shell without
//! a reading of media gives the program none. Its pictures are frames of
//! Apple's systems, where the engine takes them.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use url::Url;
use waos::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, MediaTrackKind, PlayerError, PlayerErrorKind,
};
use waos::{ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, RunningProgram, WaOS};
use waos_capabilities::ProgramFolders;

use crate::reaching::{TestGrants, off};
use crate::shell::media_assets::{AssetAnswers, FrameAsked};
use crate::shell::video_frames::{FRAME_HEIGHT, FRAME_WIDTH};
use crate::shell::{self, Lines, wait_until};

#[test]
fn an_asset_of_a_file_reads_its_length_tracks_title_and_a_picture() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.media_assets().answer_with(answers());
    let run = Run::start(&engine, &TestGrants::new(off(), false), "/data/clip.mp4\n");
    run.lines.wait_for("/data/clip.mp4: length 60s");
    run.lines
        .wait_for("/data/clip.mp4: tracks video*, audio en*, text en");
    run.lines.wait_for("/data/clip.mp4: title Bip Bop");
    run.lines.wait_for(&format!(
        "/data/clip.mp4: picture {FRAME_WIDTH}x{FRAME_HEIGHT}"
    ));
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Returned(Ok(()))
    ));

    let opened = shell.media_assets().opened();
    assert_eq!(opened.len(), 1);
    let clip = fs::canonicalize(run.data()).unwrap().join("clip.mp4");
    assert_eq!(opened[0].program, "media-assets");
    assert_eq!(opened[0].url, Url::from_file_path(&clip).unwrap().as_str());
    assert_eq!(
        opened[0].frames_asked(),
        [FrameAsked {
            time: Duration::from_secs(6),
            before: u64::MAX,
            after: u64::MAX,
            max_size: Some((160.0, 160.0)),
        }],
        "a tenth of the way in, at the nearest keyframe, fitting the size asked"
    );
    wait_until("the asset goes as the program ends", || {
        opened[0].is_dropped()
    });
}

#[test]
fn a_link_opens_once_its_host_passes_the_rules_and_refused_sources_reach_no_shell() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.media_assets().answer_with(AssetAnswers {
        duration: Ok(None),
        picture: Err(PlayerError {
            kind: PlayerErrorKind::Decode,
            message: "a damaged frame".to_owned(),
        }),
        ..answers()
    });
    let sources = [
        "/elsewhere/clip.mp4",
        "http://192.0.2.1/a.mp4",
        "ftp://example.com/a.mp4",
        "http://localhost/a.mp4",
    ];
    let run = Run::start(&engine, &TestGrants::new(off(), true), &sources.join("\n"));
    run.lines.wait_for(
        "/elsewhere/clip.mp4: length failed (refused: no file at /elsewhere/clip.mp4 among the program's)",
    );
    run.lines.wait_for(
        "/elsewhere/clip.mp4: picture failed (refused: no file at /elsewhere/clip.mp4 among the program's)",
    );
    run.lines.wait_for(
        "http://192.0.2.1/a.mp4: title failed (refused: the program may not reach 192.0.2.1)",
    );
    run.lines.wait_for(
        "ftp://example.com/a.mp4: tracks failed (refused: \"ftp://example.com/a.mp4\" is neither a path inside the program nor an http or https link)",
    );
    run.lines.wait_for("http://localhost/a.mp4: length none");
    run.lines
        .wait_for("http://localhost/a.mp4: picture failed (decode: a damaged frame)");
    shell::wait_for_end(&run.running);
    let opened = shell.media_assets().opened();
    assert_eq!(opened.len(), 1, "nothing refused reaches the shell");
    assert_eq!(opened[0].url, "http://localhost/a.mp4");
    assert_eq!(
        opened[0].frames_asked()[0].time,
        Duration::ZERO,
        "a live stream's picture at its start"
    );
}

#[test]
fn a_shell_without_a_reading_of_media_gives_the_program_none() {
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.media_assets = None;
    });
    let run = Run::start(&engine, &TestGrants::new(off(), false), "/data/clip.mp4\n");
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(run.lines.all(), ["media assets: none"]);
}

/// What the shell's assets answer: a minute, a video track, an English audio track and an English
/// text track, the media titled Bip Bop, and a red picture.
fn answers() -> AssetAnswers {
    AssetAnswers {
        duration: Ok(Some(Duration::from_secs(60))),
        tracks: Ok(vec![
            track("track:1", MediaTrackKind::Video, "", true),
            track("audible:0", MediaTrackKind::Audio, "en", true),
            track("legible:0", MediaTrackKind::Text, "en", false),
        ]),
        metadata: Ok(MediaMetadata {
            title: Some("Bip Bop".to_owned()),
            artist: None,
            album: None,
            artwork: None,
        }),
        picture: Ok([0, 0, 255, 255]),
    }
}

fn track(id: &str, kind: MediaTrackKind, language: &str, selected: bool) -> MediaTrack {
    MediaTrack {
        id: id.to_owned(),
        kind,
        language: language.to_owned(),
        label: String::new(),
        selected,
    }
}

/// `examples/media-assets` running with grants of the test's, the sources it reads written in its
/// `/data/sources`, beside a clip of its own, `/data/clip.mp4`.
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
            folders: Some(own),
            grants: Some(Arc::clone(grants) as Arc<dyn ProgramGrants>),
            ..ProgramEnvironment::default()
        };
        let program = shell::load(engine, waos_test_programs::MEDIA_ASSETS);
        let running = engine.run(&program, environment).unwrap();
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
