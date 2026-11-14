//! The widget a program's view is shown in: the view's composition layer, filling the window it
//! is given, under the cursor the program asks for and annotated with the style it asks for of the
//! system's bars over it, which Waft's view samples under each bar as it does any app's, with the
//! pointer, the keys and the keyboard's
//! focus going to the program, Edit's commands sent to it while the canvas has the focus, and the
//! program's text open to the input methods. The canvas takes the keyboard's focus only while its
//! view may hold it (`ShownView::set_holds_keyboard`).
//!
//! The canvas draws nothing of the program's: the layer's hardware buffers reach the compositor
//! without passing through the widget tree, and the canvas rebuilds only when its view's cursor
//! or bars' style changes. The program hears the view's size from Waft's view, which the canvas fills.

use std::any::TypeId;
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::rc::Rc;

use waft::ui::{KeyData, KeyEventType, ViewFocusState};
use waft::{
    Actions, AnyAction, App, BuildContext, CompositionLayerView, Entity, Focus, FocusNode,
    FocusNodeLeaf, Handle, IntoWidget, KeyEventResult, KeyRef, MouseRegion, State, StateData,
    StatefulWidget, Subscription, UnfocusDisposition, ValueKey, WidgetRef,
};
use waft_gestures::PointerEvent as HostPointerEvent;
use waft_services::{HardwareKeyboard, KeyEvent as HostKeyEvent, SystemMouseCursor};
use waft_widgets::{AnnotatedRegion, Listener as PointerListener};
use waos::bindings::wasdk::platform::views::{
    ViewFocusDirection, ViewFocusEvent, ViewFocusState as ProgramFocusState,
};
use waos::{HeldModifiers, KeyHandledFuture};

use crate::logic::cursor::cursor_of;
use crate::logic::keys;
use crate::logic::pointer::KnownPointers;
use crate::state::shown_view::{FocusChangeRequested, ShownView};
use crate::ui::edit_commands::edit_actions;
use crate::ui::program_text_input::ProgramTextInput;

/// One program's view, filling the box it is given.
pub struct ProgramCanvas {
    /// The view's own, so a canvas never takes over the focus and pointers of another view's.
    key: KeyRef,
    view: Entity<ShownView>,
}

impl ProgramCanvas {
    pub fn new(view: Entity<ShownView>) -> ProgramCanvas {
        ProgramCanvas {
            key: ValueKey::new(view.entity_id()).into(),
            view,
        }
    }
}

impl Debug for ProgramCanvas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgramCanvas")
            .field("view", &self.view)
            .finish()
    }
}

impl StatefulWidget for ProgramCanvas {
    type State = ProgramCanvasState;

    fn key(&self) -> Option<&KeyRef> {
        Some(&self.key)
    }

    fn create_state(&self) -> ProgramCanvasState {
        ProgramCanvasState {
            state: StateData::new(),
            focus: None,
            focus_requests: None,
            told_focused: false,
            known_pointers: KnownPointers::default(),
            text_input: None,
            edit_actions: HashMap::new(),
        }
    }
}

/// What the canvas keeps: what makes it the keyboard's, what the program has been told of the
/// focus and the pointers, and the view's text input.
pub struct ProgramCanvasState {
    state: StateData<ProgramCanvas>,
    /// The keyboard focus the canvas takes when its window opens, when it is pressed, or when
    /// the program asks for it. The program gets the keys while the canvas has it.
    focus: Option<Handle<FocusNode>>,
    /// Keeps the program's requests for the focus coming.
    focus_requests: Option<Subscription>,
    /// Whether the program was last told its view has the focus.
    told_focused: bool,
    known_pointers: KnownPointers,
    text_input: Option<Handle<ProgramTextInput>>,
    /// What sends the program Edit's commands while the canvas has the focus.
    edit_actions: HashMap<TypeId, AnyAction>,
}

impl State for ProgramCanvasState {
    type Widget = ProgramCanvas;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let view = self.widget(app).view.clone();
        let focus = FocusNode::new(app);
        let focus_requests = app.subscribe(&view, move |app, request: &FocusChangeRequested| {
            change_focus(self, app, request.0);
        });
        let edit_actions = edit_actions(app, &view);
        let text_input = ProgramTextInput::open(app, view, self.context(app));
        let state = app.get_mut(self);
        state.focus = Some(focus);
        state.focus_requests = Some(focus_requests);
        state.text_input = Some(text_input);
        state.edit_actions = edit_actions;
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        let state = app.get_mut(self);
        state.focus_requests = None;
        let (focus, text_input) = (state.focus.take(), state.text_input.take());
        if let Some(text_input) = text_input {
            text_input.close(app);
        }
        if let Some(focus) = focus {
            focus.dispose(app);
        }
    }

    /// Reads the shown view, so the canvas rebuilds when its cursor or its bars' style changes.
    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let view = self.widget(app).view.clone();
        let (layer, cursor, overlay_style) = {
            let shown = view.read(app);
            (shown.layer(), shown.cursor(), shown.overlay_style())
        };
        let layer = CompositionLayerView::new(layer);
        let picture = match overlay_style {
            Some(style) => AnnotatedRegion::new(layer, style).into_widget(),
            None => layer.into_widget(),
        };
        with_input(self, app, cursor_of(cursor), picture)
    }
}

