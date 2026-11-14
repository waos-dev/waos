//! The menu bar: the app's menu, the platform's own items; File, whose commands make notes, open
//! and close text files to edit in place, import, export, delete and recover notes, set a reminder
//! and close the window; Edit, the platform's
//! items, which reach the focused text as a native app's do, and Search; View, which lists the
//! notes or Recently Deleted; and Window, the platform's items, which make it the menu the system
//! lists the window in.
//!
//! A key the focused text takes never reaches a menu: ⌘⌫ in a note deletes to the line's start,
//! as in any text, and deletes the note only where the text does not have the focus.

use std::rc::Rc;

use waft_foundation::App;
use waft_services::LogicalKeyboardKey as Key;
use waft_widgets::{
    PlainPlatformMenuItem, PlatformMenu, PlatformMenuItemGroup, PlatformMenuItemRef,
    PlatformProvidedMenuItem, PlatformProvidedMenuItemType as Provided, SingleActivator,
};

use crate::browsing::Folder;
use crate::editing::{delete_shown, new_note, recover_shown};
use crate::exporting::export;
use crate::importing::import;
use crate::list_pane::focus_search;
use crate::notes_app::NotesApp;
use crate::opening::{close_shown_file, open_picked};
use crate::quitting::ask_to_quit;
use crate::reminders::remind_in_a_minute;
use crate::showing::show_folder;

/// What a command does, given the app.
type Command = fn(&mut App, &NotesApp);

/// The menu bar, whose commands act on `notes`.
pub(crate) fn menu_bar(notes: &NotesApp) -> Vec<PlatformMenuItemRef> {
    vec![
        app_menu(),
        file_menu(notes),
        edit_menu(notes),
        view_menu(notes),
        window_menu(),
    ]
}

fn app_menu() -> PlatformMenuItemRef {
    menu(
        "Notes",
        vec![
            group(vec![provided(Provided::About)]),
            group(vec![provided(Provided::ServicesSubmenu)]),
            group(vec![
                provided(Provided::Hide),
                provided(Provided::HideOtherApplications),
                provided(Provided::ShowAllApplications),
            ]),
            group(vec![provided(Provided::Quit)]),
        ],
    )
}

fn file_menu(notes: &NotesApp) -> PlatformMenuItemRef {
    let item = |label, key, command| command_item(notes, label, key, command);
    menu(
        "File",
        vec![
            group(vec![item(
                "New Note",
                Some(command_key(Key::KEY_N)),
                new_note,
            )]),
            group(vec![
                item("Open…", Some(command_key(Key::KEY_O)), open_picked),
                item("Close File", None, close_shown_file),
            ]),
            group(vec![
                item("Import…", None, import),
                item("Export…", None, export),
            ]),
            group(vec![
                item("Delete", Some(command_key(Key::BACKSPACE)), delete_shown),
                item("Recover", None, recover_shown),
            ]),
            group(vec![item(
                "Remind Me in 1 Minute",
                None,
                remind_in_a_minute,
            )]),
            group(vec![item("Close", Some(command_key(Key::KEY_W)), close)]),
        ],
    )
}

fn edit_menu(notes: &NotesApp) -> PlatformMenuItemRef {
    menu(
        "Edit",
        vec![
            group(vec![provided(Provided::Undo), provided(Provided::Redo)]),
            group(vec![
                provided(Provided::Cut),
                provided(Provided::Copy),
                provided(Provided::Paste),
                provided(Provided::Delete),
                provided(Provided::SelectAll),
            ]),
            group(vec![command_item(
                notes,
                "Search",
                Some(command_key(Key::KEY_F).alt(true)),
                focus_search,
            )]),
        ],
    )
}

fn view_menu(notes: &NotesApp) -> PlatformMenuItemRef {
    let item = |label, key, command| command_item(notes, label, key, command);
    menu(
        "View",
        vec![group(vec![
            item("Notes", Some(command_key(Key::DIGIT1)), list_notes),
            item(
                "Recently Deleted",
                Some(command_key(Key::DIGIT2)),
                list_recently_deleted,
            ),
        ])],
    )
}

fn window_menu() -> PlatformMenuItemRef {
    menu(
        "Window",
        vec![
            group(vec![
                provided(Provided::MinimizeWindow),
                provided(Provided::ZoomWindow),
            ]),
            group(vec![provided(Provided::ArrangeWindowsInFront)]),
        ],
    )
}

fn list_notes(app: &mut App, notes: &NotesApp) {
    show_folder(app, notes, Folder::Notes);
}

fn list_recently_deleted(app: &mut App, notes: &NotesApp) {
    show_folder(app, notes, Folder::RecentlyDeleted);
}

/// Asks to close the window, as its close button does, which quits.
fn close(app: &mut App, _notes: &NotesApp) {
    ask_to_quit(app);
}

/// ⌘ and `key`.
fn command_key(key: Key) -> SingleActivator {
    SingleActivator::new(key).meta(true)
}

/// An item labelled `label`, chosen with `key` where it has one, that runs `command` on `notes`.
fn command_item(
    notes: &NotesApp,
    label: &str,
    key: Option<SingleActivator>,
    command: Command,
) -> PlatformMenuItemRef {
    let notes = notes.clone();
    let item = PlainPlatformMenuItem::new(label)
        .on_selected(Rc::new(move |app: &mut App| command(app, &notes)));
    match key {
        Some(key) => Rc::new(item.shortcut(Rc::new(key))),
        None => Rc::new(item),
    }
}

fn menu(label: &str, items: Vec<PlatformMenuItemRef>) -> PlatformMenuItemRef {
    Rc::new(PlatformMenu::new(label, items))
}

/// `items`, with a line between them and the items around them.
fn group(items: Vec<PlatformMenuItemRef>) -> PlatformMenuItemRef {
    Rc::new(PlatformMenuItemGroup::new(items))
}

fn provided(item: Provided) -> PlatformMenuItemRef {
    Rc::new(PlatformProvidedMenuItem::new(item))
}
