//! Refreshing: fetching each feed's document once and taking what it lists into the library. A
//! refresh is asked for by the user, for every feed or the one listed, or by the app itself: at
//! its launch, as it comes back to the front, and every half hour while it runs, whether or not
//! its window shows. A feed the app itself fetched within the last ten minutes is left alone, so
//! a relaunch, or a glance at the window, does not ask a site twice. Each feed is asked on its own and taken in as it answers; one that fails keeps what the
//! library has of it, and says why above its items.
//!
//! The invariant a later editor will break: a feed is never asked while a request for it is under
//! way, and nothing here asks again by itself sooner than [`REFRESH_EVERY`].

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use waft::{App, AppLifecycleListener, Listener, Timer};

use crate::feed::{Feed, FeedId};
use crate::fetching::{Fetched, NotFetched, fetch_feed};
use crate::http::Validators;
use crate::library::Scope;
use crate::new_item_notices::{Found, tell_of_new_items};
use crate::reader_app::ReaderApp;

/// How often the app refreshes by itself while it runs.
const REFRESH_EVERY: Duration = Duration::from_secs(30 * 60);

/// How recently the app must have fetched a feed to leave it alone when it refreshes by itself,
/// in seconds.
const FRESH_FOR: u64 = 10 * 60;

/// Who asked for a refresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// The user, who is looking at the list.
    ByUser,
    /// The app, as its user comes to it: at its launch, and as it comes back to the front.
    OnComingForward,
    /// The app, on its schedule, perhaps with its window out of sight.
    OnSchedule,
}

/// The refreshes under way, and what went wrong with each feed's last.
#[derive(Debug, Default)]
pub(crate) struct Refreshing {
    under_way: BTreeSet<FeedId>,
    problems: BTreeMap<FeedId, String>,
}

impl Refreshing {
    /// Whether a feed of `scope` is being fetched.
    pub(crate) fn is_under_way(&self, scope: Scope) -> bool {
        self.under_way.iter().any(|id| covers(scope, *id))
    }

    /// Why the feeds of `scope` whose last refresh failed were not fetched, each by its id.
    pub(crate) fn problems(&self, scope: Scope) -> impl Iterator<Item = (FeedId, &str)> {
        let in_scope = self
            .problems
            .iter()
            .filter(move |(id, _)| covers(scope, **id));
        in_scope.map(|(id, why)| (*id, why.as_str()))
    }

    fn forget(&mut self, id: FeedId) {
        self.under_way.remove(&id);
        self.problems.remove(&id);
    }
}

fn covers(scope: Scope, id: FeedId) -> bool {
    scope == Scope::All || scope == Scope::Feed(id)
}

/// One refresh of several feeds: who asked, how many have yet to answer, and what was found.
struct Round {
    asked: Asked,
    awaited: usize,
    found: Vec<Found>,
}

/// A feed to ask: where, and what its server said of its document last time.
struct Ask {
    id: FeedId,
    address: String,
    validators: Validators,
}

/// Refreshes at once, then as the app comes back to the front and every [`REFRESH_EVERY`], for
/// as long as it runs.
pub(crate) fn refresh_regularly(app: &mut App, reader: &ReaderApp) {
    refresh(app, reader, Scope::All, Asked::OnComingForward);
    refresh_later(app, reader);
    let coming_forward = reader.clone();
    AppLifecycleListener::new(app).on_resume(
        app,
        Rc::new(move |app| refresh(app, &coming_forward, Scope::All, Asked::OnComingForward)),
    );
}

fn refresh_later(app: &mut App, reader: &ReaderApp) {
    let reader = reader.clone();
    Timer::new(
        app,
        REFRESH_EVERY,
        Listener::new(move |app: &mut App| {
            refresh(app, &reader, Scope::All, Asked::OnSchedule);
            refresh_later(app, &reader);
        }),
    );
}

