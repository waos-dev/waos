//! The keyboard drawn: its keys, the held ones lit, each C named and, where there is room for
//! a computer's keyboard beside the screen, the key that plays each note; and the pointers on
//! it, each finger or the mouse holding the key it is on and sliding from key to key.

use std::rc::Rc;

use waft::{
    Align, AlignmentGeometry, App, Border, BorderRadius, BorderStyle, BoxDecoration, BuildContext,
    Color, Column, DecoratedBox, EdgeInsetsGeometry, IntoWidget, LayoutBuilder, MainAxisSize,
    Offset, Padding, Positioned, Radius, Stack, StatelessWidget, Text, TextStyle, WidgetRef,
};
use waft_gestures::K_PRIMARY_BUTTON;
use waft_rendering::HitTestBehavior;
use waft_winui::{TextBlockStyle, ThemeResources};

use crate::held_keys::Holder;
use crate::instrument::Instrument;
use crate::keyboard_layout::{KeyPlace, KeyboardLayout};
use crate::pitch::Note;
use crate::typing;

/// The keys' colours, a piano's in either theme, and the line between two white keys.
const WHITE_KEY: Color = Color::from_argb(255, 252, 252, 252);
const BLACK_KEY: Color = Color::from_argb(255, 34, 34, 34);
const KEY_EDGE: Color = Color::from_argb(56, 0, 0, 0);

/// What is written on a white key and on a black one.
const ON_WHITE: Color = Color::from_argb(150, 0, 0, 0);
const ON_BLACK: Color = Color::from_argb(170, 255, 255, 255);

/// How round a key's lower corners are, and how far from its lower edge its lettering stands.
const KEY_CORNER: f64 = 4.0;
const LETTERING_INSET: f64 = 8.0;

/// The narrowest keyboard that shows which of the computer's keys play: narrower, the screen is
/// a phone's, which has none.
const NARROWEST_WITH_TYPING: f64 = 560.0;

pub(crate) struct KeyboardView {
    instrument: Instrument,
}

impl KeyboardView {
    pub(crate) fn new(instrument: Instrument) -> KeyboardView {
        KeyboardView { instrument }
    }
}

impl std::fmt::Debug for KeyboardView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyboardView").finish_non_exhaustive()
    }
}

impl StatelessWidget for KeyboardView {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let playing = self.instrument.playing().read(app);
        let keys = Keys {
            lowest: playing.lowest,
            held: playing.held.notes(),
            held_fill: resources.accent_fill_color_default_brush,
            on_held: resources.text_on_accent_fill_color_primary,
            lettering: TextBlockStyle::Caption.text_style(&resources, ON_WHITE),
        };
        let instrument = self.instrument.clone();
        LayoutBuilder::new(move |_app, _context, constraints| {
            let layout =
                KeyboardLayout::fitting(keys.lowest, constraints.max_width, constraints.max_height);
            let with_typing = constraints.max_width >= NARROWEST_WITH_TYPING;
            played_by_pointers(layout, instrument.clone(), keys.drawn(&layout, with_typing))
        })
        .into_widget()
    }
}

/// What the keys are drawn from.
struct Keys {
    lowest: Note,
    held: Vec<Note>,
    /// A held key's colour, and the colour of what is written on it.
    held_fill: Color,
    on_held: Color,
    lettering: TextStyle,
}

impl Keys {
    /// The keys of `layout`, the black over the white, lettered `with_typing` or without.
    fn drawn(&self, layout: &KeyboardLayout, with_typing: bool) -> WidgetRef {
        let white = layout
            .white_keys()
            .map(|(note, place)| self.key(note, place, with_typing));
        let black = layout
            .black_keys()
            .map(|(note, place)| self.key(note, place, with_typing));
        Stack::new()
            .children(white.chain(black).collect::<Vec<_>>())
            .into_widget()
    }

    /// The key of `note` at `place`, lit while it is held.
    fn key(&self, note: Note, place: KeyPlace, with_typing: bool) -> WidgetRef {
        let held = self.held.contains(&note);
        let (fill, written) = match (held, note.is_black()) {
            (true, _) => (self.held_fill, self.on_held),
            (false, true) => (BLACK_KEY, ON_BLACK),
            (false, false) => (WHITE_KEY, ON_WHITE),
        };
        let shape = BoxDecoration::new()
            .color(fill)
            .border(Border::all(KEY_EDGE, 1.0, BorderStyle::Solid, -1.0))
            .border_radius(BorderRadius::vertical(
                Radius::ZERO,
                Radius::circular(KEY_CORNER),
            ));
        let lettering = self.lettering(note, written, with_typing);
        Positioned::new(DecoratedBox::new(shape).child(lettering))
            .left(place.left)
            .top(0.0)
            .width(place.width)
            .height(place.height)
            .into_widget()
    }

    /// What is written at the foot of `note`'s key: the computer's key that plays it, and its
    /// name where it is a C.
    fn lettering(&self, note: Note, color: Color, with_typing: bool) -> WidgetRef {
        let style = self.lettering.clone().color(color);
        let typed = (with_typing && note >= self.lowest)
            .then(|| typing::printed_on_key_for(note.0 - self.lowest.0))
            .flatten()
            .map(str::to_owned);
        let name = note.is_c().then(|| format!("C{}", note.octave()));
        let lines = typed
            .into_iter()
            .chain(name)
            .map(|line| Text::new(line).style(style.clone()).into_widget());
        let column = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .children(lines.collect::<Vec<_>>());
        Align::new()
            .alignment(AlignmentGeometry::BOTTOM_CENTER)
            .child(
                Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, LETTERING_INSET))
                    .child(column),
            )
            .into_widget()
    }
}

/// `keys`, each pointer on them holding the key of `layout` it is on.
fn played_by_pointers(
    layout: KeyboardLayout,
    instrument: Instrument,
    keys: WidgetRef,
) -> WidgetRef {
    let hold = move |app: &mut App, pointer: i64, at: Option<Offset>| {
        let note = at.and_then(|at| layout.note_at(at.dx(), at.dy()));
        instrument.hold(app, Holder::Pointer(pointer), note);
    };
    let (down, moved, up, cancelled) = (hold.clone(), hold.clone(), hold.clone(), hold);
    waft_widgets::Listener::new()
        .behavior(HitTestBehavior::Opaque)
        .on_pointer_down(Rc::new(move |app: &mut App, event| {
            if event.buttons & K_PRIMARY_BUTTON != 0 {
                down(app, event.pointer, Some(event.local_position()));
            }
        }))
        .on_pointer_move(Rc::new(move |app: &mut App, event| {
            if event.buttons & K_PRIMARY_BUTTON != 0 {
                moved(app, event.pointer, Some(event.local_position()));
            }
        }))
        .on_pointer_up(Rc::new(move |app: &mut App, event| {
            up(app, event.pointer, None)
        }))
        .on_pointer_cancel(Rc::new(move |app: &mut App, event| {
            cancelled(app, event.pointer, None);
        }))
        .child(keys)
        .into_widget()
}
