//! `builtin-apps/video-player`, a Waft app over `wasdk-adapter-waft`, under the test shell, given
//! this computer's fonts and folders of each test's own: it asks for its window, titled for the
//! app, keeps it no smaller than 480 by 270, sets its menu bar, draws, and quits as its window
//! closes. A video picked through File's Open… plays on a layer the app made, the window titled
//! for it, and its keys and menus reach the player: Space plays, → skips ten seconds within what
//! it can seek, Faster and Mute ask the player, and a subtitle track the state lists is shown. A
//! video in the recent list it keeps in `/data` opens again where it was left once the player can
//! seek there, and is kept where it pauses and where it stands as the app ends. A folder added
//! through File's Add Folder… lists its video in the library, its thumbnail a tenth of the way in
//! drawn, read with its length from the system's reading of media, and a bar along it for how far
//! it was watched; a click on it plays it from where it was left, and Esc goes back to the
//! library. A thumbnail is kept in `/cache`, encoded by the system's hardware codecs from the
//! picture's own hardware buffer, and the next run draws it decoded by them into a hardware buffer,
//! the video not read at all. It is a debug build of Waft, slow to start, and draws, so it runs
//! where the engine makes hardware buffers, on Apple's systems.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use url::Url;
use waos::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, MediaTrackKind, PauseReason, PlayerActivity, PlayerError,
    PlayerErrorKind, PlayerState, PlayerStatus, TimeRange,
};
use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::bindings::wasdk::platform::menus::{MenuEntry, MenuEntryKind, Submenu};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{
    Keyboard, ProgramEnd, ProgramEnvironment, ProgramOutput, ProgramView, RunningProgram, WaOS,
};
use waos_capabilities::ProgramFolders;
use waos_headless::HeadlessView;

use crate::test_shell::image_codecs::{Asked, ImageAnswers, ImageFacts, OpenedFrom};
use crate::test_shell::media_assets::{AssetAnswers, FrameAsked};
use crate::test_shell::media_players::{Destination, OpenedPlayer, PlayerCall};
use crate::test_shell::menus::TestMenus;
use crate::test_shell::{RecordedOutput, TestShell, config, pixel_of, wait_until, within};

/// How long the media the tests play is.
const MINUTE: Duration = Duration::from_secs(60);

/// The colour of the thumbnails the shell's reading of media answers: red, as BGRA.
const THUMBNAIL_RED: [u8; 4] = [0, 0, 255, 255];

#[test]
fn it_shows_its_window_under_its_menu_bar_and_quits_as_the_window_closes() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start(&engine, &shell, "");
    let window = run.shown.window().expect("the shell placed a window");
    assert_eq!(window.config().title, "Video Player");
    assert!(
        within(run.window.picture_after_events()),
        "the video player draws its window"
    );
    wait_until("its window keeps a smallest size", || {
        window.size_limits().0.is_some()
    });
    assert_eq!(
        window.size_limits().0.map(|size| (size.width, size.height)),
        Some((480.0, 270.0))
    );

    let bar = wait_for_bar(&shell.menus, |bar| has(bar, "File"));
    let titles: Vec<_> = bar
        .iter()
        .filter(|entry| entry.parent.is_none())
        .map(label_of)
        .collect();
    assert_eq!(
        titles,
        ["Video Player", "File", "Edit", "Playback", "View", "Window"]
    );
    run.close();
}

