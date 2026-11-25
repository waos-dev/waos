//! An icon's motion on the home: it slides to its new place where the icons around it change,
//! as an app is installed or removed or a search narrows them, and one that was not there fades
//! in, as a native grid moves its items (`UICollectionView`'s updates). WinUI's timing: 300 ms
//! on its decelerating curve.
//!
//! The icon is told its place among its section's rows and the size of a cell; its state, kept
//! under its app's key wherever its row is, starts the slide the moment its place changes, from
//! where the icon is drawn at that moment, so a place changing again mid-slide goes on from there.
//!
//! The invariant: at rest the icon is drawn exactly in its place, under no transform.

use std::time::Duration;

use waft::{
    App, BuildContext, Cubic, Curve, FrameCallback, Handle, IntoWidget, KeyRef, MediaQuery, Offset,
    Opacity, OptionRetained, Ref, SingleTickerProviderStateMixin,
    SingleTickerProviderStateMixinData, State, StateData, StatefulWidget, Ticker, Transform,
    WidgetRef,
};

use crate::logic::icon_rows::IconPlace;

/// How long an icon takes to reach its place, and to fade in.
const DURATION: Duration = Duration::from_millis(300);

/// WinUI's decelerating curve, which its own repositioning uses.
const CURVE: Cubic = Cubic::new(0.1, 0.9, 0.2, 1.0);

/// `child`, an icon of the home, drawn sliding to `place` when that changes.
#[derive(Clone, Debug)]
pub(crate) struct SlidesIntoPlace {
    /// What keeps the icon's state as it moves between rows: its app's key.
    pub(crate) key: KeyRef,
    /// Its place among its section's rows.
    pub(crate) place: IconPlace,
    /// How wide a cell of the grid is.
    pub(crate) cell_width: f64,
    /// The room between two rows.
    pub(crate) row_gap: f64,
    /// Whether it fades in as it is first built: an icon that was not on the home before.
    pub(crate) appears: bool,
    pub(crate) child: WidgetRef,
}

impl StatefulWidget for SlidesIntoPlace {
    type State = SlidesIntoPlaceState;

    fn key(&self) -> Option<&KeyRef> {
        Some(&self.key)
    }

    fn create_state(&self) -> SlidesIntoPlaceState {
        SlidesIntoPlaceState {
            state: StateData::new(),
            single_ticker_provider: SingleTickerProviderStateMixinData::default(),
            ticker: None,
            elapsed: DURATION,
            from: Offset::ZERO,
            appearing: false,
        }
    }
}

pub(crate) struct SlidesIntoPlaceState {
    state: StateData<SlidesIntoPlace>,
    single_ticker_provider: SingleTickerProviderStateMixinData,
    /// The clock of the motion, running while the icon moves or fades in.
    ticker: Option<Ref<Ticker>>,
    /// How long the motion has run; [`DURATION`] at rest.
    elapsed: Duration,
    /// How far from its place the icon was drawn as the motion began.
    from: Offset,
    /// Whether the motion is the icon's fading in.
    appearing: bool,
}

impl SingleTickerProviderStateMixin for SlidesIntoPlaceState {
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

impl SlidesIntoPlaceState {
    /// How much of the motion is left to go, from 1 as it begins to 0 at rest, eased.
    fn left_to_go(self: Handle<Self>, app: &App) -> f64 {
        let run = app.get(self).elapsed.as_secs_f64() / DURATION.as_secs_f64();
        1.0 - CURVE.transform(run.clamp(0.0, 1.0))
    }

    /// How far from its place the icon is drawn now.
    fn drawn_from_its_place(self: Handle<Self>, app: &App) -> Offset {
        app.get(self).from * self.left_to_go(app)
    }

    /// Begins the motion from `from` away, fading in where `appearing`. Called as the icon is
    /// first built or its widget changes, each followed by a build, so it asks for none.
    fn begin(self: Handle<Self>, app: &mut App, from: Offset, appearing: bool) {
        let ticker = app.get(self).ticker.clone().expect("made in init_state");
        ticker.stop(app, false);
        let state = app.get_mut(self);
        state.from = from;
        state.appearing = appearing;
        state.elapsed = Duration::ZERO;
        ticker.start(app);
    }

    fn tick(self: Handle<Self>, app: &mut App, elapsed: Duration) {
        self.set_state(app, |state| state.elapsed = elapsed.min(DURATION));
        if elapsed >= DURATION {
            let ticker = app.get(self).ticker.clone().expect("made in init_state");
            ticker.stop(app, false);
        }
    }

    /// How tall the icon is, as last laid out: with the room between rows, how far apart two
    /// rows are.
    fn height(self: Handle<Self>, app: &App) -> f64 {
        self.context(app)
            .find_render_object(app)
            .and_then(|render| render.as_box())
            .map_or(0.0, |icon| icon.size(app).height())
    }
}

impl State for SlidesIntoPlaceState {
    type Widget = SlidesIntoPlace;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let ticker = SingleTickerProviderStateMixin::create_ticker(
            self,
            app,
            FrameCallback::new(move |app, elapsed| self.tick(app, elapsed)),
        );
        app.get_mut(self).ticker = Some(ticker);
        if self.widget(app).appears {
            self.begin(app, Offset::ZERO, true);
        }
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, old: &SlidesIntoPlace) {
        let widget = self.widget(app);
        let (place, cell_width, row_gap) = (widget.place, widget.cell_width, widget.row_gap);
        // A grid laid out again at another width moves every icon at once: no icon changed
        // its place among the others.
        if old.place == place || old.cell_width != cell_width {
            return;
        }
        if MediaQuery::disable_animations_of(app, self.context(app)) {
            return;
        }
        let columns = old.place.column as f64 - place.column as f64;
        let rows = old.place.row as f64 - place.row as f64;
        let moved = Offset::new(columns * cell_width, rows * (self.height(app) + row_gap));
        let from = self.drawn_from_its_place(app) + moved;
        let appearing = app.get(self).appearing && self.left_to_go(app) > 0.0;
        self.begin(app, from, appearing);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(ticker) = app.get(self).ticker.handle() {
            ticker.dispose(app);
        }
        SingleTickerProviderStateMixin::dispose(self, app);
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let child = self.widget(app).child.clone();
        let left_to_go = self.left_to_go(app);
        if left_to_go == 0.0 {
            return child;
        }
        let moved = Transform::translate(self.drawn_from_its_place(app)).child(child);
        if app.get(self).appearing {
            Opacity::new(1.0 - left_to_go).child(moved).into_widget()
        } else {
            moved.into_widget()
        }
    }
}
