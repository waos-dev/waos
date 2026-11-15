//! An Waft app that plays Apple's HLS example stream: a window with the video and three buttons,
//! one that pauses and plays it, one that shows its English subtitles or none, and one that
//! switches how the video is shown: on a composition layer the system draws it on, Waft's
//! `PlayerView`, with the subtitles over it, or drawn by the app from the frames a frame reader
//! reads, Waft's `VideoFrameView`, with a `SubtitleView` over it. The way is kept in `/data/view`,
//! `layer` or `frames`, so a run starts as the last one was left. It prints the player's status each
//! time it changes, as `player: loading`, `player: ready 1920x1080` or `player: failed (why)`, the
//! text track it shows, as `subtitles: English`, and, ten seconds in, how many of the video's frames
//! it drew itself, as `drew 597 video frames in the first ten seconds`. Where the system has no
//! players it says so in the window. Asking to close its window quits it.
//!
//! ```text
//! cargo build -p player --target wasm32-wasip3
//! cargo waos run -p player
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod frame_count;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use waft_cupertino::{CupertinoApp, CupertinoButton, CupertinoPageScaffold};
use waft_embedder::{
    MediaTrack, MediaTrackKind, PlayerActivity, PlayerState, PlayerStatus, VideoFrameReader,
    VideoOutput, WindowConfig,
};
use waft_foundation::{App, Handle, Listenable, Listener, ValueNotifier};
use waft_widgets::{
    AlignmentGeometry, Center, Column, EdgeInsetsGeometry, Expanded, IntoWidget, ListenableBuilder,
    Padding, Player, PlayerView, Positioned, Row, Stack, SubtitleView, Text, VideoFrameView,
    WidgetRef, run_app_with,
};

use frame_count::FrameCount;

/// Apple's example of an HLS stream, in fragmented MP4 at several bitrates, with subtitles.
const STREAM: &str = "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8";

/// Where the way the video is shown is kept between runs.
const VIEW_FILE: &str = "/data/view";

/// The language whose subtitles the subtitles button shows.
const SUBTITLES: &str = "en";

/// How the video is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    /// On a composition layer, drawn by the system.
    Layer,
    /// Drawn by the app from its frame reader's frames.
    Frames,
}

#[wasdk::main]
async fn main() {
    let frames_drawn = FrameCount::install();
    wasdk::spawn(frames_drawn.say_after_ten_seconds());
    wasdk_adapter_waft::run(|app| {
        let window = WindowConfig {
            title: "Player".to_owned(),
            size: [800.0, 560.0],
            ..WindowConfig::default()
        };
        let page = CupertinoPageScaffold::new(home(app));
        run_app_with(app, CupertinoApp::new().home(page).into_widget(), window);
    })
    .await;
}

/// The page: the player and its buttons; a line saying there are no players where the system has
/// none.
fn home(app: &mut App) -> WidgetRef {
    let Some(player) = Player::new(app, STREAM) else {
        return Center::new()
            .child(Text::new("This system has no media players"))
            .into_widget();
    };
    print_changes(app, player);
    let reader = app
        .platform()
        .video_frame_readers()
        .map(|readers| readers.make_reader());
    let way = app.create(ValueNotifier::new(Way::Layer));
    show_as(app, player, reader.as_ref(), way, kept_way());
    player.play(app);
    let shown: Rc<dyn Listenable> = Rc::new(way);
    ListenableBuilder::new(shown, move |app, _context, _child| {
        let video = match (*app.get(way).value(), &reader) {
            (Way::Frames, Some(reader)) => drawn_frames(player, reader),
            _ => PlayerView::new(player).into_widget(),
        };
        Column::new()
            .children([
                Expanded::new(Center::new().child(video)).into_widget(),
                Padding::new(EdgeInsetsGeometry::all(16.0))
                    .child(buttons(player, reader.clone(), way))
                    .into_widget(),
            ])
            .into_widget()
    })
    .into_widget()
}

/// The video drawn by the app from `reader`'s frames, with the player's subtitles over it.
fn drawn_frames(player: Handle<Player>, reader: &Arc<dyn VideoFrameReader>) -> WidgetRef {
    let video = VideoFrameView::new(Arc::clone(reader)).into_widget();
    let listenable: Rc<dyn Listenable> = Rc::new(player);
    ListenableBuilder::new(listenable, move |app, _context, _child| {
        Stack::new()
            .alignment(AlignmentGeometry::TOP_LEFT)
            .children([
                video.clone(),
                Positioned::fill(SubtitleView::new(player.cues(app).to_vec())).into_widget(),
            ])
            .into_widget()
    })
    .into_widget()
}

/// Play or pause, the subtitles, and the way the video is shown, each labelled for what it does.
fn buttons(
    player: Handle<Player>,
    reader: Option<Arc<dyn VideoFrameReader>>,
    way: Handle<ValueNotifier<Way>>,
) -> WidgetRef {
    let listenable: Rc<dyn Listenable> = Rc::new(player);
    ListenableBuilder::new(listenable, move |app, _context, _child| {
        let state = player.state(app);
        let mut buttons = vec![
            play_or_pause(player, is_paused(state)),
            subtitles_button(player, state),
        ];
        if let Some(reader) = &reader {
            buttons.push(way_button(player, reader, way, *app.get(way).value()));
        }
        let spaced = buttons.into_iter().map(|button| {
            Padding::new(EdgeInsetsGeometry::all(8.0))
                .child(button)
                .into_widget()
        });
        Center::new()
            .child(Row::new().children(spaced))
            .into_widget()
    })
    .into_widget()
}