#[test]
fn a_video_picked_through_open_plays_and_its_keys_and_menus_reach_the_player() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let elsewhere = TempDir::new().unwrap();
    let video = fs::canonicalize(elsewhere.path()).unwrap().join("clip.mp4");
    fs::write(&video, "a clip").unwrap();
    shell.file_dialogs.answer_next(vec![video.clone()]);
    let run = Run::start(&engine, &shell, "");
    let bar = wait_for_bar(&shell.menus, |bar| has(bar, "File"));

    run.running.menu_chosen(id_of(&bar, "Open…"));
    let player = opened(&shell, 0);
    let asked = shell.file_dialogs.asked();
    assert_eq!(asked[0].kind, "open-file");
    assert_eq!(
        asked[0].parent,
        Some(run.window.id()),
        "a sheet on its window"
    );
    assert_eq!(player.url, Url::from_file_path(&video).unwrap().as_str());
    wait_until("the video goes to a layer the app made, and plays", || {
        let calls = player.calls();
        calls.contains(&PlayerCall::Play)
            && calls.iter().any(|call| {
                matches!(
                    call,
                    PlayerCall::SetVideoOutput(Some(Destination::Layer(_)))
                )
            })
    });
    let window = run.shown.window().expect("in a window");
    wait_until("the window is titled for the video", || {
        window.title() == "clip.mp4"
    });

    player.report_position(Duration::from_secs(3));
    player.report(ready(PlayerActivity::Paused(Some(PauseReason::Requested))));
    let bar = wait_for_bar(&shell.menus, |bar| has(bar, "English"));

    Keyboard::press(&run.window, "space").unwrap();
    wait_for_call(&player, &PlayerCall::Play, 2);
    Keyboard::press(&run.window, "right").unwrap();
    wait_for_call(
        &player,
        &PlayerCall::Seek {
            position: Duration::from_secs(13),
            before: 0,
            after: 0,
        },
        1,
    );

    run.running.menu_chosen(id_of(&bar, "Faster"));
    wait_for_call(&player, &PlayerCall::SetRate(1.25), 1);
    run.running.menu_chosen(id_of(&bar, "Mute"));
    wait_for_call(&player, &PlayerCall::SetMuted(true), 1);
    run.running.menu_chosen(id_of(&bar, "English"));
    wait_for_call(
        &player,
        &PlayerCall::SelectTextTrack(Some("text-en".to_owned())),
        1,
    );

    run.close();
    assert!(player.is_dropped(), "the player goes as the app ends");
    let kept = kept_list(&run);
    assert!(
        kept.starts_with("3000\tfile:///") && kept.ends_with("/clip.mp4\n"),
        "the video is kept in the recent list where it stood: {kept:?}"
    );
}

#[test]
fn a_recent_video_opens_where_it_was_left_and_is_kept_where_it_stands_as_the_app_ends() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start(&engine, &shell, "4000\tfile:///data/clip.mp4\n");
    fs::write(run.data().join("clip.mp4"), "a clip").unwrap();
    let bar = wait_for_bar(&shell.menus, |bar| has(bar, "clip.mp4"));

    run.running.menu_chosen(id_of(&bar, "clip.mp4"));
    let player = opened(&shell, 0);
    let clip = fs::canonicalize(run.data()).unwrap().join("clip.mp4");
    assert_eq!(player.url, Url::from_file_path(&clip).unwrap().as_str());
    player.report(ready(PlayerActivity::Playing));
    wait_for_call(
        &player,
        &PlayerCall::Seek {
            position: Duration::from_secs(4),
            before: 0,
            after: 0,
        },
        1,
    );

    player.report_position(Duration::from_secs(30));
    player.report(ready(PlayerActivity::Paused(Some(PauseReason::Requested))));
    wait_until("where it paused is kept", || {
        kept_list(&run) == "30000\tfile:///data/clip.mp4\n"
    });
    player.report_position(Duration::from_secs(42));
    run.close();
    assert_eq!(
        kept_list(&run),
        "42000\tfile:///data/clip.mp4\n",
        "kept where it stood as the app ended"
    );
}

