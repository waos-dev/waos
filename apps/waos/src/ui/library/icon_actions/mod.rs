//! What can be done to a program from its icon on the home, offered by holding the icon on a phone
//! (`cupertino`, an action sheet) and by a right click on a desktop (`fluent`, a menu): putting it
//! on the home screen where the launcher pins shortcuts, on Android, its settings, closing it while
//! it runs where the library closes programs, and removing it, which still asks. What is shared between the layouts is here: which actions a program is offered now,
//! their words, and carrying one out through the library.
//!
//! The invariant: the actions offered are read from the library as the sheet or the menu opens,
//! and an action carried out goes through the library's own calls, so removing asks as the
//! library asks.

pub(crate) mod cupertino;
pub(crate) mod fluent;

use waft::{App, Entity};
use waos_package::ProgramId;

use super::words;
use crate::logic::setting_changes::SettingsOf;
use crate::state::library::{Library, LibraryPage};

/// An action on a program, from its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconAction {
    /// Asks the launcher to put it on the home screen, which asks the user.
    AddToHomeScreen,
    /// Opens its settings page.
    Settings,
    /// Closes it, which stops it without asking.
    Close,
    /// Asks whether to remove it.
    Remove,
}

impl IconAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            IconAction::AddToHomeScreen => words::ADD_TO_HOME_SCREEN,
            IconAction::Settings => words::SETTINGS,
            IconAction::Close => words::CLOSE,
            IconAction::Remove => words::REMOVE,
        }
    }

    /// Does the action to the program `id` through `library`.
    pub(crate) fn carry_out(self, library: &Entity<Library>, id: &ProgramId, app: &mut App) {
        library.update(app, |library, cx| match self {
            IconAction::AddToHomeScreen => library.add_to_home_screen(id, cx),
            IconAction::Settings => {
                library.show_page(LibraryPage::Settings(SettingsOf::Program(id.clone())), cx);
            }
            IconAction::Close => library.close(id, cx),
            IconAction::Remove => library.ask_to_remove(id, cx),
        });
    }
}

/// The actions the program `id` is offered now: Add to Home Screen, where the launcher pins
/// shortcuts; its settings; Close while it runs, where the library closes programs; and Remove.
pub(crate) fn actions_offered(library: &Library, id: &ProgramId) -> Vec<IconAction> {
    let closes = library.closes_programs() && library.is_running(id);
    [
        library
            .adds_to_home_screen(id)
            .then_some(IconAction::AddToHomeScreen),
        Some(IconAction::Settings),
        closes.then_some(IconAction::Close),
        Some(IconAction::Remove),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The program `id`'s name and version, as its actions are titled; none once it is no longer
/// installed.
pub(crate) fn name_and_version(library: &Library, id: &ProgramId) -> Option<(String, String)> {
    let program = library
        .installed()
        .iter()
        .find(|program| program.manifest.id() == id)?;
    Some((
        program.manifest.name().to_owned(),
        program.manifest.version().to_string(),
    ))
}