/// Play while the player is paused, by the program or the system; pause while it plays or waits
/// to.
fn play_or_pause(player: Handle<Player>, paused: bool) -> WidgetRef {
    let label = if paused { "Play" } else { "Pause" };
    let toggle = Listener::new(move |app| {
        if is_paused(player.state(app)) {
            player.play(app);
        } else {
            player.pause(app);
        }
    });
    CupertinoButton::filled(Text::new(label).into_widget(), Some(toggle)).into_widget()
}

/// Shows the subtitles in [`SUBTITLES`] while none are shown, and none while some are.
fn subtitles_button(player: Handle<Player>, state: &PlayerState) -> WidgetRef {
    let shown = shown_text_track(&state.tracks).is_some();
    let label = if shown {
        "Hide subtitles"
    } else {
        "Show subtitles"
    };
    let toggle = Listener::new(move |app| {
        let tracks = &player.state(app).tracks;
        if shown_text_track(tracks).is_some() {
            player.select_text_track(app, None);
        } else if let Some(track) = text_track_in(tracks, SUBTITLES) {
            let id = track.id.clone();
            player.select_text_track(app, Some(&id));
        }
    });
    CupertinoButton::filled(Text::new(label).into_widget(), Some(toggle)).into_widget()
}

/// Switches the video from the way it is shown now, `now`, to the other.
fn way_button(
    player: Handle<Player>,
    reader: &Arc<dyn VideoFrameReader>,
    way: Handle<ValueNotifier<Way>>,
    now: Way,
) -> WidgetRef {
    let (label, other) = match now {
        Way::Layer => ("Draw the frames", Way::Frames),
        Way::Frames => ("Show on a layer", Way::Layer),
    };
    let reader = Arc::clone(reader);
    let switch = Listener::new(move |app| {
        show_as(app, player, Some(&reader), way, other);
        keep_way(other);
    });
    CupertinoButton::filled(Text::new(label).into_widget(), Some(switch)).into_widget()
}

/// Shows `player`'s video `as_way`: its frames sent to `reader` before the frames' view is built;
/// on a layer, the `PlayerView` built for it takes the video as it is mounted.
fn show_as(
    app: &mut App,
    player: Handle<Player>,
    reader: Option<&Arc<dyn VideoFrameReader>>,
    way: Handle<ValueNotifier<Way>>,
    as_way: Way,
) {
    let as_way = if reader.is_some() { as_way } else { Way::Layer };
    if let (Way::Frames, Some(reader)) = (as_way, reader) {
        player.set_video_output(app, Some(VideoOutput::Frames(Arc::clone(reader))));
    }
    way.set_value(app, as_way);
}

/// The way the last run left the video shown; on a layer at the first.
fn kept_way() -> Way {
    match std::fs::read_to_string(VIEW_FILE).as_deref().map(str::trim) {
        Ok("frames") => Way::Frames,
        _ => Way::Layer,
    }
}

/// Keeps `way` for the next run.
fn keep_way(way: Way) {
    let word = match way {
        Way::Layer => "layer",
        Way::Frames => "frames",
    };
    if let Err(error) = std::fs::write(VIEW_FILE, word) {
        eprintln!("the way the video is shown is not kept: {error}");
    }
}

/// Prints `player`'s status, with its picture's size once known, and the text track it shows, each
/// time one changes.
fn print_changes(app: &mut App, player: Handle<Player>) {
    let printed = RefCell::new((String::new(), String::new()));
    let print = Listener::new(move |app| {
        let state = player.state(app);
        let status = described(state);
        let subtitles = shown_text_track(&state.tracks)
            .map_or_else(|| "none".to_owned(), |track| track.label.clone());
        let mut printed = printed.borrow_mut();
        if printed.0 != status {
            println!("player: {status}");
            printed.0 = status;
        }
        if printed.1 != subtitles {
            println!("subtitles: {subtitles}");
            printed.1 = subtitles;
        }
    });
    print.call(app);
    player.add_listener(app, print);
}

/// The text track shown, where one is.
fn shown_text_track(tracks: &[MediaTrack]) -> Option<&MediaTrack> {
    tracks
        .iter()
        .find(|track| track.kind == MediaTrackKind::Text && track.selected)
}

/// The first text track whose language is `language` or starts with it, such as `en` for `en-US`.
fn text_track_in<'a>(tracks: &'a [MediaTrack], language: &str) -> Option<&'a MediaTrack> {
    tracks.iter().find(|track| {
        track.kind == MediaTrackKind::Text
            && (track.language == language || track.language.starts_with(&format!("{language}-")))
    })
}

/// `state`'s status, with the picture's size once known.
fn described(state: &PlayerState) -> String {
    let size = state
        .size
        .map(|size| format!(" {}x{}", size.width(), size.height()))
        .unwrap_or_default();
    match &state.status {
        PlayerStatus::Loading => "loading".to_owned(),
        PlayerStatus::Ready => format!("ready{size}"),
        PlayerStatus::Failed(error) => format!("failed ({})", error.message),
    }
}

fn is_paused(state: &PlayerState) -> bool {
    matches!(state.activity, PlayerActivity::Paused(_))
}
