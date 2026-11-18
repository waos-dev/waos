//! A session's window: asked for at a size that holds a classic 80 by 24 screen, kept no smaller
//! than a few words wide, titled as its session says, closed by File's Close Window, and closed
//! by itself once its shell has ended as the user asked.
//!
//! The invariant a later editor will break: the window's state opens its session, once, as the
//! window arrives, and hangs it up as the window goes. A session opened in a build, the
//! `MultiViewApp`'s builder's included, is a new shell at each rebuild, and the build that read it
//! is rebuilt each time it changes: shells start and are hung up at once, and nothing is drawn.

use std::any::TypeId;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use waft_embedder::{Size, WindowConfig, WindowRef};
use waft_foundation::{App, Entity, Handle, Subscription};
use waft_widgets::{
    ActionRef, Actions, BuildContext, CallbackAction, Intent, IntoWidget, PageRouteBuilder,
    RouteSettingsRef, State, StateData, StatefulWidget, View, WidgetRef, WidgetsApp,
};
use waft_winui::AccentPalette;

use crate::grid_painter::TextSettings;
use crate::session::{self, Session, Stage, hang_up};
use crate::session_page::SessionPage;

/// The app's name, a window's title until its session has one.
pub(crate) const APP_NAME: &str = "Terminal";

/// The size a window opens at, in logical points: room for 80 columns and 24 rows of the
/// terminal's text at its first size.
const OPENING_SIZE: [f64; 2] = [660.0, 400.0];

/// The smallest the user may make a window, in logical points.
const MIN_SIZE: Size = Size::new(240.0, 120.0);

/// Asks the window with the focus to close.
#[derive(Debug)]
pub(crate) struct CloseWindowIntent;

impl Intent for CloseWindowIntent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Asks WaOS for another window, which shows a session of its own once it arrives.
pub(crate) fn open_window(app: &mut App) {
    let config = WindowConfig {
        title: APP_NAME.to_owned(),
        size: OPENING_SIZE,
        ..WindowConfig::default()
    };
    if let Err(error) = app.platform().request_view(config, None) {
        tracing::warn!(%error, "no window for a new session");
    }
}

/// A session of its own, in the window the widget is in, its terminal drawn in the font `text`
/// holds.
pub(crate) struct SessionWindow {
    pub(crate) text: Entity<TextSettings>,
}

impl fmt::Debug for SessionWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionWindow").finish_non_exhaustive()
    }
}

impl StatefulWidget for SessionWindow {
    type State = SessionWindowState;

    fn create_state(&self) -> SessionWindowState {
        SessionWindowState {
            state: StateData::new(),
            session: None,
            window: None,
            titled: None,
            actions: HashMap::new(),
            following: None,
        }
    }
}

pub(crate) struct SessionWindowState {
    state: StateData<SessionWindow>,
    /// The session the window shows, opened as the window arrived.
    session: Option<Entity<Session>>,
    /// The window around the view the widget is in, where it is in one.
    window: Option<WindowRef>,
    /// The title the window was given last.
    titled: Option<String>,
    /// What Close Window does here.
    actions: HashMap<TypeId, ActionRef>,
    /// Keeps the window following its session's title and end.
    following: Option<Subscription>,
}

impl SessionWindowState {
    fn session(self: Handle<Self>, app: &App) -> Entity<Session> {
        let session = app.get(self).session.as_ref();
        session.expect("init_state has run").clone()
    }

    /// Titles the window as the session says, and closes it once the shell has left.
    fn follow_session(self: Handle<Self>, app: &mut App) {
        let session = self.session(app);
        let session = session.read(app);
        let left = session.stage == Stage::Left;
        let title = session.title().unwrap_or(APP_NAME).to_owned();
        let state = app.get_mut(self);
        let Some(window) = state.window.clone() else {
            return;
        };
        if left {
            window.close();
        } else if state.titled.as_ref() != Some(&title) {
            window.set_title(&title);
            state.titled = Some(title);
        }
    }

    fn close(self: Handle<Self>, app: &App) {
        if let Some(window) = &app.get(self).window {
            window.close();
        }
    }
}

impl State for SessionWindowState {
    type Widget = SessionWindow;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let close = CallbackAction::<CloseWindowIntent>::new(
            app,
            Rc::new(move |app: &mut App, _intent: &CloseWindowIntent| {
                self.close(app);
                None
            }),
        );
        let session = session::open(app);
        let following = app.observe(&session, move |app| self.follow_session(app));
        let state = app.get_mut(self);
        state.session = Some(session);
        state
            .actions
            .insert(TypeId::of::<CloseWindowIntent>(), close.into_erased());
        state.following = Some(following);
    }

    /// The window is gone: its shell is hung up, whatever still holds the session.
    fn dispose(self: Handle<Self>, app: &mut App) {
        let session = self.session(app);
        hang_up(app, &session);
        let state = app.get_mut(self);
        state.session = None;
        state.following = None;
        state.actions.clear();
    }

    /// The view's metrics change at each resize: only another window is limited and titled anew.
    fn did_change_dependencies(self: Handle<Self>, app: &mut App) {
        let context = self.context(app);
        let window = View::maybe_of(app, context).and_then(|view| view.window());
        let same_window = match (&app.get(self).window, &window) {
            (Some(was), Some(is)) => Rc::ptr_eq(was, is),
            (was, is) => was.is_none() && is.is_none(),
        };
        if same_window {
            return;
        }
        if let Some(limits) = window.as_ref().and_then(|window| window.size_limits()) {
            limits.set_min_size(Some(MIN_SIZE));
        }
        let state = app.get_mut(self);
        state.window = window;
        state.titled = None;
        self.follow_session(app);
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let page = SessionPage {
            session: self.session(app),
            text: self.widget(app).text.clone(),
        };
        Actions::new(app.get(self).actions.clone(), widgets_app(page)).into_widget()
    }
}

/// The window's app: the navigator its page is the one route of, in Fluent's accent.
fn widgets_app(page: SessionPage) -> WidgetRef {
    WidgetsApp::new(AccentPalette::default().base)
        .title(APP_NAME)
        .debug_show_checked_mode_banner(false)
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            route.into_erased()
        })
        .home(page)
        .into_widget()
}
