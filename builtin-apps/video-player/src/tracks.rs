//! The media's subtitles and audio as the user chooses among them, alike in the View menu and in
//! the controls: "Off" and each text track, and each audio track, the one shown or playing marked.

use waft_embedder::{MediaTrack, MediaTrackKind, PlayerState};
use waft_foundation::{App, Handle};
use waft_widgets::Player;

/// One track to choose, or none of the text tracks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrackChoice {
    pub(crate) label: String,
    /// Whether it is the one shown or playing now.
    pub(crate) chosen: bool,
    pub(crate) pick: TrackPick,
}

/// What choosing a track asks of the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TrackPick {
    /// The text track of this id, or none: subtitles off.
    Subtitles(Option<String>),
    /// The audio track of this id.
    Audio(String),
}

/// "Off", then each of the media's text tracks.
pub(crate) fn subtitle_choices(state: &PlayerState) -> Vec<TrackChoice> {
    let texts = tracks_of(state, MediaTrackKind::Text);
    let off = TrackChoice {
        label: "Off".to_owned(),
        chosen: !texts.iter().any(|track| track.selected),
        pick: TrackPick::Subtitles(None),
    };
    let numbered = texts.into_iter().enumerate();
    let tracks = numbered.map(|(index, track)| TrackChoice {
        label: label_of(track, index),
        chosen: track.selected,
        pick: TrackPick::Subtitles(Some(track.id.clone())),
    });
    std::iter::once(off).chain(tracks).collect()
}

/// Each of the media's audio tracks; none for media with one or none, which leaves nothing to
/// choose.
pub(crate) fn audio_choices(state: &PlayerState) -> Vec<TrackChoice> {
    let audio = tracks_of(state, MediaTrackKind::Audio);
    if audio.len() < 2 {
        return Vec::new();
    }
    let numbered = audio.into_iter().enumerate();
    numbered
        .map(|(index, track)| TrackChoice {
            label: label_of(track, index),
            chosen: track.selected,
            pick: TrackPick::Audio(track.id.clone()),
        })
        .collect()
}

/// Asks `player` for the track `pick` names.
pub(crate) fn pick_track(app: &mut App, player: Handle<Player>, pick: &TrackPick) {
    match pick {
        TrackPick::Subtitles(id) => player.select_text_track(app, id.as_deref()),
        TrackPick::Audio(id) => player.select_audio_track(app, id),
    }
}

fn tracks_of(state: &PlayerState, kind: MediaTrackKind) -> Vec<&MediaTrack> {
    state
        .tracks
        .iter()
        .filter(|track| track.kind == kind)
        .collect()
}

/// What a track is called: its name as the media gives it, else its language, else its number
/// among its kind's.
fn label_of(track: &MediaTrack, index: usize) -> String {
    if !track.label.is_empty() {
        return track.label.clone();
    }
    if !track.language.is_empty() {
        return track.language.clone();
    }
    format!("Track {}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, kind: MediaTrackKind, label: &str, selected: bool) -> MediaTrack {
        MediaTrack {
            id: id.to_owned(),
            kind,
            language: "en".to_owned(),
            label: label.to_owned(),
            selected,
        }
    }

    fn with_tracks(tracks: Vec<MediaTrack>) -> PlayerState {
        PlayerState {
            tracks,
            ..PlayerState::loading()
        }
    }

    #[test]
    fn subtitles_offer_off_and_each_text_track_the_shown_one_chosen() {
        let state = with_tracks(vec![
            track("audio-1", MediaTrackKind::Audio, "Main", true),
            track("text-en", MediaTrackKind::Text, "English", false),
            track("text-fr", MediaTrackKind::Text, "", true),
        ]);
        let choices = subtitle_choices(&state);
        let shown: Vec<_> = choices
            .iter()
            .map(|choice| (choice.label.as_str(), choice.chosen))
            .collect();
        assert_eq!(shown, [("Off", false), ("English", false), ("en", true)]);
        assert_eq!(
            choices[1].pick,
            TrackPick::Subtitles(Some("text-en".to_owned()))
        );
    }

    #[test]
    fn subtitles_are_off_while_no_text_track_shows() {
        let state = with_tracks(vec![track(
            "text-en",
            MediaTrackKind::Text,
            "English",
            false,
        )]);
        assert!(subtitle_choices(&state)[0].chosen);
    }

    #[test]
    fn audio_offers_a_choice_only_among_two_tracks_or_more() {
        let one = with_tracks(vec![track("a", MediaTrackKind::Audio, "Main", true)]);
        assert!(audio_choices(&one).is_empty());
        let two = with_tracks(vec![
            track("a", MediaTrackKind::Audio, "", true),
            track("b", MediaTrackKind::Audio, "Commentary", false),
        ]);
        let choices = audio_choices(&two);
        assert_eq!(choices[0].label, "en");
        assert_eq!(choices[1].pick, TrackPick::Audio("b".to_owned()));
    }
}
