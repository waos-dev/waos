//! The canvas of a prompt's dialog: the composition layer showing WaOS's context, filling the
//! dialog, with the pointer and the keys going to WaOS as the prompt's reports. The cursor over it
//! is left to the dialog's view's host, which shows the one WaOS's view asks for, as Waft leaves
//! the cursor over a platform view to the system.
//!
//! The invariant: the canvas draws nothing, and decides nothing of the prompt; every key it takes
//! is WaOS's, so none goes further in the window process.

use std::fmt::{self, Debug};
use std::rc::Rc;

use waft::{
    App, BuildContext, CompositionLayerView, Focus, FocusNode, FocusNodeLeaf, Handle, IntoWidget,
    KeyEventResult, KeyRef, MouseRegion, State, StateData, StatefulWidget, ValueKey, WidgetRef,
};
use waft_gestures::PointerEvent;
use waft_services::{KeyEvent, MouseCursor};
use waft_widgets::Listener as PointerListener;
use waos_window_messages::PromptReport;

use crate::prompt_dialogs::DialogLink;
use crate::prompt_pointers::PromptPointers;

/// One prompt's canvas, filling its dialog.
pub(crate) struct PromptCanvas {
    /// The prompt's own, so a canvas never takes over another dialog's focus and pointers.
    key: KeyRef,
    link: Rc<DialogLink>,
}

impl PromptCanvas {
    pub(crate) fn new(link: Rc<DialogLink>) -> PromptCanvas {
        PromptCanvas {
            key: ValueKey::new(link.prompt()).into(),
            link,
        }
    }
}

impl Debug for PromptCanvas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromptCanvas")
            .field("prompt", &self.link.prompt())
            .finish()
    }
}

impl StatefulWidget for PromptCanvas {
    type State = PromptCanvasState;

    fn key(&self) -> Option<&KeyRef> {
        Some(&self.key)
    }

    fn create_state(&self) -> PromptCanvasState {
        PromptCanvasState {
            state: StateData::new(),
            focus: None,
            pointers: PromptPointers::default(),
        }
    }
}

/// What the canvas keeps: what makes it the keyboard's while its dialog has the focus, and the
/// pointers WaOS has been told of.
pub(crate) struct PromptCanvasState {
    state: StateData<PromptCanvas>,
    focus: Option<Handle<FocusNode>>,
    pointers: PromptPointers,
}

impl State for PromptCanvasState {
    type Widget = PromptCanvas;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let focus = FocusNode::new(app);
        app.get_mut(self).focus = Some(focus);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(focus) = app.get_mut(self).focus.take() {
            focus.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let layer = self.widget(app).link.layer();
        let focus = app.get(self).focus.expect("init_state has run");
        MouseRegion::new()
            .cursor(<dyn MouseCursor>::uncontrolled())
            .on_enter(Rc::new(move |app, event| {
                deliver_pointer(self, app, PointerEvent::Enter(event));
            }))
            .on_exit(Rc::new(move |app, event| {
                deliver_pointer(self, app, PointerEvent::Exit(event));
            }))
            .child(
                Focus::new(pointer_listener(self).child(CompositionLayerView::new(layer)))
                    .focus_node(focus.as_node())
                    .autofocus(true)
                    .on_key_event(Rc::new(move |app, _node, event| {
                        deliver_key(self, app, event)
                    })),
            )
            .into_widget()
    }
}

/// Every pointer event over the canvas, on its way to WaOS. A press also takes the keyboard's
/// focus, as it does over a program's canvas.
fn pointer_listener(this: Handle<PromptCanvasState>) -> PointerListener {
    PointerListener::new()
        .on_pointer_down(Rc::new(move |app, event| {
            take_focus(this, app);
            deliver_pointer(this, app, PointerEvent::Down(event));
        }))
        .on_pointer_move(Rc::new(move |app, event| {
            deliver_pointer(this, app, PointerEvent::Move(event));
        }))
        .on_pointer_up(Rc::new(move |app, event| {
            deliver_pointer(this, app, PointerEvent::Up(event));
        }))
        .on_pointer_hover(Rc::new(move |app, event| {
            deliver_pointer(this, app, PointerEvent::Hover(event));
        }))
        .on_pointer_cancel(Rc::new(move |app, event| {
            deliver_pointer(this, app, PointerEvent::Cancel(event));
        }))
        .on_pointer_signal(Rc::new(move |app, event| {
            deliver_pointer(this, app, event);
        }))
        .on_pointer_pan_zoom_start(Rc::new(move |app, event| {
            deliver_pointer(this, app, PointerEvent::PanZoomStart(event));
        }))
        .on_pointer_pan_zoom_update(Rc::new(move |app, event| {
            deliver_pointer(this, app, PointerEvent::PanZoomUpdate(event));
        }))
        .on_pointer_pan_zoom_end(Rc::new(move |app, event| {
            deliver_pointer(this, app, PointerEvent::PanZoomEnd(event));
        }))
}

/// `event` as the reports WaOS takes.
fn deliver_pointer(this: Handle<PromptCanvasState>, app: &mut App, event: PointerEvent) {
    let link = Rc::clone(&this.widget(app).link);
    let reports = app.get_mut(this).pointers.translate(&event, link.ratio());
    for report in reports {
        link.report(PromptReport::Pointer(report));
    }
}

/// Every key goes to WaOS, which hands it to its own framework, where the prompt's focus is.
fn deliver_key(this: Handle<PromptCanvasState>, app: &mut App, event: &KeyEvent) -> KeyEventResult {
    this.widget(app)
        .link
        .report(PromptReport::Key(event.to_key_data()));
    KeyEventResult::Handled
}

fn take_focus(this: Handle<PromptCanvasState>, app: &mut App) {
    let focus = app.get(this).focus.expect("init_state has run");
    focus.request_focus(app, None);
}
