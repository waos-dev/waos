//! The scope: the wave of what the instrument plays, drawn anew thirty times a second while
//! there is sound, and a still line while there is none.
//!
//! Not at the screen's rate: the program has one thread, and each frame it draws holds that
//! thread for most of a refresh, which is time the sound cannot be made in. At half the rate of
//! a 60 Hz screen every other refresh is left to the sound.
//!
//! The invariant: it draws only while the sound's trace is live. A scope that drew regardless
//! would keep the program drawing for as long as its window shows.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use waft::ui::valo::{Cap, Join, Point};
use waft::ui::{Canvas, Paint, PaintStyle, PathBuilder, Stroke};
use waft::{
    App, BuildContext, Color, CustomPaint, Handle, IntoWidget, KeyRef, Listener, Size, State,
    StateData, StatefulWidget, Subscription, Timer, WidgetRef,
};
use waft_rendering::CustomPainter;
use waft_winui::ThemeResources;

use crate::instrument::Instrument;
use crate::trace::Trace;

/// How many samples the scope shows across its width: a little over 20 ms at 48 kHz, two
/// periods of the lowest C a keyboard starts at.
const SHOWN: usize = 1024;

/// How long the scope waits between two drawings of a live trace.
const DRAWN_EVERY: Duration = Duration::from_millis(33);

/// How much of the scope's half height full scale takes, and the wave's line width.
const FULL_SCALE: f32 = 1.6;
const LINE_WIDTH: f32 = 1.5;

pub(crate) struct ScopeView {
    instrument: Instrument,
}

impl ScopeView {
    pub(crate) fn new(instrument: Instrument) -> ScopeView {
        ScopeView { instrument }
    }
}

impl std::fmt::Debug for ScopeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopeView").finish_non_exhaustive()
    }
}

impl StatefulWidget for ScopeView {
    type State = ScopeViewState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> ScopeViewState {
        ScopeViewState {
            state: StateData::new(),
            next_drawing: None,
            keys_watched: None,
        }
    }
}

pub(crate) struct ScopeViewState {
    state: StateData<ScopeView>,
    /// What draws the trace again in a moment, while it is live.
    next_drawing: Option<Timer>,
    /// Hears each change of the keys held: a key pressed makes the trace live.
    keys_watched: Option<Subscription>,
}

impl ScopeViewState {
    fn trace(self: Handle<Self>, app: &App) -> Option<Rc<RefCell<Trace>>> {
        self.widget(app).instrument.trace().cloned()
    }

    fn is_live(self: Handle<Self>, app: &App) -> bool {
        self.trace(app)
            .is_some_and(|trace| trace.borrow().is_live())
    }

    /// Starts drawing a trace gone live, unless it is being drawn already.
    fn draw_while_live(self: Handle<Self>, app: &mut App) {
        if self.is_live(app) && app.get(self).next_drawing.is_none() {
            self.draw(app);
        }
    }

    /// Draws the trace as it is now, and again in a moment while it is live: once more after
    /// the sound has died away, which leaves the still line.
    fn draw(self: Handle<Self>, app: &mut App) {
        let next = self
            .is_live(app)
            .then(|| Timer::new(app, DRAWN_EVERY, Listener::new(move |app| self.draw(app))));
        self.set_state(app, |state| state.next_drawing = next);
    }
}

impl State for ScopeViewState {
    type Widget = ScopeView;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let playing = self.widget(app).instrument.playing().clone();
        let watched = app.observe(&playing, move |app| self.draw_while_live(app));
        app.get_mut(self).keys_watched = Some(watched);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        let state = app.get_mut(self);
        state.keys_watched = None;
        if let Some(next_drawing) = state.next_drawing.take() {
            next_drawing.cancel(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let samples = match self.trace(app) {
            Some(trace) => trace.borrow().steady(SHOWN),
            None => vec![0.0; SHOWN],
        };
        CustomPaint::new()
            .painter(Wave {
                samples,
                color: resources.accent_fill_color_default_brush,
            })
            .into_widget()
    }
}

/// A wave as a line across the scope, silence along its middle.
#[derive(PartialEq)]
struct Wave {
    samples: Vec<f32>,
    color: Color,
}

impl CustomPainter for Wave {
    fn paint(&self, _app: &mut App, canvas: &mut Canvas, size: Size) {
        if size.is_empty() || self.samples.len() < 2 {
            return;
        }
        let (width, middle) = (size.width() as f32, size.height() as f32 / 2.0);
        let step = width / (self.samples.len() - 1) as f32;
        let height_of = |sample: f32| middle - (sample / FULL_SCALE).clamp(-1.0, 1.0) * middle;
        let mut line = PathBuilder::new();
        line.move_to(Point::new(0.0, height_of(self.samples[0])));
        for (index, sample) in self.samples.iter().enumerate().skip(1) {
            line.line_to(Point::new(index as f32 * step, height_of(*sample)));
        }
        let mut paint = Paint::from_color(self.color.into());
        paint.style = PaintStyle::Stroke(Stroke {
            cap: Cap::Round,
            join: Join::Round,
            ..Stroke::new(LINE_WIDTH)
        });
        canvas.draw_path(&line.build(), Default::default(), &paint);
    }

    fn should_repaint(&self, _app: &App, old_delegate: &dyn CustomPainter) -> bool {
        old_delegate.as_any().downcast_ref::<Self>() != Some(self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