#[test]
fn a_folder_added_lists_its_video_which_a_click_plays_from_where_it_was_left_and_esc_leaves() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.media_assets.answer_with(minute_long_red_clip());
    let elsewhere = TempDir::new().unwrap();
    let films = fs::canonicalize(elsewhere.path()).unwrap().join("Films");
    fs::create_dir_all(&films).unwrap();
    fs::write(films.join("clip.mp4"), "a clip").unwrap();
    fs::write(films.join("notes.txt"), "not a video").unwrap();
    let clip = Url::from_file_path(films.join("clip.mp4")).unwrap();
    shell.file_dialogs.answer_next(vec![films.clone()]);
    let run = Run::start(&engine, &shell, &format!("30000\t{clip}\n"));
    let bar = wait_for_bar(&shell.menus, |bar| has(bar, "Add Folder…"));

    run.running.menu_chosen(id_of(&bar, "Add Folder…"));
    wait_until("the library reads the folder's video", || {
        shell.media_assets.opened().len() == 1
    });
    assert_eq!(shell.file_dialogs.asked()[0].kind, "open-folder");
    let asset = &shell.media_assets.opened()[0];
    assert_eq!(
        asset.url,
        clip.as_str(),
        "the video, not the notes beside it"
    );
    wait_until("its length is read, then its thumbnail", || {
        asset.reads() == ["duration", "metadata", "frame-at"]
    });
    assert_eq!(
        asset.frames_asked(),
        [FrameAsked {
            time: Duration::from_secs(6),
            before: u64::MAX,
            after: u64::MAX,
            max_size: Some((480.0, 480.0)),
        }],
        "a tenth of the way in, at the nearest keyframe, small"
    );
    let (shown, thumbnail) = wait_for_thumbnail(&run.window);
    let watched_at = |fraction: f64| {
        let x = thumbnail.left + ((thumbnail.right - thumbnail.left) as f64 * fraction) as usize;
        // Just beneath the red: the bar takes the thumbnail's bottom rows.
        pixel_of(&shown, x, thumbnail.bottom + 2)
    };
    assert!(
        is_white(watched_at(0.25)),
        "watched half way: {:?}",
        watched_at(0.25)
    );
    assert!(
        !is_white(watched_at(0.75)),
        "not watched past half way: {:?}",
        watched_at(0.75)
    );

    run.running.mouse().tap(&run.window, thumbnail.centre());
    let player = opened(&shell, 0);
    assert_eq!(player.url, clip.as_str());
    player.report(ready(PlayerActivity::Playing));
    wait_for_call(
        &player,
        &PlayerCall::Seek {
            position: Duration::from_secs(30),
            before: 0,
            after: 0,
        },
        1,
    );

    Keyboard::press(&run.window, "escape").unwrap();
    wait_until("Esc goes back to the library", || player.is_dropped());
    wait_for_thumbnail(&run.window);
    run.close();
}

#[test]
fn a_thumbnail_is_kept_encoded_from_its_frame_and_the_next_run_draws_it_decoded_with_no_video_read()
{
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.media_assets.answer_with(minute_long_red_clip());
    shell.image_codecs.answer_with(ImageAnswers {
        decoded_formats: vec!["image/png".to_owned()],
        encoded_formats: vec!["image/png".to_owned()],
        opened: Ok(ImageFacts::still("image/png", 480.0, 270.0)),
        pixels: Ok([255, 0, 0, 255]),
        encoded: Ok(b"a kept thumbnail".to_vec()),
    });
    // A folder of the program's own, which every run reaches, listed as the library's.
    let folders = TempDir::new().unwrap();
    fs::create_dir_all(folders.path().join("data/Films")).unwrap();
    fs::create_dir_all(folders.path().join("cache")).unwrap();
    fs::write(folders.path().join("data/Films/clip.mp4"), "a clip").unwrap();
    fs::write(folders.path().join("data/folders"), "/data/Films\n").unwrap();
    let first = Run::in_folders(&engine, &shell, 0, folders);
    wait_for_thumbnail(&first.window);
    wait_until("the thumbnail is kept", || {
        first.kept_thumbnails().len() == 1
    });
    let kept = first.kept_thumbnails()[0].clone();
    assert_eq!(fs::read(&kept).unwrap(), b"a kept thumbnail");
    assert!(
        shell.image_codecs.asked().contains(&Asked::Encode {
            width: 64,
            height: 48,
            format: "image/png".to_owned(),
            quality: None,
            hardware_first_pixel: Some(THUMBNAIL_RED),
        }),
        "the frame's own hardware buffer encoded: {:?}",
        shell.image_codecs.asked()
    );
    first.close();

    let opened_before = shell.media_assets.opened().len();
    let asked_before = shell.image_codecs.asked().len();
    let again = Run::in_folders(&engine, &shell, 1, first.folders);
    wait_for_thumbnail(&again.window);
    let asked = shell.image_codecs.asked()[asked_before..].to_vec();
    let machine_path = fs::canonicalize(&kept).unwrap();
    assert_eq!(
        asked[..2],
        [
            Asked::Open {
                program: "video-player".to_owned(),
                from: OpenedFrom::File(machine_path),
            },
            Asked::Decode {
                index: 0,
                max_size: None,
                hardware: true,
            },
        ],
        "decoded into a hardware buffer from where it was kept"
    );
    assert_eq!(
        shell.media_assets.opened().len(),
        opened_before,
        "its length and its thumbnail kept, the video is not read"
    );
    again.close();
}

