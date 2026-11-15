//! The library as the deck's home. The library is one state with a page of its own, the grid of
//! programs, getting programs or a settings page; on a phone it is what the home's tab shows, so
//! it is in sight only while the home's page is. This keeps the two together: whatever the library
//! asks to be seen for brings the deck's window forward, as on Android, where a program may be in
//! a task of its own in front; a program that asks its user to change one of its switches opens
//! its settings page in the library, and the deck goes home to show it; and once the home's page is out of sight, a program waiting for its user
//! to leave a settings page hears so, and a home opened later starts on the grid of programs.
//!
//! The invariant: the library is on a page other than its grid of programs only while a home is
//! open to show it.

use waft::{App, Entity};

use super::tabs::Tabs;
use crate::state::library::{Library, LibraryPage, WantsToBeSeen};

/// From now on the deck of `tabs` goes home when `library` has a settings page to show its user,
/// and `library` hears when the home's page goes out of sight.
pub(crate) fn keep_library_with_the_home(
    app: &mut App,
    library: &Entity<Library>,
    tabs: &Entity<Tabs>,
) {
    show_what_the_library_wants_seen(app, library, tabs);
    tell_library_when_the_home_is_out_of_sight(app, library, tabs);
}

/// The deck's window comes forward whenever the library asks to be seen, and goes home where a
/// program asked for one of its switches. What else the library asks to be seen for, a question or
/// a problem, shows as an alert over whatever page the deck is on, and moves no page.
fn show_what_the_library_wants_seen(app: &mut App, library: &Entity<Library>, tabs: &Entity<Tabs>) {
    let (asked, tabs) = (library.downgrade(), tabs.downgrade());
    app.subscribe(library, move |app, _: &WantsToBeSeen| {
        let (Some(library), Some(tabs)) = (asked.upgrade(), tabs.upgrade()) else {
            return;
        };
        tabs.read(app).bring_forward();
        if library.read(app).switch_asked().is_some() {
            tabs.update(app, |tabs, cx| {
                tabs.go_home(cx);
            });
        }
    })
    .detach();
}

fn tell_library_when_the_home_is_out_of_sight(
    app: &mut App,
    library: &Entity<Library>,
    tabs: &Entity<Tabs>,
) {
    let (library, watched) = (library.downgrade(), tabs.downgrade());
    app.observe(tabs, move |app| {
        let (Some(library), Some(tabs)) = (library.upgrade(), watched.upgrade()) else {
            return;
        };
        let list = tabs.read(app).list();
        let home = list.home();
        if home.is_some_and(|home| list.in_sight(home)) {
            return;
        }
        library.update(app, |library, cx| {
            library.hidden(cx);
            if home.is_none() {
                library.show_page(LibraryPage::Home, cx);
            }
        });
    })
    .detach();
}
