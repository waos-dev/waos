//! What the window plays: one session of a source in its player, or none, beside the sources
//! opened lately. Opening a source ends the session before it, keeping where that one was left,
//! and starts the new one where it was left the last time.
//!
//! The invariant: a session's player is disposed as the session ends, and nothing reads it
//! after: whatever is built of a session is keyed by its number, so nothing built for one that
//! ended is built again.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use waft_embedder::{PauseReason, PlayerActivity, PlayerState, PlayerStatus, SeekTolerance};
use waft_foundation::{App, Entity, Handle, Listenable, Listener, WeakEntity};
use waft_widgets::Player;

use crate::recents::{self, Recents};
use crate::source::Source;

/// How near its end media is left as ended, to start from the beginning next time.
const NEAR_THE_END: Duration = Duration::from_secs(5);

/// What plays, and what played lately.
pub(crate) struct Playback {
    session: Option<Session>,
    recents: Recents,
    /// How many sessions this run has started, which numbers the next.
    started: u64,
}

/// One source, in the player it plays in.
#[derive(Clone)]
pub(crate) struct Session {
    /// Names the session among this run's.
    pub(crate) number: u64,
    pub(crate) source: Source,
    pub(crate) player: Handle<Player>,
}

impl Playback {
    /// Nothing playing, and the sources the last run opened.
    pub(crate) fn new(app: &mut App) -> Entity<Playback> {
        app.new_entity(|_cx| Playback {
            session: None,
            recents: recents::load(),
            started: 0,
        })
    }

    pub(crate) fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub(crate) fn recents(&self) -> &Recents {
        &self.recents
    }
}

/// The player of what plays, if anything does.
pub(crate) fn current_player(playback: &Entity<Playback>, app: &App) -> Option<Handle<Player>> {
    playback.read(app).session().map(|session| session.player)
}

/// Plays `source` in place of what played, from where it was left the last time. Where the
/// system has no players nothing opens, which the screen says.
pub(crate) fn open(playback: &Entity<Playback>, app: &mut App, source: Source) {
    close(playback, app);
    let Some(player) = Player::new(app, source.url()) else {
        return;
    };
    if let Some(left_at) = playback.read(app).recents.left_at(&source) {
        resume_at(app, player, left_at);
    }
    keep_where_it_pauses(app, player, playback.downgrade());
    player.play(app);
    playback.update(app, |playback, cx| {
        playback.recents.opened(&source);
        recents::save(&playback.recents);
        playback.started += 1;
        playback.session = Some(Session {
            number: playback.started,
            source,
            player,
        });
        cx.notify();
    });
}

/// Ends what plays, keeping where it was left.
pub(crate) fn close(playback: &Entity<Playback>, app: &mut App) {
    keep_where_it_is(playback, app);
    let ended = playback.update(app, |playback, cx| {
        cx.notify();
        playback.session.take()
    });
    if let Some(ended) = ended {
        ended.player.dispose(app);
    }
}

/// Keeps where what plays is now, for the next time its source opens.
pub(crate) fn keep_where_it_is(playback: &Entity<Playback>, app: &mut App) {
    let Some(session) = playback.read(app).session.clone() else {
        return;
    };
    let player = session.player;
    let Some(left_at) = resume_point(player.state(app), player.position(app)) else {
        return;
    };
    playback.update(app, |playback, cx| {
        playback.recents.left(&session.source, left_at);
        recents::save(&playback.recents);
        cx.notify();
    });
}

/// Forgets the sources opened lately.
pub(crate) fn clear_recents(playback: &Entity<Playback>, app: &mut App) {
    playback.update(app, |playback, cx| {
        playback.recents.clear();
        recents::save(&playback.recents);
        cx.notify();
    });
}

/// Where media in `state` at `position` opens next time: there, or at its start once it ended,
/// is near its end, or is live; none while it is not ready, which leaves what was kept.
fn resume_point(state: &PlayerState, position: Duration) -> Option<Duration> {
    if state.status != PlayerStatus::Ready {
        return None;
    }
    let ended = state.activity == PlayerActivity::Paused(Some(PauseReason::End));
    let near_the_end = state
        .duration
        .is_none_or(|duration| position + NEAR_THE_END >= duration);
    Some(if ended || near_the_end {
        Duration::ZERO
    } else {
        position
    })
}

/// Seeks `player` to `at` once it can: when it is ready and `at` is in one of its seekable
/// ranges, since a seek outside them moves nothing.
fn resume_at(app: &mut App, player: Handle<Player>, at: Duration) {
    if at.is_zero() {
        return;
    }
    let resumed = Cell::new(false);
    let resume = Listener::new(move |app| {
        let state = player.state(app);
        let seekable = state
            .seekable
            .iter()
            .any(|range| range.start <= at && at < range.end);
        if resumed.get() || state.status != PlayerStatus::Ready || !seekable {
            return;
        }
        resumed.set(true);
        player.seek_to(app, at, SeekTolerance::EXACT);
    });
    listen(app, player, resume);
}

/// Keeps where `player` stands each time it comes to pause, whoever paused it.
fn keep_where_it_pauses(app: &mut App, player: Handle<Player>, playback: WeakEntity<Playback>) {
    let was_paused = Cell::new(true);
    let keep = Listener::new(move |app| {
        let paused = matches!(player.state(app).activity, PlayerActivity::Paused(_));
        if paused
            && !was_paused.get()
            && let Some(playback) = playback.upgrade()
        {
            keep_where_it_is(&playback, app);
        }
        was_paused.set(paused);
    });
    listen(app, player, keep);
}

/// Has `listener` hear each change of `player`, until it is disposed.
fn listen(app: &mut App, player: Handle<Player>, listener: Listener) {
    let listenable: Rc<dyn Listenable> = Rc::new(player);
    listenable.add_listener(app, listener);
}

#[cfg(test)]
mod tests {
    use waft_embedder::PlayerError;

    use super::*;

    fn ready(activity: PlayerActivity, duration: Option<Duration>) -> PlayerState {
        PlayerState {
            status: PlayerStatus::Ready,
            activity,
            duration,
            ..PlayerState::loading()
        }
    }

    #[test]
    fn media_opens_next_time_where_it_was_left() {
        let state = ready(PlayerActivity::Playing, Some(Duration::from_secs(600)));
        assert_eq!(
            resume_point(&state, Duration::from_secs(42)),
            Some(Duration::from_secs(42))
        );
    }

    #[test]
    fn media_that_ended_or_nearly_did_or_is_live_opens_at_its_start() {
        let minute = Some(Duration::from_secs(60));
        let ended = ready(PlayerActivity::Paused(Some(PauseReason::End)), minute);
        assert_eq!(
            resume_point(&ended, Duration::from_secs(60)),
            Some(Duration::ZERO)
        );
        let playing = ready(PlayerActivity::Playing, minute);
        assert_eq!(
            resume_point(&playing, Duration::from_secs(57)),
            Some(Duration::ZERO)
        );
        let live = ready(PlayerActivity::Playing, None);
        assert_eq!(
            resume_point(&live, Duration::from_secs(10)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn media_that_is_not_ready_keeps_where_it_was_left_before() {
        assert_eq!(resume_point(&PlayerState::loading(), Duration::ZERO), None);
        let failed = PlayerState::failed(PlayerError {
            kind: waft_embedder::PlayerErrorKind::Network,
            message: "offline".to_owned(),
        });
        assert_eq!(resume_point(&failed, Duration::ZERO), None);
    }
}
