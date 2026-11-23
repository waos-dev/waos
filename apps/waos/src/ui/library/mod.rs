//! The library's widgets, over the one library state: the library in a window, in the layout its
//! width takes (`logic::library_layout`), Fluent's or Cupertino's. Each screen is a module of its
//! own, with a submodule per control set and what is not about controls shared above them: the
//! home of program icons (`home`), an icon's actions (`icon_actions`), getting programs
//! (`get_programs`), what the library asks or says, as a dialog over what shows (`questions`), and
//! the settings pages (`settings`); with a program's icon (`program_icon`), the banner of who
//! records (`recording`), and the words every layout says (`words`).
//!
//! The invariant: every layout shows the same state and changes it through the same calls; what a
//! layout keeps of its own is only what the user is typing and where they are in it, which a
//! change of layout drops.

pub(crate) mod cupertino;
pub(crate) mod fluent;
pub(crate) mod get_programs;
pub(crate) mod home;
pub(crate) mod icon_actions;
pub(crate) mod program_icon;
pub(crate) mod questions;
pub(crate) mod recording;
#[cfg(test)]
mod recording_tests;
mod settings;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
pub(crate) mod test_steps;
#[cfg(test)]
mod tests;
pub(crate) mod words;

use waft::{
    App, BuildContext, Entity, IntoWidget, KeyRef, MediaQuery, PageRef, StatelessWidget, ValueKey,
    WidgetRef,
};

use crate::logic::library_layout::LibraryLayout;
use crate::logic::setting_changes::SettingsOf;
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;
use cupertino::CupertinoLibrary;
use fluent::FluentLibrary;

/// The library in a window, laid out for the window's width, with the programs recording.
#[derive(Clone, Debug)]
pub(crate) struct LaidOutLibrary {
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
    /// Whether the library shows what it asks over itself, as in the panel; not where what holds
    /// it shows that over more, as the deck shows it over the home and the tabs alike.
    pub(crate) questions_here: bool,
}

impl StatelessWidget for LaidOutLibrary {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let width = MediaQuery::size_of(app, context).width();
        let (library, recordings) = (self.library.clone(), self.recordings.clone());
        match LibraryLayout::for_width(width) {
            LibraryLayout::Fluent => FluentLibrary {
                library,
                recordings,
                dismissed: None,
                questions_here: self.questions_here,
                home: None,
            }
            .into_widget(),
            LibraryLayout::Cupertino => CupertinoLibrary {
                library,
                recordings,
                questions_here: self.questions_here,
            }
            .into_widget(),
        }
    }
}
/// The pages a kit's navigator stacks while the library is on `shown`: the home at the bottom, and
/// over it `shown` when that is another page, as iOS stacks a page pushed from the home.
fn stacked_pages(shown: &LibraryPage) -> Vec<LibraryPage> {
    let mut pages = vec![LibraryPage::Home];
    if *shown != LibraryPage::Home {
        pages.push(shown.clone());
    }
    pages
}

/// The key of the navigator's page that shows `page`: the library's pages tell apart as the
/// library tells them, so a page given again updates the one shown.
fn page_key(page: &LibraryPage) -> KeyRef {
    ValueKey::new(page.clone()).into()
}

/// What a kit's navigator calls when it removes a page by itself, as a swipe back does: when the
/// library was on that page, it goes home, so its pages no longer list it, as the navigator asks.
fn home_once_removed(library: &Entity<Library>) -> impl Fn(&mut App, PageRef) + 'static {
    let library = library.clone();
    move |app, removed| {
        let shown = page_key(library.read(app).page());
        if removed.key() == Some(&shown) {
            library.update(app, |library, cx| library.show_page(LibraryPage::Home, cx));
        }
    }
}

/// The name of the program whose settings `of` shows, as its user knows it, or its id once it is
/// no longer installed; none on the global page.
fn program_name(library: &Entity<Library>, of: &SettingsOf, app: &App) -> Option<String> {
    let SettingsOf::Program(id) = of else {
        return None;
    };
    let installed = library
        .read(app)
        .installed()
        .iter()
        .find(|program| program.manifest.id() == id)
        .map(|program| program.name());
    Some(installed.unwrap_or_else(|| id.to_string()))
}