/// Fetches the feeds of `scope` that no request is under way for, and takes each in as it answers.
pub(crate) fn refresh(app: &mut App, reader: &ReaderApp, scope: Scope, asked: Asked) {
    let asks = feeds_to_ask(app, reader, scope, asked);
    if asks.is_empty() {
        return;
    }
    reader.refreshing.update(app, |refreshing, cx| {
        refreshing.under_way.extend(asks.iter().map(|ask| ask.id));
        cx.notify();
    });
    let round = Rc::new(RefCell::new(Round {
        asked,
        awaited: asks.len(),
        found: Vec::new(),
    }));
    for ask in asks {
        let (reader, round) = (reader.clone(), Rc::clone(&round));
        app.spawn(async move |cx| {
            let answer = fetch_feed(&ask.address, &ask.validators).await;
            cx.update(|app| answered(app, &reader, ask, answer, &round));
        });
    }
}

/// The feed `id` is followed no more: nothing is said of its refreshes any longer.
pub(crate) fn forget_feed(app: &mut App, reader: &ReaderApp, id: FeedId) {
    reader.refreshing.update(app, |refreshing, cx| {
        refreshing.forget(id);
        cx.notify();
    });
}

/// The feeds of `scope` to ask now: those not being asked already and, when the app asks by
/// itself, not fetched within [`FRESH_FOR`].
fn feeds_to_ask(app: &App, reader: &ReaderApp, scope: Scope, asked: Asked) -> Vec<Ask> {
    let now = seconds_now();
    let under_way = &reader.refreshing.read(app).under_way;
    let is_due = |feed: &&Feed| {
        let fresh = feed
            .refreshed
            .is_some_and(|at| now.saturating_sub(at) < FRESH_FOR);
        covers(scope, feed.id)
            && !under_way.contains(&feed.id)
            && !(fresh && asked != Asked::ByUser)
    };
    let library = reader.library.read(app);
    let due = library.feeds().filter(is_due);
    due.map(|feed| Ask {
        id: feed.id,
        address: feed.address.clone(),
        validators: feed.validators.clone(),
    })
    .collect()
}

/// Takes in what the feed asked answered, and ends its round where it was the last awaited.
fn answered(
    app: &mut App,
    reader: &ReaderApp,
    ask: Ask,
    answer: Result<Fetched, NotFetched>,
    round: &Rc<RefCell<Round>>,
) {
    let id = ask.id;
    let (found, problem) = match answer {
        Ok(fetched) => (take_in(app, reader, ask, fetched), None),
        Err(NotFetched(why)) => (None, Some(why)),
    };
    reader.refreshing.update(app, |refreshing, cx| {
        refreshing.forget(id);
        if let Some(why) = problem {
            refreshing.problems.insert(id, why);
        }
        cx.notify();
    });
    if round_ends_with(round, found) {
        round_ended(app, reader, &round.borrow());
    }
}

/// Notes in `round` what one of its feeds found; answers whether it was the last awaited.
fn round_ends_with(round: &Rc<RefCell<Round>>, found: Option<Found>) -> bool {
    let mut round = round.borrow_mut();
    let news = found.filter(|found| found.count > 0 && !found.first_fetch);
    round.found.extend(news);
    round.awaited -= 1;
    round.awaited == 0
}

/// Takes what `fetched` lists into the library; answers what was found that it did not have.
fn take_in(app: &mut App, reader: &ReaderApp, ask: Ask, fetched: Fetched) -> Option<Found> {
    let now = seconds_now();
    reader.change_library(app, |library| {
        let first_fetch = library.feed(ask.id)?.refreshed.is_none();
        let count = match fetched {
            Fetched::Unchanged => {
                library.refreshed(ask.id, ask.validators, None, now);
                0
            }
            Fetched::Feed(document) => {
                let count = library.merge(ask.id, document.feed, now);
                library.refreshed(ask.id, document.validators, document.moved_to, now);
                count
            }
        };
        Some(Found {
            feed: ask.id,
            title: library.feed(ask.id)?.title.clone(),
            count,
            first_fetch,
        })
    })
}

/// Every feed of the round has answered: a round the app made on its schedule tells its user of
/// what it found.
fn round_ended(app: &mut App, reader: &ReaderApp, round: &Round) {
    if round.asked == Asked::OnSchedule && !round.found.is_empty() {
        tell_of_new_items(app, reader, &round.found);
    }
}

pub(crate) fn seconds_now() -> u64 {
    let since_1970 = SystemTime::now().duration_since(UNIX_EPOCH);
    since_1970.map_or(0, |duration| duration.as_secs())
}
