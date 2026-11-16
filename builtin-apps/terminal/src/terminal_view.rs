//! A session's grid in its window: drawn in cells measured from the font, sized to the room it is
//! given, which sets the screen's size; with the keyboard's focus, the keys and the text typed
//! going to the shell; the pointer selecting and scrolling; and Edit's Copy, Paste and Select
//! All, and Clear, acting on it.
//!
//! The invariant a later editor will break: the screen's size follows the grid's room after the
//! frame that laid it out, never during the build, where changing the session would ask for the
//! build under way again.

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use waft_embedder::{Rect, Size};
use waft_foundation::{App, Entity, Handle, Ref};
use waft_services::SystemMouseCursors;
use waft_widgets::{
    ActionRef, Actions, BuildContext, CallbackAction, ColoredBox, CopySelectionTextIntent,
    CustomPaint, EdgeInsetsGeometry, Focus, FocusNode, FocusNodeLeaf, Intent, IntoWidget,
    LayoutBuilder, Listener, MediaQuery, MouseRegion, Padding, PasteTextIntent,
    SelectAllTextIntent, State, StateData, StatefulWidget, View, WidgetRef,
};

use crate::cell_metrics::CellMetrics;
use crate::clipboard_commands::{copy_selection, paste, select_all};
use crate::grid_painter::{GridPainter, TextSettings, color, measure_cells};
use crate::palette::Palette;
use crate::pointing::Pointing;
use crate::run_cache::RunCache;
use crate::session::{Session, clear, resize};
use crate::text_entry::TextEntry;
use crate::typing::{KeyTarget, key_pressed};

/// The room left around the grid, in logical pixels.
const PADDING: f64 = 6.0;

/// Asks the terminal with the focus to forget its scrollback and draw its screen again.
#[derive(Debug)]
pub(crate) struct ClearIntent;

impl Intent for ClearIntent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The grid of `session`, drawn in the font `text` holds.
pub(crate) struct TerminalView {
    pub(crate) session: Entity<Session>,
    pub(crate) text: Entity<TextSettings>,
}

impl fmt::Debug for TerminalView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalView").finish_non_exhaustive()
    }
}

impl StatefulWidget for TerminalView {
    type State = TerminalViewState;

    fn create_state(&self) -> TerminalViewState {
        TerminalViewState {
            state: StateData::new(),
            parts: None,
            measured: None,
        }
    }
}

/// What the view makes once and keeps while it is in the tree.
#[derive(Clone)]
struct Parts {
    focus: Ref<FocusNode>,
    entry: Ref<TextEntry>,
    pointing: Ref<Pointing>,
    /// Edit's commands and Clear, acting on the session.
    actions: HashMap<TypeId, ActionRef>,
    /// The runs of text the last frame laid out.
    runs: Rc<RefCell<RunCache>>,
}

/// The cells measured for a font on a screen, kept until either changes.
#[derive(Clone, Copy, PartialEq)]
struct Measured {
    text: TextSettings,
    pixel_ratio: f64,
    cells: CellMetrics,
}

pub(crate) struct TerminalViewState {
    state: StateData<TerminalView>,
    parts: Option<Parts>,
    measured: Option<Measured>,
}

impl TerminalViewState {
    fn parts(self: Handle<Self>, app: &App) -> &Parts {
        app.get(self).parts.as_ref().expect("init_state has run")
    }

    /// The cells of `text` on a screen of `pixel_ratio`, measured where they were not yet.
    fn cells(
        self: Handle<Self>,
        app: &mut App,
        text: TextSettings,
        pixel_ratio: f64,
    ) -> CellMetrics {
        if let Some(measured) = app.get(self).measured
            && measured.text == text
            && measured.pixel_ratio == pixel_ratio
        {
            return measured.cells;
        }
        let cells = measure_cells(app, &text, pixel_ratio);
        app.get_mut(self).measured = Some(Measured {
            text,
            pixel_ratio,
            cells,
        });
        cells
    }

    /// With the focus the view takes the text typed; without it, it stops. Either way the
    /// cursor is drawn anew.
    fn focus_changed(self: Handle<Self>, app: &mut App, focused: bool) {
        let entry = self.parts(app).entry.handle();
        if focused {
            let context = self.context(app);
            let view = View::maybe_of(app, context).map(|view| view.id());
            entry.open(app, view);
        } else {
            entry.close(app);
        }
        self.set_state(app, |_state| {});
    }

    /// Tells the platform where the cursor is in the view, once the frame has laid the grid out.
    fn place_caret(self: Handle<Self>, app: &mut App, cells: CellMetrics) {
        let context = self.context(app);
        let Some(grid) = context.find_render_object(app) else {
            return;
        };
        let session = self.widget(app).session.clone();
        let Some((row, column)) = session.read(app).screen.cursor() else {
            return;
        };
        let caret = Rect::from_ltwh(
            PADDING + f64::from(column) * cells.width,
            PADDING + f64::from(row) * cells.height,
            cells.width,
            cells.height,
        );
        let size = MediaQuery::size_of(app, context);
        let transform = grid.get_transform_to(app, None);
        let entry = self.parts(app).entry.handle();
        entry.place_caret(app, size, transform, caret);
    }
}