/// What the shell's reading of media answers of every video: a minute long, with no title, and
/// a red picture.
fn minute_long_red_clip() -> AssetAnswers {
    AssetAnswers {
        duration: Ok(Some(MINUTE)),
        tracks: Err(PlayerError {
            kind: PlayerErrorKind::Other,
            message: "not asked".to_owned(),
        }),
        metadata: Ok(MediaMetadata {
            title: None,
            artist: None,
            album: None,
            artwork: None,
        }),
        picture: Ok(THUMBNAIL_RED),
    }
}

/// Where a thumbnail is on a view, in its physical pixels: its columns from `left` up to `right`
/// and its rows from `top` up to `bottom`.
#[derive(Clone, Copy, Debug)]
struct Region {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

impl Region {
    fn centre(self) -> (f64, f64) {
        (
            (self.left + self.right) as f64 / 2.0,
            (self.top + self.bottom) as f64 / 2.0,
        )
    }
}

/// Waits until `window` shows a red thumbnail, and answers what it shows with where the thumbnail
/// is.
fn wait_for_thumbnail(window: &ProgramView) -> (PixelBuffer, Region) {
    let found = std::cell::RefCell::new(None);
    wait_until("the window shows the video's thumbnail", || {
        let Some(shown) = window.picture().expect("the screenshot is drawn") else {
            return false;
        };
        let region = red_region(&shown);
        *found.borrow_mut() = region.map(|region| (shown, region));
        region.is_some()
    });
    found.into_inner().expect("a thumbnail")
}

/// Where `shown` is red, the thumbnails' colour, or about it, as drawing and scaling leave it; none
/// where nothing is.
fn red_region(shown: &PixelBuffer) -> Option<Region> {
    let mut region: Option<Region> = None;
    for y in 0..shown.height as usize {
        for x in 0..shown.width as usize {
            let [blue, green, red, _alpha] = pixel_of(shown, x, y);
            if red > 200 && green < 60 && blue < 60 {
                let grown = region.get_or_insert(Region {
                    left: x,
                    top: y,
                    right: x + 1,
                    bottom: y + 1,
                });
                grown.left = grown.left.min(x);
                grown.top = grown.top.min(y);
                grown.right = grown.right.max(x + 1);
                grown.bottom = grown.bottom.max(y + 1);
            }
        }
    }
    region.filter(|region| region.right - region.left > 20 && region.bottom - region.top > 20)
}

fn is_white([blue, green, red, _alpha]: [u8; 4]) -> bool {
    blue > 200 && green > 200 && red > 200
}

/// `builtin-apps/video-player` running with this computer's fonts and folders of its own, its
/// recent list `recents`; and its window, once it asked for it.
struct Run {
    running: RunningProgram,
    shown: Arc<HeadlessView>,
    window: ProgramView,
    output: Arc<RecordedOutput>,
    folders: TempDir,
}

impl Run {
    fn start(engine: &WaOS, shell: &TestShell, recents: &str) -> Run {
        let folders = TempDir::new().unwrap();
        fs::create_dir_all(folders.path().join("data")).unwrap();
        fs::create_dir_all(folders.path().join("cache")).unwrap();
        fs::write(folders.path().join("data/recents"), recents).unwrap();
        Run::in_folders(engine, shell, 0, folders)
    }

    /// The app running in `folders`, its `/data` and `/cache`, with what the test or an earlier
    /// run put there.
    fn in_folders(engine: &WaOS, shell: &TestShell, view_index: usize, folders: TempDir) -> Run {
        let own = ProgramFolders {
            data: folders.path().join("data"),
            cache: folders.path().join("cache"),
        };
        let output = Arc::new(RecordedOutput::default());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
            folders: Some(own),
            ..waos_local::LocalCapabilities::new().environment()
        };
        let program = engine
            .load(Path::new(waos_test_programs::VIDEO_PLAYER))
            .unwrap();
        let running = engine.run(&program, environment).unwrap();
        let shown =
            within(shell.headless.view(view_index)).expect("the video player asks for its window");
        let window = shown.view().expect("the video player's view");
        Run {
            running,
            shown,
            window,
            output,
            folders,
        }
    }

