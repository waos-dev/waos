//! The app's one page, on a `FluentPage`, clear of a phone's bars: the preset, the sustain switch and the octave above the scope, the tone's sliders under it,
//! and the keyboard along the bottom. At a phone's
//! width the sliders stand one under another and the keyboard is lower.

use waft::{
    App, Border, BorderRadius, BorderStyle, BoxDecoration, BuildContext, Column,
    CrossAxisAlignment, DecoratedBox, EdgeInsetsGeometry, Expanded, IntoWidget, LayoutBuilder,
    Padding, Row, SizedBox, Spacer, StatelessWidget, WidgetRef,
};
use waft_winui::{Brush, InfoBar, InfoBarSeverity, ThemeResources};

use crate::controls_view::{OctaveButtons, PresetList, SustainSwitch, ToneSliders};
use crate::instrument::Instrument;
use crate::keyboard_view::KeyboardView;
use crate::scope_view::ScopeView;

/// The widest page laid out as a phone's.
const WIDEST_NARROW: f64 = 640.0;

/// The room around the page and between its parts, and the keyboard's height, wide and narrow.
const MARGIN: (f64, f64) = (20.0, 12.0);
const GAP: f64 = 16.0;
const KEYBOARD_HEIGHT: (f64, f64) = (220.0, 160.0);

/// How wide the list of presets is where there is room for it.
const PRESETS_WIDTH: f64 = 200.0;

/// The scope's card: how round its corners are, and the room inside it.
const CARD_CORNER: f64 = 8.0;
const CARD_PADDING: f64 = 12.0;

pub(crate) struct Page {
    instrument: Instrument,
}

impl Page {
    pub(crate) fn new(instrument: Instrument) -> Page {
        Page { instrument }
    }
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Page").finish_non_exhaustive()
    }
}

impl StatelessWidget for Page {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let background = resources.solid_background_fill_color_base;
        let instrument = self.instrument.clone();
        let parts = LayoutBuilder::new(move |_app, _context, constraints| {
            let narrow = constraints.max_width < WIDEST_NARROW;
            laid_out(&instrument, &resources, narrow)
        });
        waft_winui::FluentPage::new()
            .background(Brush::Solid(background))
            .content(parts.into_widget())
            .into_widget()
    }
}

/// The page's parts, top to bottom, `narrow` as on a phone or not.
fn laid_out(instrument: &Instrument, resources: &ThemeResources, narrow: bool) -> WidgetRef {
    let pick = |sizes: (f64, f64)| if narrow { sizes.1 } else { sizes.0 };
    let gap = || SizedBox::new().height(GAP).into_widget();
    let top = top_row(instrument, narrow);
    let keyboard = SizedBox::new()
        .height(pick(KEYBOARD_HEIGHT))
        .child(KeyboardView::new(instrument.clone()));
    let mut parts = Vec::new();
    if !instrument.has_sound() {
        parts.extend([no_sound(), gap()]);
    }
    parts.extend([
        top,
        gap(),
        Expanded::new(scope_card(instrument, resources)).into_widget(),
        gap(),
        ToneSliders::new(instrument.clone(), narrow).into_widget(),
        gap(),
        keyboard.into_widget(),
    ]);
    let column = Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(parts);
    Padding::new(EdgeInsetsGeometry::all(pick(MARGIN)))
        .child(column)
        .into_widget()
}

/// The preset's list, the sustain switch and the octave buttons in a row: the list at its own
/// width with the buttons at the far side, or, `narrow`, taking the room the others leave.
fn top_row(instrument: &Instrument, narrow: bool) -> WidgetRef {
    let presets = PresetList::new(instrument.clone());
    let between = SizedBox::new().width(GAP).into_widget();
    let (presets, room) = if narrow {
        (Expanded::new(presets).into_widget(), between.clone())
    } else {
        let presets = SizedBox::new().width(PRESETS_WIDTH).child(presets);
        (presets.into_widget(), Spacer::new().into_widget())
    };
    Row::new()
        .children([
            presets,
            between,
            SustainSwitch::new(instrument.clone()).into_widget(),
            room,
            OctaveButtons::new(instrument.clone()).into_widget(),
        ])
        .into_widget()
}

/// The scope on a card.
fn scope_card(instrument: &Instrument, resources: &ThemeResources) -> WidgetRef {
    let card = BoxDecoration::new()
        .color(resources.card_background_fill_color_default_brush)
        .border(Border::all(
            resources.card_stroke_color_default_brush,
            1.0,
            BorderStyle::Solid,
            -1.0,
        ))
        .border_radius(BorderRadius::circular(CARD_CORNER));
    let scope = Padding::new(EdgeInsetsGeometry::all(CARD_PADDING))
        .child(ScopeView::new(instrument.clone()));
    DecoratedBox::new(card).child(scope).into_widget()
}

/// What says, where the platform has no sound, that the keys will play nothing.
fn no_sound() -> WidgetRef {
    InfoBar::new(true, |_app, _open| {})
        .severity(InfoBarSeverity::Warning)
        .title("No sound")
        .message("This device gives Synth no sound output, so the keys play nothing.")
        .is_closable(false)
        .into_widget()
}
