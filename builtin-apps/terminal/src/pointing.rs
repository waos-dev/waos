//! The pointer over a session's grid: a mouse's drag selects text, by cells, by words after a
//! double click and by lines after a triple one; its wheel scrolls the scrollback; a finger's
//! drag scrolls it too. While the program on the screen asked to hear of the mouse, the presses,
//! drags and wheel go to it instead, unless Shift is held, as in xterm.
//!
//! The invariant a later editor will break: a press that was reported to the program has its
//! drag and its release reported too, and one that started a selection never is, whatever the
//! program asks for meanwhile.

use waft_embedder::{Offset, PointerDeviceKind};
use waft_foundation::{App, Entity, Handle};
use waft_gestures::{
    K_PRIMARY_BUTTON, K_SECONDARY_BUTTON, PointerDownEvent, PointerEvent, PointerMoveEvent,
    PointerUpEvent,
};
use waft_services::LogicalKeyboardKey as Key;

use crate::cell_metrics::CellMetrics;
use crate::clicks::{ClickUnit, Clicks};
use crate::keyboard::held_modifiers;
use crate::keys::{Modifiers, bytes_for_key};
use crate::mouse::{MouseAction, MouseButton, mouse_report};
use crate::screen::Screen;
use crate::selection::{CellRange, Selection};
use crate::session::{Session, scroll_by, select, type_bytes};
use crate::wheel::Wheel;

/// The middle button's bit among a pointer's buttons.
const K_MIDDLE_BUTTON: i64 = 0x04;

/// What a press began, until its release.
#[derive(Clone, Copy, Debug)]
enum Gesture {
    /// A selection, from the cells the press took, by the unit its click count chose.
    Selecting { from: CellRange, unit: ClickUnit },
    /// A press reported to the program, with the cell last reported.
    Reported {
        button: MouseButton,
        cell: (u16, u16),
    },
    /// A finger's drag, which scrolls.
    Scrolling,
}

/// The pointer's state over one session's grid.
pub(crate) struct Pointing {
    session: Entity<Session>,
    /// The cells the grid is drawn in, as the view measured them last.
    metrics: Option<CellMetrics>,
    clicks: Clicks,
    wheel: Wheel,
    gesture: Option<Gesture>,
}

impl Pointing {
    pub(crate) fn new(app: &mut App, session: Entity<Session>) -> Handle<Pointing> {
        app.create(Pointing {
            session,
            metrics: None,
            clicks: Clicks::default(),
            wheel: Wheel::default(),
            gesture: None,
        })
    }

    /// The grid is drawn in cells of `metrics` from now on.
    pub(crate) fn set_metrics(self: Handle<Self>, app: &mut App, metrics: CellMetrics) {
        app.get_mut(self).metrics = Some(metrics);
    }

    pub(crate) fn pressed(self: Handle<Self>, app: &mut App, event: &PointerDownEvent) {
        let Some(cell) = self.cell_at(app, event.local_position()) else {
            return;
        };
        let gesture = if event.kind == PointerDeviceKind::Touch {
            Some(Gesture::Scrolling)
        } else if let Some(button) = self.reported_button(app, event.buttons) {
            self.report(app, MouseAction::Press(button), cell);
            Some(Gesture::Reported { button, cell })
        } else if event.buttons & K_PRIMARY_BUTTON != 0 {
            Some(self.start_selecting(app, event, cell))
        } else {
            None
        };
        app.get_mut(self).gesture = gesture;
    }

    pub(crate) fn moved(self: Handle<Self>, app: &mut App, event: &PointerMoveEvent) {
        let Some(cell) = self.cell_at(app, event.local_position()) else {
            return;
        };
        match app.get(self).gesture {
            Some(Gesture::Selecting { from, unit }) => {
                let session = app.get(self).session.clone();
                let to = cells_of(&session.read(app).screen, unit, cell);
                let selection = Selection::of(from).extended_to(to);
                let is_a_click = unit == ClickUnit::Cell && to == from;
                select(app, &session, (!is_a_click).then_some(selection));
            }
            Some(Gesture::Reported { button, cell: last }) if last != cell => {
                self.report(app, MouseAction::Drag(button), cell);
                app.get_mut(self).gesture = Some(Gesture::Reported { button, cell });
            }
            Some(Gesture::Scrolling) => self.scroll(app, -event.local_delta().dy(), cell),
            Some(Gesture::Reported { .. }) | None => {}
        }
    }

    pub(crate) fn released(self: Handle<Self>, app: &mut App, event: &PointerUpEvent) {
        let gesture = app.get_mut(self).gesture.take();
        if let Some(Gesture::Reported { button, cell }) = gesture {
            let cell = self.cell_at(app, event.local_position()).unwrap_or(cell);
            self.report(app, MouseAction::Release(button), cell);
        }
    }

