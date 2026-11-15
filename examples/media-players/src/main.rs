//! A program that plays media through the system's players: it opens a player on each source
//! listed in `/data/sources`, one a line, sends each one's picture to a composition layer of its
//! own, where the system has a compositor, and plays it from two seconds in; then, in the order
//! listed, says each player's state once it no longer loads, as `source: ready 640x360, playing` or
//! `source: failed (why)`; then shows the first player's first text track, where it lists one,
//! saying `text track <id> on`, and says each change of the first player's cues, from those now,
//! as `cues: Hello` or `cues: none`, of its media's title, as `title: Bip Bop`, and of its state,
//! until it is stopped. Where the system has no players it says `media players: none`.
//!
//! ```text
//! cargo build -p media-players --target wasm32-wasip3
//! ```

use std::fs;

use wasdk::media_players::{
    self, Cue, CueContent, MediaPlayer, MediaTrackKind, PlayerActivity, PlayerState, PlayerStatus,
    SeekTolerance,
};
use wasdk::video_frames::VideoOutput;
use wasdk::views::{self, CompositionLayer};

/// What lists the sources it plays.
const SOURCES: &str = "/data/sources";

/// Where it plays each source from, in nanoseconds: two seconds in.
const START: u64 = 2_000_000_000;

/// A player of one source, with the layer its picture goes to.
struct Opened {
    source: String,
    player: MediaPlayer,
    /// Held while the player draws on it; none where the system has no compositor.
    _layer: Option<CompositionLayer>,
}

#[wasdk::main]
async fn main() {
    let Some(players) = media_players::get_media_players() else {
        println!("media players: none");
        return;
    };
    let sources = fs::read_to_string(SOURCES).unwrap_or_default();
    let opened: Vec<Opened> = sources
        .lines()
        .map(|source| play(&players, source))
        .collect();
    let mut loaded = Vec::new();
    for opened in &opened {
        loaded.push(opened.say_once_loaded().await);
    }
    if let (Some(first), Some(state)) = (opened.first(), loaded.first()) {
        first.show_first_text_track(state);
        first.say_cues_and_metadata();
        first.say_each_change().await;
    }
}

/// A player of `source`, playing from [`START`], its picture on a layer of its own.
fn play(players: &media_players::MediaPlayers, source: &str) -> Opened {
    let player = players.open(source);
    let layer = views::get_compositor().map(|compositor| compositor.make_layer(false));
    if let Some(layer) = &layer {
        player.set_video_output(Some(&VideoOutput::Layer(layer)));
    }
    player.seek(
        START,
        SeekTolerance {
            before: 0,
            after: 0,
        },
    );
    player.play();
    Opened {
        source: source.to_owned(),
        player,
        _layer: layer,
    }
}

impl Opened {
    /// Says the player's state once it no longer loads, and answers it.
    async fn say_once_loaded(&self) -> PlayerState {
        let mut changes = self.player.state_changes();
        let mut state = self.player.state();
        while matches!(state.status, PlayerStatus::Loading) {
            match changes.next().await {
                Some(next) => state = next,
                None => break,
            }
        }
        self.say(&state);
        state
    }

    /// Shows the first text track `state` lists, where it lists one, and says so.
    fn show_first_text_track(&self, state: &PlayerState) {
        let text_track = state
            .tracks
            .iter()
            .find(|track| track.kind == MediaTrackKind::Text);
        if let Some(track) = text_track {
            self.player.select_text_track(Some(&track.id));
            println!("text track {} on", track.id);
        }
    }

    /// Says the cues to show each time they change, from those now, and the media's title each
    /// time its metadata changes, each in a task of its own until the player goes.
    fn say_cues_and_metadata(&self) {
        let mut cues = self.player.cue_changes();
        wasdk::spawn(async move {
            while let Some(shown) = cues.next().await {
                println!("cues: {}", described_cues(&shown));
            }
        });
        let mut metadata = self.player.metadata_changes();
        wasdk::spawn(async move {
            while let Some(metadata) = metadata.next().await {
                let title = metadata.title.as_deref().unwrap_or("none");
                println!("title: {title}");
            }
        });
    }

    /// Says each state the player changes to, until the program is stopped.
    async fn say_each_change(&self) {
        let mut changes = self.player.state_changes();
        while let Some(state) = changes.next().await {
            self.say(&state);
        }
    }

    fn say(&self, state: &PlayerState) {
        println!("{}: {}", self.source, described(state));
    }
}

/// The text of `cues`, a cue's runs joined, the cues apart by ` / `; `none` for none, and
/// `image` for a picture subtitle.
fn described_cues(cues: &[Cue]) -> String {
    if cues.is_empty() {
        return "none".to_owned();
    }
    let texts: Vec<String> = cues
        .iter()
        .map(|cue| match &cue.content {
            CueContent::Text(runs) => runs.iter().map(|run| run.text.as_str()).collect(),
            CueContent::Image(_) => "image".to_owned(),
        })
        .collect();
    texts.join(" / ")
}

/// `state` in a few words: its status, the picture's size once known, and whether it plays.
fn described(state: &PlayerState) -> String {
    let status = match &state.status {
        PlayerStatus::Loading => "loading".to_owned(),
        PlayerStatus::Ready => "ready".to_owned(),
        PlayerStatus::Failed(error) => format!("failed ({})", error.message),
    };
    let size = state
        .size
        .map(|size| format!(" {}x{}", size.width, size.height))
        .unwrap_or_default();
    let playing = match state.activity {
        PlayerActivity::Playing => ", playing",
        _ => "",
    };
    format!("{status}{size}{playing}")
}
