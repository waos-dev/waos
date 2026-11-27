//! A player's state and its media's metadata in Waft's terms, as the program reads them: field for
//! field, a time in the nanoseconds of a WIT `duration`. A media asset's tracks, metadata and
//! failures are the player's, and read so too; a program's seek tolerance goes the other way, as
//! Waft takes it.

use std::time::Duration;

use waft::ui;
use waos::bindings::wasdk::media::media_players::{
    EncodedImage, MediaMetadata, MediaTrack, MediaTrackKind, PauseReason, PlayerActivity,
    PlayerError, PlayerErrorKind, PlayerState, PlayerStatus, SeekTolerance, TimeRange, WaitReason,
};
use waos::bindings::wasdk::platform::windows::Size;

/// Waft's `state` of a player, as the program reads it.
pub(crate) fn program_state(state: ui::PlayerState) -> PlayerState {
    PlayerState {
        status: program_status(state.status),
        activity: program_activity(state.activity),
        seeking: state.seeking.map(nanoseconds),
        rate: state.rate,
        volume: state.volume as f32,
        muted: state.muted,
        duration: state.duration.map(nanoseconds),
        loaded: state.loaded.into_iter().map(program_range).collect(),
        seekable: state.seekable.into_iter().map(program_range).collect(),
        size: state.size.map(|size| Size {
            width: size.width(),
            height: size.height(),
        }),
        tracks: state.tracks.into_iter().map(program_track).collect(),
    }
}

fn program_activity(activity: ui::PlayerActivity) -> PlayerActivity {
    match activity {
        ui::PlayerActivity::Paused(reason) => {
            PlayerActivity::Paused(reason.map(program_pause_reason))
        }
        ui::PlayerActivity::Waiting(reason) => {
            PlayerActivity::Waiting(reason.map(program_wait_reason))
        }
        ui::PlayerActivity::Playing => PlayerActivity::Playing,
    }
}

fn program_pause_reason(reason: ui::PauseReason) -> PauseReason {
    match reason {
        ui::PauseReason::Requested => PauseReason::Requested,
        ui::PauseReason::End => PauseReason::End,
        ui::PauseReason::Interrupted => PauseReason::Interrupted,
        ui::PauseReason::OutputRemoved => PauseReason::OutputRemoved,
        ui::PauseReason::Backgrounded => PauseReason::Backgrounded,
    }
}

fn program_wait_reason(reason: ui::WaitReason) -> WaitReason {
    match reason {
        ui::WaitReason::Data => WaitReason::Data,
        ui::WaitReason::EvaluatingData => WaitReason::EvaluatingData,
        ui::WaitReason::HeldBySystem => WaitReason::HeldBySystem,
    }
}

fn program_range(range: ui::TimeRange) -> TimeRange {
    TimeRange {
        start: nanoseconds(range.start),
        end: nanoseconds(range.end),
    }
}

pub(crate) fn program_track(track: ui::MediaTrack) -> MediaTrack {
    MediaTrack {
        id: track.id,
        kind: match track.kind {
            ui::MediaTrackKind::Audio => MediaTrackKind::Audio,
            ui::MediaTrackKind::Video => MediaTrackKind::Video,
            ui::MediaTrackKind::Text => MediaTrackKind::Text,
        },
        language: track.language,
        label: track.label,
        selected: track.selected,
    }
}

/// Waft's `metadata` of a player's media, as the program reads it.
pub(crate) fn program_metadata(metadata: ui::MediaMetadata) -> MediaMetadata {
    MediaMetadata {
        title: metadata.title,
        artist: metadata.artist,
        album: metadata.album,
        artwork: metadata.artwork.map(|artwork| EncodedImage {
            mime_type: artwork.mime_type,
            data: artwork.data.to_vec(),
        }),
    }
}

fn program_status(status: ui::PlayerStatus) -> PlayerStatus {
    match status {
        ui::PlayerStatus::Loading => PlayerStatus::Loading,
        ui::PlayerStatus::Ready => PlayerStatus::Ready,
        ui::PlayerStatus::Failed(error) => PlayerStatus::Failed(program_error(error)),
    }
}

