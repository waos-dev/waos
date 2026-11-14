//! What the user asks of what plays, alike from the menus, the keys and the controls: each a
//! function of the session's player, and the commands of the Playback menu with their keys.

use std::time::Duration;

use waft_embedder::{PlayerActivity, PlayerState, SeekTolerance, TimeRange};
use waft_foundation::{App, Handle};
use waft_services::LogicalKeyboardKey;
use waft_widgets::{Player, SingleActivator};

/// How far a skip moves.
const SKIP: Duration = Duration::from_secs(10);

/// The rates the player steps through, slowest first.
pub(crate) const RATES: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// A command of the Playback menu, which its key runs as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackCommand {
    PlayPause,
    Forward,
    Back,
    Faster,
    Slower,
    Mute,
}

impl PlaybackCommand {
    pub(crate) const ALL: [PlaybackCommand; 6] = [
        PlaybackCommand::PlayPause,
        PlaybackCommand::Forward,
        PlaybackCommand::Back,
        PlaybackCommand::Faster,
        PlaybackCommand::Slower,
        PlaybackCommand::Mute,
    ];

    /// The key that runs it: Space, the arrows, ⌘] and ⌘[, and ⌥⌘↓, as QuickTime's and VLC's.
    pub(crate) fn key(self) -> SingleActivator {
        match self {
            PlaybackCommand::PlayPause => SingleActivator::new(LogicalKeyboardKey::SPACE),
            PlaybackCommand::Forward => SingleActivator::new(LogicalKeyboardKey::ARROW_RIGHT),
            PlaybackCommand::Back => SingleActivator::new(LogicalKeyboardKey::ARROW_LEFT),
            PlaybackCommand::Faster => {
                SingleActivator::new(LogicalKeyboardKey::BRACKET_RIGHT).meta(true)
            }
            PlaybackCommand::Slower => {
                SingleActivator::new(LogicalKeyboardKey::BRACKET_LEFT).meta(true)
            }
            PlaybackCommand::Mute => SingleActivator::new(LogicalKeyboardKey::ARROW_DOWN)
                .meta(true)
                .alt(true),
        }
    }

    pub(crate) fn run(self, app: &mut App, player: Handle<Player>) {
        match self {
            PlaybackCommand::PlayPause => play_or_pause(app, player),
            PlaybackCommand::Forward => skip(app, player, Skip::Forward),
            PlaybackCommand::Back => skip(app, player, Skip::Back),
            PlaybackCommand::Faster => step_rate(app, player, Step::Faster),
            PlaybackCommand::Slower => step_rate(app, player, Step::Slower),
            PlaybackCommand::Mute => toggle_mute(app, player),
        }
    }
}

/// Which way a skip moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skip {
    Forward,
    Back,
}

/// Which way the rate steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Faster,
    Slower,
}

/// Whether the player is paused, by the program or the system, rather than playing or waiting
/// to.
pub(crate) fn is_paused(state: &PlayerState) -> bool {
    matches!(state.activity, PlayerActivity::Paused(_))
}

/// Plays `player` while it is paused, from its start once it ended; pauses it while it plays or
/// waits to.
pub(crate) fn play_or_pause(app: &mut App, player: Handle<Player>) {
    if is_paused(player.state(app)) {
        player.play(app);
    } else {
        player.pause(app);
    }
}

/// Moves `player` ten seconds forward or back, as far as it can seek.
pub(crate) fn skip(app: &mut App, player: Handle<Player>, skip: Skip) {
    let state = player.state(app);
    let from = state.seeking.unwrap_or_else(|| player.position(app));
    if let Some(to) = skipped(from, skip, &state.seekable) {
        player.seek_to(app, to, SeekTolerance::EXACT);
    }
}

/// Plays `player` a step faster or slower among [`RATES`].
pub(crate) fn step_rate(app: &mut App, player: Handle<Player>, step: Step) {
    if let Some(rate) = stepped_rate(player.state(app).rate, step) {
        player.set_rate(app, rate);
    }
}

pub(crate) fn toggle_mute(app: &mut App, player: Handle<Player>) {
    let muted = player.state(app).muted;
    player.set_muted(app, !muted);
}

/// Where a skip from `from` lands: [`SKIP`] on, kept within the seekable range `from` is in;
/// none where the media cannot seek.
fn skipped(from: Duration, skip: Skip, seekable: &[TimeRange]) -> Option<Duration> {
    let range = seekable
        .iter()
        .find(|range| range.start <= from && from <= range.end)
        .or_else(|| seekable.last())?;
    let to = match skip {
        Skip::Forward => from.saturating_add(SKIP),
        Skip::Back => from.saturating_sub(SKIP),
    };
    Some(to.clamp(range.start, range.end))
}

/// The rate a step from `rate` reaches; none past the fastest or the slowest.
pub(crate) fn stepped_rate(rate: f64, step: Step) -> Option<f64> {
    match step {
        Step::Faster => RATES
            .into_iter()
            .find(|faster| *faster > rate + f64::EPSILON),
        Step::Slower => RATES
            .into_iter()
            .rfind(|slower| *slower < rate - f64::EPSILON),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: u64, end: u64) -> TimeRange {
        TimeRange {
            start: Duration::from_secs(start),
            end: Duration::from_secs(end),
        }
    }

    #[test]
    fn a_skip_moves_ten_seconds_within_where_the_media_seeks() {
        let seekable = [range(0, 100)];
        let at = |seconds| Some(Duration::from_secs(seconds));
        assert_eq!(
            skipped(Duration::from_secs(30), Skip::Forward, &seekable),
            at(40)
        );
        assert_eq!(
            skipped(Duration::from_secs(30), Skip::Back, &seekable),
            at(20)
        );
        assert_eq!(
            skipped(Duration::from_secs(95), Skip::Forward, &seekable),
            at(100)
        );
        assert_eq!(
            skipped(Duration::from_secs(4), Skip::Back, &seekable),
            at(0)
        );
    }

    #[test]
    fn a_skip_keeps_within_a_live_window_and_moves_nothing_where_nothing_seeks() {
        let live = [range(500, 560)];
        assert_eq!(
            skipped(Duration::from_secs(505), Skip::Back, &live),
            Some(Duration::from_secs(500))
        );
        assert_eq!(skipped(Duration::from_secs(5), Skip::Forward, &[]), None);
    }

    #[test]
    fn the_rate_steps_among_the_rates_and_stops_at_either_end() {
        assert_eq!(stepped_rate(1.0, Step::Faster), Some(1.25));
        assert_eq!(stepped_rate(1.0, Step::Slower), Some(0.75));
        assert_eq!(stepped_rate(2.0, Step::Faster), None);
        assert_eq!(stepped_rate(0.5, Step::Slower), None);
        assert_eq!(
            stepped_rate(1.1, Step::Slower),
            Some(1.0),
            "a rate the system set itself steps to the nearest"
        );
    }
}