    /// A wheel's turn, or a trackpad's scroll.
    pub(crate) fn signalled(self: Handle<Self>, app: &mut App, event: &PointerEvent) {
        let PointerEvent::Scroll(scroll) = event else {
            return;
        };
        if let Some(cell) = self.cell_at(app, scroll.local_position()) {
            self.scroll(app, scroll.scroll_delta.dy(), cell);
        }
    }

    /// The row and the column of the cell at `position` in the grid; none before the grid has
    /// been measured.
    fn cell_at(self: Handle<Self>, app: &App, position: Offset) -> Option<(u16, u16)> {
        let pointing = app.get(self);
        let size = pointing.session.read(app).screen.size();
        let metrics = pointing.metrics?;
        Some(metrics.cell_at(position.dx(), position.dy(), size))
    }

    /// The button of `buttons` a press is reported with; none where the program on the screen
    /// did not ask for the mouse, or Shift keeps the press for a selection.
    fn reported_button(self: Handle<Self>, app: &mut App, buttons: i64) -> Option<MouseButton> {
        let asked = app
            .get(self)
            .session
            .read(app)
            .screen
            .mouse_reporting()
            .is_on();
        if !asked || held_modifiers(app).shift {
            return None;
        }
        if buttons & K_PRIMARY_BUTTON != 0 {
            Some(MouseButton::Left)
        } else if buttons & K_MIDDLE_BUTTON != 0 {
            Some(MouseButton::Middle)
        } else if buttons & K_SECONDARY_BUTTON != 0 {
            Some(MouseButton::Right)
        } else {
            None
        }
    }

    /// Reports `action` at `cell` to the program on the screen, if it asked to hear of it.
    fn report(self: Handle<Self>, app: &mut App, action: MouseAction, cell: (u16, u16)) {
        let modifiers = held_modifiers(app);
        let session = app.get(self).session.clone();
        let reporting = session.read(app).screen.mouse_reporting();
        let (row, column) = cell;
        if let Some(report) = mouse_report(action, column, row, modifiers, reporting) {
            type_bytes(app, &session, &report);
        }
    }

    /// Starts a selection at `cell`: the word or the line there after a double or a triple
    /// click, which is selected at once, or the cell, which a drag selects from.
    fn start_selecting(
        self: Handle<Self>,
        app: &mut App,
        event: &PointerDownEvent,
        cell: (u16, u16),
    ) -> Gesture {
        let unit = app.get_mut(self).clicks.press(event.time_stamp, cell);
        let session = app.get(self).session.clone();
        let from = cells_of(&session.read(app).screen, unit, cell);
        let selected = (unit != ClickUnit::Cell).then(|| Selection::of(from));
        select(app, &session, selected);
        Gesture::Selecting { from, unit }
    }

    /// Scrolls by `pixels`, positive towards the text's end, with the pointer over `cell`: the
    /// program on the screen hears the wheel if it asked for the mouse; a full-screen program
    /// that did not is sent the arrow keys, as it has no scrollback to show; otherwise the
    /// scrollback scrolls.
    fn scroll(self: Handle<Self>, app: &mut App, pixels: f64, cell: (u16, u16)) {
        let Some(line_height) = app.get(self).metrics.map(|metrics| metrics.height) else {
            return;
        };
        let lines = app.get_mut(self).wheel.lines(pixels, line_height);
        if lines == 0 {
            return;
        }
        let session = app.get(self).session.clone();
        let screen = &session.read(app).screen;
        if screen.mouse_reporting().is_on() {
            let action = if lines > 0 {
                MouseAction::WheelDown
            } else {
                MouseAction::WheelUp
            };
            for _ in 0..lines.unsigned_abs() {
                self.report(app, action, cell);
            }
        } else if screen.is_alternate() {
            let key = if lines > 0 {
                Key::ARROW_DOWN
            } else {
                Key::ARROW_UP
            };
            let arrow = bytes_for_key(key, Modifiers::default(), screen.key_modes(false));
            let typed = arrow.unwrap_or_default().repeat(lines.unsigned_abs());
            type_bytes(app, &session, &typed);
        } else {
            scroll_by(app, &session, -lines);
        }
    }
}

/// The cells a press or a drag at `cell`, a row and a column of the rows shown, takes by `unit`.
fn cells_of(screen: &Screen, unit: ClickUnit, cell: (u16, u16)) -> CellRange {
    let (row, column) = cell;
    let (first, last) = match unit {
        ClickUnit::Cell => (column, column),
        ClickUnit::Word => screen.word_at(row, column),
        ClickUnit::Line => (0, screen.size().columns - 1),
    };
    CellRange {
        first: screen.point_at(row, first),
        last: screen.point_at(row, last),
    }
}