/// `picture`, the view's layer, under `cursor`, wrapped in what carries the pointer, the keys and
/// Edit's commands to the program. The picture is the innermost widget, so annotating it or not
/// keeps the focus where it is.
fn with_input(
    this: Handle<ProgramCanvasState>,
    app: &App,
    cursor: SystemMouseCursor,
    picture: WidgetRef,
) -> WidgetRef {
    let state = app.get(this);
    let focus = state.focus.expect("init_state has run");
    let edit_actions = state.edit_actions.clone();
    let holds_keyboard = this.widget(app).view.read(app).holds_keyboard();
    MouseRegion::new()
        .cursor(cursor.into())
        .on_enter(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::Enter(event));
        }))
        .on_exit(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::Exit(event));
        }))
        .child(Actions::new(
            edit_actions,
            Focus::new(pointer_listener(this).child(picture))
                .focus_node(focus.as_node())
                .autofocus(holds_keyboard)
                .on_focus_change(move |app, focused| deliver_focus(this, app, focused))
                .on_key_event(Rc::new(move |app, _node, event| {
                    deliver_key(this, app, event)
                })),
        ))
        .into_widget()
}

/// Every pointer event over the canvas, on its way to the program. A press also takes the
/// keyboard's focus, where the view may hold it.
fn pointer_listener(this: Handle<ProgramCanvasState>) -> PointerListener {
    PointerListener::new()
        .on_pointer_down(Rc::new(move |app, event| {
            if this.widget(app).view.read(app).holds_keyboard() {
                take_focus(this, app);
            }
            deliver_pointer(this, app, HostPointerEvent::Down(event));
        }))
        .on_pointer_move(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::Move(event));
        }))
        .on_pointer_up(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::Up(event));
        }))
        .on_pointer_hover(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::Hover(event));
        }))
        .on_pointer_cancel(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::Cancel(event));
        }))
        .on_pointer_signal(Rc::new(move |app, event| {
            deliver_pointer(this, app, event);
        }))
        .on_pointer_pan_zoom_start(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::PanZoomStart(event));
        }))
        .on_pointer_pan_zoom_update(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::PanZoomUpdate(event));
        }))
        .on_pointer_pan_zoom_end(Rc::new(move |app, event| {
            deliver_pointer(this, app, HostPointerEvent::PanZoomEnd(event));
        }))
}

/// `event` as the program's, with the modifier keys the keyboard holds as it happens.
fn deliver_pointer(this: Handle<ProgramCanvasState>, app: &mut App, event: HostPointerEvent) {
    let view = this.widget(app).view.clone();
    let (ratio, now) = {
        let shown = view.read(app);
        (shown.device_pixel_ratio(), shown.view().now())
    };
    let modifiers = held_modifiers(app);
    let events = app
        .get_mut(this)
        .known_pointers
        .translate(&event, ratio, now);
    let link = view.read(app).view();
    for event in events {
        link.pointer(event, &modifiers);
    }
}

/// The modifier keys Waft's keyboard holds, from every key it holds: its physical key, with
/// the logical key it means.
fn held_modifiers(app: &mut App) -> HeldModifiers {
    let keyboard = HardwareKeyboard::instance(app);
    keyboard
        .physical_keys_pressed(app)
        .into_iter()
        .filter_map(|physical| {
            let logical = keyboard.look_up_layout(app, physical)?;
            Some((physical.usb_hid_usage, logical.key_id))
        })
        .collect()
}

/// The canvas keeps each key for the program while it has the focus, since the program answers
/// later, on its own thread; a key going down that the program leaves is offered to the menu bar
/// once it says so.
fn deliver_key(
    this: Handle<ProgramCanvasState>,
    app: &mut App,
    event: &HostKeyEvent,
) -> KeyEventResult {
    let view = this.widget(app).view.clone();
    let link = view.read(app).view();
    let reply = link.key(keys::translated(event, link.now()));
    offer_to_menu_bar_if_left(app, event.to_key_data(), reply);
    KeyEventResult::Handled
}

/// Once the program replies that it left `key`, a key going down, the menu bar is offered it
/// (the menus proposal's decision 9), as Chrome sends on a key its page did not consume: a key
/// equivalent, such as ⌘C to Edit's Copy, then chooses its item. Where the platform has no menu
/// bar, a key left goes nowhere further.
fn offer_to_menu_bar_if_left(app: &mut App, key: KeyData, reply: KeyHandledFuture) {
    if key.event_type == KeyEventType::Up {
        return;
    }
    app.spawn(async move |cx| {
        if reply.await {
            return;
        }
        cx.update(|app| {
            if let Some(menu_bar) = app.platform().menu_bar() {
                menu_bar.perform_key_equivalent(&key);
            }
        });
    });
}

/// The canvas gained or lost the keyboard's focus: as its window did, by a press, or at the
/// program's request. Waft does not say which way the focus moved, so the program hears no
/// direction; and the focus node notifies of more than its focus, so only a change is told.
fn deliver_focus(this: Handle<ProgramCanvasState>, app: &mut App, focused: bool) {
    if app.get(this).told_focused == focused {
        return;
    }
    app.get_mut(this).told_focused = focused;
    let state = if focused {
        ProgramFocusState::Focused
    } else {
        ProgramFocusState::Unfocused
    };
    let view = this.widget(app).view.clone();
    view.read(app).view().focus(ViewFocusEvent {
        state,
        direction: ViewFocusDirection::Undefined,
    });
}

/// Gives the canvas the keyboard's focus or takes it away, as the program asked. Waft's `View`
/// asks its host to bring the window forward when a widget in it takes the focus.
fn change_focus(this: Handle<ProgramCanvasState>, app: &mut App, state: ViewFocusState) {
    match state {
        ViewFocusState::Focused => take_focus(this, app),
        ViewFocusState::Unfocused => {
            let focus = app.get(this).focus.expect("init_state has run");
            focus.as_node().unfocus(app, UnfocusDisposition::Scope);
        }
    }
}

fn take_focus(this: Handle<ProgramCanvasState>, app: &mut App) {
    let focus = app.get(this).focus.expect("init_state has run");
    focus.request_focus(app, None);
}
