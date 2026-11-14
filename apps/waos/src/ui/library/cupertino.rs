//! The library at a phone's size, in Cupertino's widgets: the page the library is on, under an app
//! of its own: the home, getting programs, or a settings page, the global one or a program's, each
//! a page of its own with a way back to the home; and, where the library shows its own questions,
//! what it asks or says as an alert over them.
//!
//! What it shows is the library's, read by the widget that shows each page.

use waft::{
    App, BuildContext, Builder, EdgeInsetsGeometry, Entity, Icon, IntoWidget, Listener,
    MainAxisSize, MediaQuery, Row, StatelessWidget, Text, WidgetRef,
};
use waft_cupertino::{CupertinoApp, CupertinoButton, CupertinoIcons};

use super::get_programs::cupertino::CupertinoGetPrograms;
use super::home::cupertino::CupertinoHome;
use super::program_name;
use super::questions::cupertino::CupertinoAsked;
use super::settings::cupertino::CupertinoSettings;
use super::words;
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

/// The page the library is on.
#[derive(Debug)]
struct LibraryPages {
    library: Entity<Library>,
    recordings: Entity<Recordings>,
}

impl StatelessWidget for LibraryPages {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let library = self.library.clone();
        match library.read(app).page().clone() {
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
