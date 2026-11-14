//! The menu bar: the app's menu, the platform's own items; Shell, which opens and closes
//! windows; Edit, the platform's Copy, Paste and Select All, which reach the terminal with the
//! focus as they reach any text, and Clear; View, the size of the text; and Window, the
//! platform's items.
//!
//! Control with a letter is the shell's, so every command's key is with Command, which the
//! terminal leaves to the menus.

use std::rc::Rc;

use waft_foundation::{App, Entity};
use waft_services::LogicalKeyboardKey as Key;
use waft_widgets::{
    IntentRef, PlainPlatformMenuItem, PlatformMenu, PlatformMenuItemGroup, PlatformMenuItemRef,
    PlatformProvidedMenuItem, PlatformProvidedMenuItemType as Provided, SingleActivator,
};

use crate::grid_painter::TextSettings;
use crate::terminal_view::ClearIntent;
use crate::text_size::{TextSizeStep, step_text_size};
use crate::window::{APP_NAME, CloseWindowIntent, open_window};

/// The menu bar, whose View menu sizes the text `text` holds.
pub(crate) fn menu_bar(text: &Entity<TextSettings>) -> Vec<PlatformMenuItemRef> {
    vec![
        app_menu(),
        shell_menu(),
        edit_menu(),
        view_menu(text),
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

fn shell_menu() -> PlatformMenuItemRef {
    let new_window = PlainPlatformMenuItem::new("New Window")
        .on_selected(Rc::new(open_window))
        .shortcut(Rc::new(command_key(Key::KEY_N)));
    menu(
        "Shell",
        vec![
            group(vec![Rc::new(new_window)]),
            group(vec![intent_item(
                "Close Window",
                command_key(Key::KEY_W),
                Rc::new(CloseWindowIntent),
            )]),
        ],
    )
}

fn edit_menu() -> PlatformMenuItemRef {
    menu(
        "Edit",
        vec![
            group(vec![
                provided(Provided::Copy),
                provided(Provided::Paste),
                provided(Provided::SelectAll),
            ]),
            group(vec![intent_item(
                "Clear",
                command_key(Key::KEY_K),
                Rc::new(ClearIntent),
            )]),
        ],
    )
}

fn view_menu(text: &Entity<TextSettings>) -> PlatformMenuItemRef {
    let item = |label, key, step| size_item(text, label, key, step);
    menu(
        "View",
        vec![group(vec![
            item("Bigger", Key::EQUAL, TextSizeStep::Bigger),
            item("Smaller", Key::MINUS, TextSizeStep::Smaller),
            item("Default Size", Key::DIGIT0, TextSizeStep::Default),
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

/// ⌘ and `key`.
fn command_key(key: Key) -> SingleActivator {
    SingleActivator::new(key).meta(true)
}

/// An item labelled `label`, chosen with `key`, that sends `intent` to what has the focus.
fn intent_item(label: &str, key: SingleActivator, intent: IntentRef) -> PlatformMenuItemRef {
    Rc::new(
        PlainPlatformMenuItem::new(label)
            .on_selected_intent(intent)
            .shortcut(Rc::new(key)),
    )
}

/// An item labelled `label`, chosen with ⌘ and `key`, that sizes the text of `text` by `step`.
fn size_item(
    text: &Entity<TextSettings>,
    label: &str,
    key: Key,
    step: TextSizeStep,
) -> PlatformMenuItemRef {
    let text = text.clone();
    Rc::new(
        PlainPlatformMenuItem::new(label)
            .on_selected(Rc::new(move |app: &mut App| {
                step_text_size(app, &text, step)
            }))
            .shortcut(Rc::new(command_key(key))),
    )
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
