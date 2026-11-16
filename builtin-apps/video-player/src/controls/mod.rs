//! The controls over the picture, in the familiar style: a bar along its bottom with back and
//! forward ten seconds, play or pause, the time played, the seek bar and the time left, the
//! sound, the rate and the tracks; and the way back to the library at its top left. They show
//! while the pointer moves, and hide, with the pointer, a few seconds after it stops, unless the
//! player is paused or the pointer is over the bar.
//!
//! The invariant: the controls hide only through the timer the pointer's last move started, so
//! a move always shows them for that long.

mod bar;
mod menu_button;
mod progress;
mod track_bar;
mod volume;

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use waft_foundation::{App, Handle, ListenableObject, Listener, Ref, Timer};
use waft_services::SystemMouseCursors;
use waft_widgets::{
    AnimatedOpacity, BuildContext, IgnorePointer, IntoWidget, KeyRef, MouseRegion, Player,
    Positioned, Stack, StackFit, State, StateData, StatefulWidget, WidgetRef,
};
use waft_winui::Page;

use crate::commands;

/// How long the controls stay after the pointer stops.
const SHOWN_FOR: Duration = Duration::from_secs(3);

/// How long the controls take to fade.
const FADE: Duration = Duration::from_millis(250);

/// How far the bar keeps from the window's sides and bottom.
const MARGIN: f64 = 16.0;

/// `child`, the picture, with `player`'s controls over it, and `back`, which goes back to the
/// library.
pub(crate) struct Controls {
    player: Ref<Player>,
    child: WidgetRef,
    back: Listener,
}

impl Controls {
    pub(crate) fn new(player: Ref<Player>, child: WidgetRef, back: Listener) -> Controls {
        Controls {
            player,
            child,
            back,
        }
    }
}

impl fmt::Debug for Controls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Controls").finish_non_exhaustive()
    }
}

impl StatefulWidget for Controls {
    type State = ControlsState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> ControlsState {
        ControlsState {
            state: StateData::new(),
            shown: true,
            over_bar: false,
            hiding: None,
        }
    }
}

pub(crate) struct ControlsState {
    state: StateData<Controls>,
    shown: bool,
    /// Whether the pointer is over the bar, which keeps it shown.
    over_bar: bool,
    /// What hides the controls once the pointer has been still long enough.
    hiding: Option<Timer>,
}

impl ControlsState {
    fn player(self: Handle<Self>, app: &App) -> Handle<Player> {
        self.widget(app).player.handle()
    }

    /// The pointer moved: the controls show, for [`SHOWN_FOR`] from now.
    fn pointer_moved(self: Handle<Self>, app: &mut App) {
        if !app.get(self).shown {
            self.set_state(app, |state| state.shown = true);
        }
        self.hide_later(app);
    }

    /// Hides the controls [`SHOWN_FOR`] from now, unless the pointer moves first.
    fn hide_later(self: Handle<Self>, app: &mut App) {
        if let Some(hiding) = app.get_mut(self).hiding.take() {
            hiding.cancel(app);
        }
        let hide = Listener::handle_method(self, Self::hide_unless_kept);
        let hiding = Timer::new(app, SHOWN_FOR, hide);
        app.get_mut(self).hiding = Some(hiding);
    }

    /// Hides the controls, unless the player is paused or the pointer is over the bar.
    fn hide_unless_kept(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).hiding = None;
        let paused = commands::is_paused(self.player(app).state(app));
        if self.mounted(app) && !paused && !app.get(self).over_bar {
            self.set_state(app, |state| state.shown = false);
        }
    }

    /// The player changed: a pause shows the controls, and playing again hides them later.
    fn player_changed(self: Handle<Self>, app: &mut App) {
        let paused = commands::is_paused(self.player(app).state(app));
        if paused && !app.get(self).shown {
            self.set_state(app, |state| state.shown = true);
        } else if !paused && app.get(self).shown && app.get(self).hiding.is_none() {
            self.hide_later(app);
        }
    }

    fn pointer_over_bar(self: Handle<Self>, app: &mut App, over: bool) {
        app.get_mut(self).over_bar = over;
    }

    fn listener(self: Handle<Self>) -> Listener {
        Listener::handle_method(self, Self::player_changed)
    }
}

impl State for ControlsState {
    type Widget = Controls;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        self.player(app).add_listener(app, self.listener());
        self.hide_later(app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        self.player(app).remove_listener(app, &self.listener());
        if let Some(hiding) = app.get_mut(self).hiding.take() {
            hiding.cancel(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let (player, child, shown) = (
            self.widget(app).player.clone(),
            self.widget(app).child.clone(),
            app.get(self).shown,
        );
        let bar = MouseRegion::new()
            .on_enter(Rc::new(move |app: &mut App, _event| {
                self.pointer_over_bar(app, true);
            }))
            .on_exit(Rc::new(move |app: &mut App, _event| {
                self.pointer_over_bar(app, false);
            }))
            .child(bar::control_bar(&player));
        let placed = Positioned::new(faded(shown, bar))
            .left(MARGIN)
            .right(MARGIN)
            .bottom(MARGIN);
        let back = Positioned::new(faded(
            shown,
            bar::back_button(self.widget(app).back.clone()),
        ))
        .left(MARGIN)
        .top(MARGIN);
        let cursor = if shown {
            SystemMouseCursors::BASIC
        } else {
            SystemMouseCursors::NONE
        };
        MouseRegion::new()
            .cursor(cursor.into())
            .on_hover(Rc::new(move |app: &mut App, _event| {
                self.pointer_moved(app)
            }))
            .child(Stack::new().fit(StackFit::Expand).children([
                child,
                clear_of_the_bars([placed.into_widget(), back.into_widget()]),
            ]))
            .into_widget()
    }
}

/// `controls` over the picture, kept clear of a phone's bars and cutout as the picture is not.
/// The page has no background, so the picture shows around them and takes the taps they miss.
fn clear_of_the_bars(controls: [WidgetRef; 2]) -> WidgetRef {
    let over = Stack::new().fit(StackFit::Expand).children(controls);
    Page::new().content(over).into_widget()
}

/// `child`, shown or faded out, and passed through by the pointer while faded.
fn faded<K>(shown: bool, child: impl IntoWidget<K>) -> WidgetRef {
    AnimatedOpacity::new(if shown { 1.0 } else { 0.0 }, FADE)
        .child(IgnorePointer::new().ignoring(!shown).child(child))
        .into_widget()
}
