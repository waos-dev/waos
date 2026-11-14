//! The menu bar: the app's menu, the platform's own items; File and View, the app's commands
//! ([`crate::commands`]); Edit, the platform's items, which reach the focused text as a native
//! app's do; and Window, the platform's items, which make it the menu the system lists the
//! window in.

use std::rc::Rc;

use waft::{
    App, PlainPlatformMenuItem, PlatformMenu, PlatformMenuItemGroup, PlatformMenuItemRef,
    PlatformProvidedMenuItem, PlatformProvidedMenuItemType as Provided,
};

use crate::commands::{MenuCommand, file_groups, view_groups};
use crate::documents_app::DocumentsApp;
use crate::window::APP_NAME;

/// The menu bar, whose commands act on `documents`.
pub(crate) fn menu_bar(app: &App, documents: &DocumentsApp) -> Vec<PlatformMenuItemRef> {
    vec![
        app_menu(),
        commands_menu(app, documents, "File", file_groups()),
        edit_menu(),
        commands_menu(app, documents, "View", view_groups()),
        window_menu(),
    ]
}

fn app_menu() -> PlatformMenuItemRef {
    menu(
        APP_NAME,
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

fn commands_menu(
    app: &App,
    documents: &DocumentsApp,
    label: &str,
    groups: Vec<Vec<MenuCommand>>,
) -> PlatformMenuItemRef {
    let groups = groups.into_iter().map(|commands| {
        let items = commands
            .into_iter()
            .map(|command| item(app, documents, command));
        group(items.collect())
    });
    menu(label, groups.collect())
}

fn edit_menu() -> PlatformMenuItemRef {
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
        ],
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

/// An item that does `command` on `documents`, showing its key where it has one.
fn item(app: &App, documents: &DocumentsApp, command: MenuCommand) -> PlatformMenuItemRef {
    let documents = documents.clone();
    let run = command.run;
    let item = PlainPlatformMenuItem::new(command.label)
        .on_selected(Rc::new(move |app: &mut App| run(app, &documents)));
    match command.chord {
        Some(chord) => Rc::new(item.shortcut(Rc::new(chord.activator(app)))),
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
