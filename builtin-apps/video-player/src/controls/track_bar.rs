//! A bar the user clicks or drags along to pick a fraction of it, as the seek bar is: the part up
//! to its value filled, with a knob there, over lighter spans such as what is loaded. Drawn here,
//! not Fluent's `Slider`, whose default style shows no spans and which tells a click from a drag
//! by nothing, where a click seeks exactly and a drag wherever the system lands soonest.

use std::fmt;

use waft_embedder::Color;
use waft_foundation::{App, Listener, ValueChanged};
use waft_rendering::HitTestBehavior;
use waft_widgets::{
    BorderRadius, BoxDecoration, BoxShape, BuildContext, DecoratedBox, GestureDetector, IntoWidget,
    LayoutBuilder, Positioned, SizedBox, Stack, StatelessWidget, WidgetRef,
};

/// How tall the bar's track is, and the room around it the pointer may hit.
const TRACK_HEIGHT: f64 = 4.0;
const HEIGHT: f64 = 20.0;
const KNOB: f64 = 12.0;

const TRACK: Color = Color::from_argb(70, 255, 255, 255);
const SPAN: Color = Color::from_argb(130, 255, 255, 255);
const FILLED: Color = Color::from_argb(255, 255, 255, 255);

/// A bar at `value`, a fraction of it, over `spans`, each a start and an end as fractions.
pub(crate) struct TrackBar {
    /// Where it stands; none draws neither the filled part nor the knob.
    value: Option<f64>,
    spans: Vec<(f64, f64)>,
    /// What a click at a fraction asks.
    on_click: Option<ValueChanged<f64>>,
    /// What each move of a drag asks, at the fraction under the pointer.
    on_drag: Option<ValueChanged<f64>>,
    on_drag_end: Option<Listener>,
}

impl TrackBar {
    pub(crate) fn new(value: Option<f64>) -> TrackBar {
        TrackBar {
            value,
            spans: Vec::new(),
            on_click: None,
            on_drag: None,
            on_drag_end: None,
        }
    }

    pub(crate) fn spans(mut self, spans: Vec<(f64, f64)>) -> TrackBar {
        self.spans = spans;
        self
    }

    pub(crate) fn on_click(mut self, on_click: ValueChanged<f64>) -> TrackBar {
        self.on_click = Some(on_click);
        self
    }

    pub(crate) fn on_drag(mut self, on_drag: ValueChanged<f64>) -> TrackBar {
        self.on_drag = Some(on_drag);
        self
    }

    pub(crate) fn on_drag_end(mut self, on_drag_end: Listener) -> TrackBar {
        self.on_drag_end = Some(on_drag_end);
        self
    }
}

impl fmt::Debug for TrackBar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrackBar")
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl StatelessWidget for TrackBar {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let (value, spans) = (self.value, self.spans.clone());
        let (on_click, on_drag, on_drag_end) = (
            self.on_click.clone(),
            self.on_drag.clone(),
            self.on_drag_end.clone(),
        );
        LayoutBuilder::new(move |_app, _context, constraints| {
            let width = constraints.max_width;
            let bar = drawn(width, value, &spans);
            pointed(
                width,
                bar,
                on_click.clone(),
                on_drag.clone(),
                on_drag_end.clone(),
            )
        })
        .into_widget()
    }
}

/// The bar, `width` wide: its track, its spans, and its filled part and knob at `value`.
fn drawn(width: f64, value: Option<f64>, spans: &[(f64, f64)]) -> WidgetRef {
    let mut layers = vec![along(0.0, width, TRACK)];
    layers.extend(
        spans
            .iter()
            .map(|(start, end)| along(start * width, (end - start) * width, SPAN)),
    );
    if let Some(value) = value {
        let at = value.clamp(0.0, 1.0) * width;
        layers.push(along(0.0, at, FILLED));
        layers.push(knob(at));
    }
    SizedBox::new()
        .width(width)
        .height(HEIGHT)
        .child(Stack::new().children(layers))
        .into_widget()
}

/// A stretch of the track from `left`, `width` long, in `color`.
fn along(left: f64, width: f64, color: Color) -> WidgetRef {
    let stretch = DecoratedBox::new(
        BoxDecoration::new()
            .color(color)
            .border_radius(BorderRadius::circular(TRACK_HEIGHT / 2.0)),
    );
    Positioned::new(stretch)
        .left(left)
        .top((HEIGHT - TRACK_HEIGHT) / 2.0)
        .width(width.max(0.0))
        .height(TRACK_HEIGHT)
        .into_widget()
}

/// The knob, centred at `at` along the bar.
fn knob(at: f64) -> WidgetRef {
    let circle = DecoratedBox::new(BoxDecoration::new().color(FILLED).shape(BoxShape::Circle));
    Positioned::new(circle)
        .left(at - KNOB / 2.0)
        .top((HEIGHT - KNOB) / 2.0)
        .width(KNOB)
        .height(KNOB)
        .into_widget()
}

/// `bar`, `width` wide, turning clicks and drags along it into fractions of it.
fn pointed(
    width: f64,
    bar: WidgetRef,
    on_click: Option<ValueChanged<f64>>,
    on_drag: Option<ValueChanged<f64>>,
    on_drag_end: Option<Listener>,
) -> WidgetRef {
    let fraction = move |dx: f64| {
        if width > 0.0 {
            (dx / width).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let mut detector = GestureDetector::new().behavior(HitTestBehavior::Opaque);
    if let Some(on_click) = on_click {
        detector = detector.on_tap_up(std::rc::Rc::new(move |app: &mut App, details| {
            on_click(app, fraction(details.local_position.dx()));
        }));
    }
    if let Some(on_drag) = on_drag {
        let started = on_drag.clone();
        detector = detector
            .on_horizontal_drag_start(std::rc::Rc::new(move |app: &mut App, details| {
                started(app, fraction(details.local_position.dx()));
            }))
            .on_horizontal_drag_update(std::rc::Rc::new(move |app: &mut App, details| {
                on_drag(app, fraction(details.local_position.dx()));
            }));
    }
    if let Some(on_drag_end) = on_drag_end {
        detector =
            detector.on_horizontal_drag_end(std::rc::Rc::new(move |app: &mut App, _details| {
                on_drag_end.call(app);
            }));
    }
    detector.child(bar).into_widget()
}
