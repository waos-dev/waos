//! The library at a desktop's or a tablet's size, in Fluent's controls (waft-winui): the page the
//! library is on, on Fluent's solid background: the home, its own or the one it is given, as the
//! desktop's launcher gives its search row over the grid, getting programs, or a settings page,
//! the global one or a program's, each with Back to the home; and, where the library shows its
//! own questions, what it asks or says as a dialog over them. Escape does `dismissed`, where
//! given.
//!
//! What it shows is the library's, read by the widget that shows each page.

use std::rc::Rc;

use waft::{
    App, BuildContext, ColoredBox, Entity, Focus, IntoWidget, KeyEventResult, Listener,
    StatelessWidget, ValueKey, WidgetRef, WidgetsApp,
};
use waft_services::{KeyEvent, LogicalKeyboardKey};
use waft_widgets::{PageRoute, PageRouteBuilder, RouteSettingsRef};
use waft_winui::{AccentPalette, ThemeResources};

use super::get_programs::fluent::FluentGetPrograms;
use super::home::fluent::FluentHome;
use super::program_name;
use super::questions::fluent::FluentAsked;
use super::settings::fluent::FluentSettings;
use super::words;
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;

/// The library in Fluent's controls; Escape in it does `dismissed`, where given.
#[derive(Debug)]
pub(crate) struct FluentLibrary {
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
    pub(crate) dismissed: Option<Listener>,
    /// Whether the library shows what it asks over itself; not where what holds it shows that over
    /// more, as the deck does.
    pub(crate) questions_here: bool,
    /// What the home shows in place of the title bar over the grid, where given.
    pub(crate) home: Option<WidgetRef>,
}

impl StatelessWidget for FluentLibrary {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let page = LibraryPages {
            library: self.library.clone(),
            recordings: self.recordings.clone(),
            dismissed: self.dismissed.clone(),
            home: self.home.clone(),
        };
        let home = if self.questions_here {
            FluentAsked {
                library: self.library.clone(),
                child: page.into_widget(),
            }
            .into_widget()
        } else {
            page.into_widget()
        };
        WidgetsApp::new(AccentPalette::default().base)
            .title(words::HOME_TITLE)
            .debug_show_checked_mode_banner(false)
            .page_route_builder(|app, settings, builder| {
                let route = PageRouteBuilder::new(
                    app,
                    Rc::new(move |app, context, _, _| builder(app, context)),
                )
                .settings(app, RouteSettingsRef::Settings(settings.clone()));
                PageRoute::as_page_route(route)
            })
            .home(home)
            .into_widget()
    }
}

/// The page the library is on, on Fluent's background.
#[derive(Debug)]
struct LibraryPages {
    library: Entity<Library>,
    recordings: Entity<Recordings>,
    dismissed: Option<Listener>,
    home: Option<WidgetRef>,
}

impl StatelessWidget for LibraryPages {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let background = ThemeResources::of(app, context).solid_background_fill_color_base;
        let library = self.library.clone();
        let shown = library.read(app).page().clone();
        let content = match shown.clone() {
            LibraryPage::Home => self.home.clone().unwrap_or_else(|| {
                FluentHome {
                    library,
                    recordings: self.recordings.clone(),
                }
                .into_widget()
            }),
            LibraryPage::GetPrograms => FluentGetPrograms { library }.into_widget(),
            LibraryPage::Settings(of) => FluentSettings {
                program_name: program_name(&library, &of, app),
                library,
                of,
            }
            .into_widget(),
        };
        // Keyed by the page, so each page takes the focus as it shows, and Escape reaches it there.
        let page = Focus::new(ColoredBox::new(background).child(content))
            .key(ValueKey::new(shown).into())
            .autofocus(true);
        match self.dismissed.clone() {
            Some(dismissed) => page
                .on_key_event(Rc::new(move |app, _, event| {
                    escape_does(&dismissed, app, event)
                }))
                .into_widget(),
            None => page.into_widget(),
        }
    }
}

/// Escape pressed does `dismissed`; every other key goes on.
fn escape_does(dismissed: &Listener, app: &mut App, event: &KeyEvent) -> KeyEventResult {
    if matches!(event, KeyEvent::Down(_)) && event.logical_key() == LogicalKeyboardKey::ESCAPE {
        dismissed.call(app);
        KeyEventResult::Handled
    } else {
        KeyEventResult::Ignored
    }
}