pub(crate) fn program_error(error: ui::PlayerError) -> PlayerError {
    PlayerError {
        kind: match error.kind {
            ui::PlayerErrorKind::Network => PlayerErrorKind::Network,
            ui::PlayerErrorKind::Decode => PlayerErrorKind::Decode,
            ui::PlayerErrorKind::Unsupported => PlayerErrorKind::Unsupported,
            ui::PlayerErrorKind::Protected => PlayerErrorKind::Protected,
            ui::PlayerErrorKind::Refused => PlayerErrorKind::Refused,
            ui::PlayerErrorKind::Other => PlayerErrorKind::Other,
        },
        message: error.message,
    }
}

/// `duration` in nanoseconds; the longest a WIT `duration` holds for one past it.
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

fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wafts_state_is_the_programs_field_for_field() {
        let state = program_state(ui::PlayerState {
            status: ui::PlayerStatus::Ready,
            activity: ui::PlayerActivity::Waiting(Some(ui::WaitReason::Data)),
            seeking: Some(Duration::from_secs(3)),
            rate: 2.0,
            volume: 0.5,
            muted: true,
            duration: Some(Duration::from_secs(10)),
            loaded: vec![ui::TimeRange {
                start: Duration::ZERO,
                end: Duration::from_millis(1500),
            }],
            seekable: vec![ui::TimeRange {
                start: Duration::ZERO,
                end: Duration::from_secs(10),
            }],
            size: Some(ui::Size::new(1920.0, 1080.0)),
            tracks: vec![ui::MediaTrack {
                id: "audio-1".to_owned(),
                kind: ui::MediaTrackKind::Audio,
                language: "en".to_owned(),
                label: "English".to_owned(),
                selected: true,
            }],
        });
        assert!(matches!(state.status, PlayerStatus::Ready));
        assert!(matches!(
            state.activity,
            PlayerActivity::Waiting(Some(WaitReason::Data))
        ));
        assert_eq!(state.seeking, Some(3_000_000_000));
        assert_eq!((state.rate, state.volume, state.muted), (2.0, 0.5, true));
        assert_eq!(state.duration, Some(10_000_000_000));
        assert_eq!(
            (state.loaded[0].start, state.loaded[0].end),
            (0, 1_500_000_000)
        );
        assert_eq!(state.seekable[0].end, 10_000_000_000);
        let size = state.size.unwrap();
        assert_eq!((size.width, size.height), (1920.0, 1080.0));
        let track = &state.tracks[0];
        assert_eq!(
            (track.id.as_str(), track.kind),
            ("audio-1", MediaTrackKind::Audio)
        );
        assert_eq!(
            (track.language.as_str(), track.label.as_str()),
            ("en", "English")
        );
        assert!(track.selected);
    }

    #[test]
    fn the_medias_metadata_is_the_programs_its_artwork_copied() {
        let metadata = program_metadata(ui::MediaMetadata {
            title: Some("Bip Bop".to_owned()),
            artwork: Some(ui::EncodedImage {
                mime_type: "image/png".to_owned(),
                data: vec![0x89, b'P'].into(),
            }),
            ..ui::MediaMetadata::default()
        });
        assert_eq!(metadata.title.as_deref(), Some("Bip Bop"));
        assert_eq!(metadata.artist, None);
        let artwork = metadata.artwork.unwrap();
        assert_eq!(
            (artwork.mime_type.as_str(), artwork.data),
            ("image/png", vec![0x89, b'P'])
        );
    }

    #[test]
    fn a_failure_keeps_its_kind_and_the_systems_words_and_knows_nothing_else() {
        let state = program_state(ui::PlayerState::failed(ui::PlayerError {
            kind: ui::PlayerErrorKind::Network,
            message: "the network is down".to_owned(),
        }));
        assert!(matches!(
            state.status,
            PlayerStatus::Failed(PlayerError { kind: PlayerErrorKind::Network, message })
                if message == "the network is down"
        ));
        assert!(matches!(state.activity, PlayerActivity::Paused(None)));
        assert_eq!(state.duration, None);
        assert!(state.size.is_none());
    }

    #[test]
    fn a_time_past_what_a_duration_holds_is_the_longest_it_holds() {
        assert_eq!(nanoseconds(Duration::MAX), u64::MAX);
    }
}