impl State for TerminalViewState {
    type Widget = TerminalView;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let session = self.widget(app).session.clone();
        let parts = Parts {
            focus: FocusNode::new(app),
            entry: TextEntry::new(app, session.clone()),
            pointing: Pointing::new(app, session.clone()),
            actions: session_actions(app, &session),
            runs: Rc::default(),
        };
        app.get_mut(self).parts = Some(parts);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        let Some(parts) = app.get_mut(self).parts.take() else {
            return;
        };
        parts.entry.close(app);
        parts.focus.dispose(app);
        parts.runs.borrow_mut().clear();
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let session = self.widget(app).session.clone();
        let text = *self.widget(app).text.read(app);
        let parts = self.parts(app).clone();
        let pixel_ratio = MediaQuery::device_pixel_ratio_of(app, context);
        let palette = Palette::of_look(MediaQuery::platform_brightness_of(app, context));
        let cells = self.cells(app, text, pixel_ratio);
        parts.pointing.set_metrics(app, cells);
        let focused = parts.focus.as_node().has_focus(app);
        let composing = session.read(app).composing.clone();
        if focused {
            let state = self;
            app.spawn(async move |cx| cx.update(|app| state.place_caret_if_mounted(app, cells)));
        }
        let grid: WidgetRef = Rc::new(LayoutBuilder::new({
            let session = session.clone();
            let runs = Rc::clone(&parts.runs);
            move |app, _context, constraints| {
                let room = constraints.biggest();
                fit_screen(app, &session, cells, room, pixel_ratio);
                let painter = GridPainter {
                    session: session.clone(),
                    metrics: cells,
                    text,
                    palette,
                    focused,
                    composing: composing.clone(),
                    runs: Rc::clone(&runs),
                };
                CustomPaint::new().painter(painter).size(room).into_widget()
            }
        }));
        let pointed = pointer_listener(&parts, grid);
        let padded = ColoredBox::new(color(palette.background)).child(
            Padding::new(EdgeInsetsGeometry::all(PADDING)).child(
                MouseRegion::new()
                    .cursor(Rc::new(SystemMouseCursors::TEXT))
                    .child(pointed),
            ),
        );
        let keys = {
            let target = KeyTarget {
                session: session.clone(),
                text: self.widget(app).text.clone(),
            };
            Rc::new(move |app: &mut App, focus, event: &_| key_pressed(app, &target, focus, event))
        };
        let focus = Focus::new(padded)
            .focus_node(parts.focus.erase())
            .autofocus(true)
            .on_key_event(keys)
            .on_focus_change(move |app, focused| self.focus_changed(app, focused));
        Actions::new(parts.actions, focus).into_widget()
    }
}

impl TerminalViewState {
    /// [`place_caret`](Self::place_caret), unless the view left the tree since it was asked.
    fn place_caret_if_mounted(self: Handle<Self>, app: &mut App, cells: CellMetrics) {
        if app.contains(self) && self.mounted(app) {
            self.place_caret(app, cells);
        }
    }
}

/// Gives the session's screen the cells that fit in `room`, once this frame is done, where they
/// are not the cells it has.
fn fit_screen(
    app: &mut App,
    session: &Entity<Session>,
    cells: CellMetrics,
    room: Size,
    pixel_ratio: f64,
) {
    let size = cells.grid_in(room.width(), room.height());
    if session.read(app).screen.size() == size {
        return;
    }
    let cell_pixels = (
        (cells.width * pixel_ratio).round() as u16,
        (cells.height * pixel_ratio).round() as u16,
    );
    let session = session.downgrade();
    app.spawn(async move |cx| {
        cx.update(|app| {
            if let Some(session) = session.upgrade() {
                resize(app, &session, size, cell_pixels);
            }
        });
    });
}

/// `grid` hearing the pointer: a press takes the keyboard's focus and shows the on-screen
/// keyboard where there is one, then selects, scrolls or is reported, as `pointing` decides.
fn pointer_listener(parts: &Parts, grid: WidgetRef) -> Listener {
    let (focus, entry) = (parts.focus.clone(), parts.entry.clone());
    let [down, moving, up, signalled] = std::array::from_fn(|_| parts.pointing.clone());
    Listener::new()
        .on_pointer_down(Rc::new(move |app, event| {
            if focus.as_node().has_focus(app) {
                let view = Some(event.view_id);
                entry.open(app, view);
            } else {
                focus.request_focus(app, None);
            }
            down.pressed(app, &event);
        }))
        .on_pointer_move(Rc::new(move |app, event| moving.moved(app, &event)))
        .on_pointer_up(Rc::new(move |app, event| up.released(app, &event)))
        .on_pointer_signal(Rc::new(move |app, event| signalled.signalled(app, &event)))
        .child(grid)
}

/// Edit's Copy, Paste and Select All, as the platform's menu and its keys send them to what has
/// the focus, and Clear, each acting on `session`.
fn session_actions(app: &mut App, session: &Entity<Session>) -> HashMap<TypeId, ActionRef> {
    let mut actions = HashMap::new();
    add_action::<CopySelectionTextIntent>(app, &mut actions, session, copy_selection);
    add_action::<PasteTextIntent>(app, &mut actions, session, paste);
    add_action::<SelectAllTextIntent>(app, &mut actions, session, select_all);
    add_action::<ClearIntent>(app, &mut actions, session, clear);
    actions
}

/// Adds to `actions` what the intent `I` does: `command`, on `session`.
fn add_action<I: Intent>(
    app: &mut App,
    actions: &mut HashMap<TypeId, ActionRef>,
    session: &Entity<Session>,
    command: fn(&mut App, &Entity<Session>),
) {
    let session = session.clone();
    let action = CallbackAction::<I>::new(
        app,
        Rc::new(move |app: &mut App, _intent: &I| {
            command(app, &session);
            None
        }),
    );
    actions.insert(TypeId::of::<I>(), action.into_erased());
}
