//! The keys the app answers itself: each Playback command's, run on what plays. They sit with
//! the app's other shortcuts, above the text fields', so a field still types a space and moves
//! its caret with the arrows; the menu items show the same keys. And Esc on the page of what
//! plays, which goes back to the library: the page's own answer to the dismissal Esc asks for,
//! which its route leaves unanswered, as a page cannot be dismissed, and a dialog over the page
//! answers first.

use std::any::TypeId;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use waft_foundation::{App, Entity, Handle, Listener};
use waft_widgets::{
    Action, Actions, AnyAction, BuildContext, CallbackAction, DismissIntent, Focus, FocusNode,
    FocusNodeLeaf, IntoWidget, KeyRef, ShortcutMap, State, StateData, StatefulWidget,
    VoidCallbackIntent, WidgetRef, WidgetsApp,
};

use crate::commands::PlaybackCommand;
use crate::playback::{self, Playback};

/// The Playback commands' keys, before the app's usual ones, which Space and the arrows would
/// otherwise move the focus with.
pub(crate) fn shortcuts(app: &App, playback: &Entity<Playback>) -> ShortcutMap {
    let playing = PlaybackCommand::ALL
        .into_iter()
        .map(|command| playback_shortcut(command, playback));
    playing.chain(WidgetsApp::default_shortcuts(app)).collect()
}

/// `command`'s key, running it on what plays.
fn playback_shortcut(
    command: PlaybackCommand,
    playback: &Entity<Playback>,
) -> (waft_widgets::ShortcutActivatorRef, waft_widgets::IntentRef) {
    let playback = playback.clone();
    let run = Listener::new(move |app: &mut App| {
        if let Some(player) = playback::current_player(&playback, app) {
            command.run(app, player);
        }
    });
    (
        Rc::new(command.key()),
        Rc::new(VoidCallbackIntent::new(run)),
    )
}

/// `child`, the page of what plays, which Esc leaves for the library while the focus is in it,
/// as it is from the page's start: its focus is asked for as the page comes, since the library's
/// page before it may have left a focus of its own behind, which autofocus would not displace.
pub(crate) struct BackOnEscape {
    playback: Entity<Playback>,
    child: WidgetRef,
}

impl BackOnEscape {
    pub(crate) fn new(playback: Entity<Playback>, child: WidgetRef) -> BackOnEscape {
        BackOnEscape { playback, child }
    }
}

impl fmt::Debug for BackOnEscape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackOnEscape").finish_non_exhaustive()
    }
}

impl StatefulWidget for BackOnEscape {
    type State = BackOnEscapeState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> BackOnEscapeState {
        BackOnEscapeState {
            state: StateData::new(),
            actions: HashMap::new(),
            focus: None,
        }
    }
}

pub(crate) struct BackOnEscapeState {
    state: StateData<BackOnEscape>,
    /// The dismissal's action, made once for the page.
    actions: HashMap<TypeId, AnyAction>,
    /// The page's focus, which its keys reach the action from.
    focus: Option<Handle<FocusNode>>,
}

impl State for BackOnEscapeState {
    type Widget = BackOnEscape;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let playback = self.widget(app).playback.clone();
        let back = CallbackAction::<DismissIntent>::new(
            app,
            Rc::new(move |app: &mut App, _intent: &DismissIntent| {
                playback::close(&playback, app);
                None
            }),
        );
        app.get_mut(self)
            .actions
            .insert(TypeId::of::<DismissIntent>(), back.as_action());
        // Taken as the node is attached, below.
        let focus = FocusNode::new(app);
        focus.request_focus(app, None);
        app.get_mut(self).focus = Some(focus);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        for action in std::mem::take(&mut app.get_mut(self).actions).into_values() {
            app.destroy(action.id());
        }
        if let Some(focus) = app.get_mut(self).focus.take() {
            focus.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let child = self.widget(app).child.clone();
        let focus = app.get(self).focus.expect("init_state has run");
        Actions::new(
            app.get(self).actions.clone(),
            Focus::new(child).focus_node(focus.as_node()),
        )
        .into_widget()
    }
}
