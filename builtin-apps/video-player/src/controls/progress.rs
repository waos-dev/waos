//! The time played, the seek bar and the time left. While the player plays they follow its
//! position at each frame, read from the player as Waft's `Player::position` asks of a widget
//! that shows it; a seek under way shows its target, and a drag the place under the pointer.
//! A click on the bar seeks there exactly, and a drag seeks as it goes, wherever the system lands
//! soonest.

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use waft_embedder::{PlayerActivity, PlayerState, SeekTolerance, TimeRange};
use waft_foundation::{App, Handle, ListenableObject, Listener, OptionRetained, Ref};
use waft_scheduler::{FrameCallback, Ticker};
use waft_widgets::{
    Align, AlignmentGeometry, BuildContext, Expanded, IntoWidget, KeyRef, Player, Row,
    SingleTickerProviderStateMixin, SingleTickerProviderStateMixinData, SizedBox, State, StateData,
    StatefulWidget, WidgetRef,
};
use waft_winui::TextBlockStyle;

use super::track_bar::TrackBar;
use crate::clock::clock;
use crate::text::{Tone, line};

/// How wide each time's room is, so the bar does not shift as the digits change.
const TIME_WIDTH: f64 = 56.0;

/// `player`'s time played, seek bar and time left.
pub(crate) struct Progress {
    player: Ref<Player>,
}

impl Progress {
    pub(crate) fn new(player: Ref<Player>) -> Progress {
        Progress { player }
    }
}

impl fmt::Debug for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Progress").finish_non_exhaustive()
    }
}

impl StatefulWidget for Progress {
    type State = ProgressState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> ProgressState {
        ProgressState {
            state: StateData::new(),
            single_ticker_provider: SingleTickerProviderStateMixinData::default(),
            ticker: None,
            dragged_to: None,
        }
    }
}

pub(crate) struct ProgressState {
    state: StateData<Progress>,
    single_ticker_provider: SingleTickerProviderStateMixinData,
    /// Builds the widget again at each frame while the player plays.
    ticker: Option<Ref<Ticker>>,
    /// Where a drag along the bar is, while one is under way.
    dragged_to: Option<Duration>,
}

impl ProgressState {
    fn player(self: Handle<Self>, app: &App) -> Handle<Player> {
        self.widget(app).player.handle()
    }

    fn listener(self: Handle<Self>) -> Listener {
        Listener::handle_method(self, Self::player_changed)
    }

    fn player_changed(self: Handle<Self>, app: &mut App) {
        self.tick_while_playing(app);
        self.set_state(app, |_| {});
    }

    /// Runs the ticker while the player plays, and stops it otherwise.
    fn tick_while_playing(self: Handle<Self>, app: &mut App) {
        let Some(ticker) = app.get(self).ticker.handle() else {
            return;
        };
        let playing = self.player(app).state(app).activity == PlayerActivity::Playing;
        if playing && !ticker.is_active(app) {
            let _ticking = ticker.start(app);
        } else if !playing && ticker.is_active(app) {
            ticker.stop(app, true);
        }
    }

    /// A drag along the bar reached `to`: shown there, and sought to.
    fn drag_to(self: Handle<Self>, app: &mut App, to: Duration) {
        self.set_state(app, |state| state.dragged_to = Some(to));
        self.player(app).seek_to(app, to, SeekTolerance::ANY);
    }

    fn drag_ended(self: Handle<Self>, app: &mut App) {
        self.set_state(app, |state| state.dragged_to = None);
    }
}

impl SingleTickerProviderStateMixin for ProgressState {
    fn single_ticker_provider_data(
        self: Handle<Self>,
        app: &App,
    ) -> &SingleTickerProviderStateMixinData {
        &app.get(self).single_ticker_provider
    }

    fn single_ticker_provider_data_mut(
        self: Handle<Self>,
        app: &mut App,
    ) -> &mut SingleTickerProviderStateMixinData {
        &mut app.get_mut(self).single_ticker_provider
    }
}

impl State for ProgressState {
    type Widget = Progress;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let tick = FrameCallback::new(move |app: &mut App, _elapsed: Duration| {
            self.set_state(app, |_| {});
        });
        let ticker = SingleTickerProviderStateMixin::create_ticker(self, app, tick);
        app.get_mut(self).ticker = Some(ticker);
        self.player(app).add_listener(app, self.listener());
        self.tick_while_playing(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, old_widget: &Progress) {
        if old_widget.player == self.player(app) {
            return;
        }
        old_widget
            .player
            .handle()
            .remove_listener(app, &self.listener());
        self.player(app).add_listener(app, self.listener());
        self.tick_while_playing(app);
    }

