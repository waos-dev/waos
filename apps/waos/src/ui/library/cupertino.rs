//! The library at a phone's size, in Cupertino's widgets: the page the library is on, under an app
//! of its own: the home, getting programs, or a settings page, the global one or a program's, each
//! pushed over the home with a way back to it, as iOS slides a page in and swipes it back; and,
//! where the library shows its own questions, what it asks or says as an alert over them.
//!
//! What it shows is the library's, read by the widget that shows each page.

use std::rc::Rc;

use waft::{
    App, BuildContext, Builder, Clip, EdgeInsetsGeometry, Entity, Handle, HeroController,
    HeroControllerScope, Icon, IntoWidget, Listener, MainAxisSize, MediaQuery, Navigator, PageRef,
    Ref, Row, State, StateData, StatefulWidget, StatelessWidget, Text, WidgetRef,
};
use waft_cupertino::{CupertinoApp, CupertinoButton, CupertinoIcons, CupertinoPage};

use super::get_programs::cupertino::CupertinoGetPrograms;
use super::home::cupertino::CupertinoHome;
use super::program_name;
use super::questions::cupertino::CupertinoAsked;
use super::settings::cupertino::CupertinoSettings;
use super::{back_once_removed, page_key, words};
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;

/// The library in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoLibrary {
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
    /// Whether the library shows what it asks over itself; not where what holds it shows that over
    /// more, as the deck does.
    pub(crate) questions_here: bool,
}

impl StatelessWidget for CupertinoLibrary {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let page = LibraryPages {
            library: self.library.clone(),
            recordings: self.recordings.clone(),
        };
        let home = if self.questions_here {
            CupertinoAsked {
                library: self.library.clone(),
                child: page.into_widget(),
            }
            .into_widget()
        } else {
            page.into_widget()
        };
        CupertinoApp::new()
            .title(words::HOME_TITLE)
            .home(home)
            .into_widget()
    }
}

/// The library's pages, as a navigator of Cupertino's pages: the home, and the page the library
/// is on over it. A change of page slides in or out as iOS's do, the navigation bars' titles moving
/// between them, and a swipe back takes the library home. It clips nothing, as an app's root
/// navigator doesn't: it spans the screen's width and a page slides in sideways, so what a clip
/// would cut is off the screen already.
#[derive(Clone, Debug)]
struct LibraryPages {
    library: Entity<Library>,
    recordings: Entity<Recordings>,
}

impl StatefulWidget for LibraryPages {
    type State = LibraryPagesState;

    fn create_state(&self) -> LibraryPagesState {
        LibraryPagesState {
            state: StateData::new(),
            hero_controller: None,
        }
    }
}

/// The navigator's own hero controller, which moves the navigation bars' titles between pages: a
/// navigator inside an app has none of the app's, so it makes one, as Cupertino's tab view does.
struct LibraryPagesState {
    state: StateData<LibraryPages>,
    hero_controller: Option<Ref<HeroController>>,
}

impl State for LibraryPagesState {
    type Widget = LibraryPages;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let hero_controller = CupertinoApp::create_cupertino_hero_controller(app);
        app.get_mut(self).hero_controller = Some(hero_controller);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(hero_controller) = app.get_mut(self).hero_controller.take() {
            hero_controller.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let widget = self.widget(app).clone();
        let shown = widget.library.read(app).page().clone();
        let pages: Vec<PageRef> = shown
            .stacked()
            .into_iter()
            .map(|page| widget.page(app, page))
            .collect();
        let navigator = Navigator::new()
            .clip_behavior(Clip::None)
            .pages(pages)
            .on_did_remove_page(back_once_removed(&widget.library));
        let hero_controller = app
            .get(self)
            .hero_controller
            .clone()
            .expect("init_state makes the controller");
        HeroControllerScope::new(hero_controller, navigator).into_widget()
    }
}

impl LibraryPages {
    /// The navigator's page that shows `page`.
    fn page(&self, app: &mut App, page: LibraryPage) -> PageRef {
        let key = page_key(&page);
        Rc::new(CupertinoPage::new(self.content(app, page)).key(key))
    }

    /// What `page` shows.
    fn content(&self, app: &mut App, page: LibraryPage) -> WidgetRef {
        let library = self.library.clone();
        match page {
            LibraryPage::Home => CupertinoHome {
                library,
                recordings: self.recordings.clone(),
            }
            .into_widget(),
            LibraryPage::GetPrograms => CupertinoGetPrograms { library }.into_widget(),
            LibraryPage::Settings(of) => CupertinoSettings {
                program_name: program_name(&library, &of, app),
                library,
                of,
            }
            .into_widget(),
        }
    }
}

/// What `build` makes of how far down the page's navigation bar, and the system's bar above it,
/// reach: the top padding `CupertinoPageScaffold` reports to what is inside it, so a page under a
/// translucent bar starts below it and scrolls beneath it.
pub(crate) fn under_bar(build: impl Fn(f64) -> WidgetRef + 'static) -> WidgetRef {
    Builder::new(move |app, context| build(MediaQuery::padding_of(app, context).top)).into_widget()
}

/// The navigation bar's way back to the home, named after it, as iOS names a back button after
/// the page it goes back to.
pub(crate) fn back_to_home(library: &Entity<Library>) -> WidgetRef {
    let library = library.clone();
    let back = Listener::new(move |app| {
        library.update(app, |library, cx| library.show_page(LibraryPage::Home, cx));
    });
    let label = Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(2.0)
        .children([
            Icon::new(Some(CupertinoIcons::back())).into_widget(),
            Text::new(words::HOME_TITLE).into_widget(),
        ]);
    CupertinoButton::new(label.into_widget(), Some(back))
        .padding(EdgeInsetsGeometry::all(0.0))
        .into_widget()
}