    /// The program's own `/data`, on the machine.
    fn data(&self) -> PathBuf {
        self.folders.path().join("data")
    }

    /// The thumbnails the program kept in its own `/cache`, on the machine.
    fn kept_thumbnails(&self) -> Vec<PathBuf> {
        let kept = fs::read_dir(self.folders.path().join("cache/thumbnails"));
        kept.map(|kept| kept.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default()
    }

    /// Asks the window to close, which quits the app, and waits for its end.
    fn close(&self) {
        self.window.close_requested();
        let end = within(self.running.ended());
        assert!(
            matches!(end, ProgramEnd::Returned(Ok(()))),
            "closing its window quits it, not {end:?}; it printed {:#?}",
            self.output.lines()
        );
    }
}

/// The recent list as the app keeps it in its `/data`.
fn kept_list(run: &Run) -> String {
    fs::read_to_string(run.data().join("recents")).unwrap_or_default()
}

/// A player's state once it can play: a minute long, all of it seekable, with an audio track
/// and an English text track.
fn ready(activity: PlayerActivity) -> PlayerState {
    PlayerState {
        status: PlayerStatus::Ready,
        activity,
        seeking: None,
        rate: 1.0,
        volume: 1.0,
        muted: false,
        duration: Some(nanoseconds(MINUTE)),
        loaded: vec![TimeRange {
            start: 0,
            end: nanoseconds(Duration::from_secs(20)),
        }],
        seekable: vec![TimeRange {
            start: 0,
            end: nanoseconds(MINUTE),
        }],
        size: Some(Size {
            width: 640.0,
            height: 360.0,
        }),
        tracks: vec![
            track("audio-1", MediaTrackKind::Audio, "Main", true),
            track("text-en", MediaTrackKind::Text, "English", false),
        ],
    }
}

fn track(id: &str, kind: MediaTrackKind, label: &str, selected: bool) -> MediaTrack {
    MediaTrack {
        id: id.to_owned(),
        kind,
        language: "en".to_owned(),
        label: label.to_owned(),
        selected,
    }
}

fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap()
}

/// The player opened after `before` others, once the shell has it.
fn opened(shell: &TestShell, before: usize) -> OpenedPlayer {
    wait_until("the shell opens a player", || {
        shell.media_players.opened().len() > before
    });
    shell.media_players.opened()[before].clone()
}

/// Waits until `player` has been asked `call` `count` times.
fn wait_for_call(player: &OpenedPlayer, call: &PlayerCall, count: usize) {
    wait_until(&format!("{call:?} asked {count} times"), || {
        calls_of(player, |asked| asked == call) >= count
    });
}

fn calls_of(player: &OpenedPlayer, which: impl Fn(&PlayerCall) -> bool) -> usize {
    player.calls().iter().filter(|call| which(call)).count()
}

/// Waits until the program has set a menu bar `wanted` accepts, and answers it.
fn wait_for_bar(menus: &TestMenus, wanted: impl Fn(&[MenuEntry]) -> bool) -> Vec<MenuEntry> {
    wait_until("the menu bar wanted", || {
        menus.bars().last().is_some_and(|(_, bar)| wanted(bar))
    });
    menus.bars().pop().expect("a bar").1
}

fn has(bar: &[MenuEntry], label: &str) -> bool {
    bar.iter().any(|entry| label_of(entry) == label)
}

/// The id of the entry titled or labelled `label`.
fn id_of(bar: &[MenuEntry], label: &str) -> u32 {
    bar.iter()
        .find(|entry| label_of(entry) == label)
        .unwrap_or_else(|| panic!("no {label} in {bar:?}"))
        .id
}

/// A submenu's title or an item's label; nothing for another entry.
fn label_of(entry: &MenuEntry) -> &str {
    match &entry.kind {
        MenuEntryKind::Submenu(Submenu { label, .. }) => label,
        MenuEntryKind::Item(item) => &item.label,
        MenuEntryKind::Provided(_) | MenuEntryKind::Separator => "",
    }
}