    fn activate(self: Handle<Self>, app: &mut App) {
        SingleTickerProviderStateMixin::activate(self, app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        self.player(app).remove_listener(app, &self.listener());
        if let Some(ticker) = app.get_mut(self).ticker.take() {
            ticker.dispose(app);
        }
        SingleTickerProviderStateMixin::dispose(self, app);
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let player = self.player(app);
        let state = player.state(app).clone();
        let at = app
            .get(self)
            .dragged_to
            .or(state.seeking)
            .unwrap_or_else(|| player.position(app));
        let extent = Extent::of(&state);
        Row::new()
            .children([
                time(clock(at), AlignmentGeometry::CENTER_RIGHT),
                Expanded::new(seek_bar(self, extent, at, &state.loaded)).into_widget(),
                time(
                    time_left(extent, &state, at),
                    AlignmentGeometry::CENTER_LEFT,
                ),
            ])
            .into_widget()
    }
}

/// The bar at `at` over what is `loaded`, seeking where it is clicked or dragged; one that
/// stands still where the media has no extent yet.
fn seek_bar(
    progress: Handle<ProgressState>,
    extent: Option<Extent>,
    at: Duration,
    loaded: &[TimeRange],
) -> WidgetRef {
    let Some(extent) = extent else {
        return TrackBar::new(None).into_widget();
    };
    let spans = loaded
        .iter()
        .map(|range| (extent.fraction(range.start), extent.fraction(range.end)))
        .collect();
    TrackBar::new(Some(extent.fraction(at)))
        .spans(spans)
        .on_click(Rc::new(move |app: &mut App, fraction| {
            let player = progress.player(app);
            player.seek_to(app, extent.at(fraction), SeekTolerance::EXACT);
        }))
        .on_drag(Rc::new(move |app: &mut App, fraction| {
            progress.drag_to(app, extent.at(fraction));
        }))
        .on_drag_end(Listener::handle_method(progress, ProgressState::drag_ended))
        .into_widget()
}

/// A time, in its room, set to one side of it.
fn time(text: String, alignment: AlignmentGeometry) -> WidgetRef {
    let label = line(text, TextBlockStyle::Caption, Tone::Primary);
    SizedBox::new()
        .width(TIME_WIDTH)
        .child(Align::new().alignment(alignment).child(label))
        .into_widget()
}

/// What is left from `at`, as `-1:23`; `Live` for a stream without an end.
fn time_left(extent: Option<Extent>, state: &PlayerState, at: Duration) -> String {
    match (state.duration, extent) {
        (Some(duration), _) => format!("-{}", clock(duration.saturating_sub(at))),
        (None, Some(_)) => "Live".to_owned(),
        (None, None) => String::new(),
    }
}

/// The part of the media the seek bar stands for: all of media with a duration, or the window a
/// live stream seeks in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Extent {
    start: Duration,
    end: Duration,
}

impl Extent {
    /// None while the media has neither a duration nor a range to seek in.
    fn of(state: &PlayerState) -> Option<Extent> {
        let extent = match state.duration {
            Some(duration) => Extent {
                start: Duration::ZERO,
                end: duration,
            },
            None => {
                let window = state.seekable.last()?;
                Extent {
                    start: window.start,
                    end: window.end,
                }
            }
        };
        (extent.end > extent.start).then_some(extent)
    }

    /// How far along it `at` is, from 0 to 1.
    fn fraction(self, at: Duration) -> f64 {
        let along = at.saturating_sub(self.start).as_secs_f64();
        (along / (self.end - self.start).as_secs_f64()).clamp(0.0, 1.0)
    }

    /// The time `fraction` of the way along it.
    fn at(self, fraction: f64) -> Duration {
        self.start + (self.end - self.start).mul_f64(fraction.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::PlayerStatus;

    use super::*;

    #[test]
    fn the_bar_stands_for_the_whole_media_or_a_live_window() {
        let file = PlayerState {
            status: PlayerStatus::Ready,
            duration: Some(Duration::from_secs(200)),
            ..PlayerState::loading()
        };
        let extent = Extent::of(&file).unwrap();
        assert_eq!(extent.fraction(Duration::from_secs(50)), 0.25);
        assert_eq!(extent.at(0.5), Duration::from_secs(100));

        let live = PlayerState {
            status: PlayerStatus::Ready,
            seekable: vec![TimeRange {
                start: Duration::from_secs(1000),
                end: Duration::from_secs(1060),
            }],
            ..PlayerState::loading()
        };
        let window = Extent::of(&live).unwrap();
        assert_eq!(window.at(0.5), Duration::from_secs(1030));
        assert_eq!(window.fraction(Duration::from_secs(10)), 0.0);
        assert_eq!(time_left(Some(window), &live, Duration::ZERO), "Live");

        assert_eq!(Extent::of(&PlayerState::loading()), None);
    }

    #[test]
    fn the_time_left_counts_down_to_the_end() {
        let state = PlayerState {
            duration: Some(Duration::from_secs(90)),
            ..PlayerState::loading()
        };
        assert_eq!(
            time_left(Extent::of(&state), &state, Duration::from_secs(30)),
            "-1:00"
        );
    }
}
