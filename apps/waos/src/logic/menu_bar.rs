//! WaOS's own menu bar: its app menu, Edit and Window, every item the platform's, which macOS shows
//! while WaOS is in front. A program's bar is its app's, which macOS shows while the program's
//! window is in front, so WaOS's bar never holds a program's menus.
//!
//! The invariant: every item is the platform's, which carries it out itself, so no choice in the
//! bar is WaOS's to carry out, and its entries need no ids of their own: each is 0, as those of
//! Waft's standard bar are.

use waft::ui::{MenuEntry, ProvidedItem};

/// WaOS's bar: its app menu, Edit and Window.
pub(crate) fn waos_bar() -> Vec<MenuEntry> {
    vec![
        provided_menu("WaOS", &APP_ITEMS),
        provided_menu("Edit", &EDIT_ITEMS),
        provided_menu("Window", &WINDOW_ITEMS),
    ]
}

/// The app menu's items, a separator where none is given.
const APP_ITEMS: [Option<ProvidedItem>; 9] = [
    Some(ProvidedItem::About),
    None,
    Some(ProvidedItem::Services),
    None,
    Some(ProvidedItem::Hide),
    Some(ProvidedItem::HideOthers),
    Some(ProvidedItem::ShowAll),
    None,
    Some(ProvidedItem::Quit),
];

/// Edit's items, a separator where none is given.
const EDIT_ITEMS: [Option<ProvidedItem>; 8] = [
    Some(ProvidedItem::Undo),
    Some(ProvidedItem::Redo),
    None,
    Some(ProvidedItem::Cut),
    Some(ProvidedItem::Copy),
    Some(ProvidedItem::Paste),
    Some(ProvidedItem::Delete),
    Some(ProvidedItem::SelectAll),
];

/// Window's items, a separator where none is given.
const WINDOW_ITEMS: [Option<ProvidedItem>; 4] = [
    Some(ProvidedItem::Minimize),
    Some(ProvidedItem::Zoom),
    None,
    Some(ProvidedItem::BringAllToFront),
];

/// The menu titled `label` of `items`, the platform's, a separator where none is given.
fn provided_menu(label: &str, items: &[Option<ProvidedItem>]) -> MenuEntry {
    let entries = items
        .iter()
        .map(|item| match item {
            Some(item) => MenuEntry::Provided {
                id: 0,
                item: *item,
                enabled: true,
            },
            None => MenuEntry::Separator,
        })
        .collect();
    MenuEntry::Submenu {
        id: 0,
        label: label.to_owned(),
        enabled: true,
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waos_bar_is_its_app_menu_edit_and_window_every_item_the_platforms() {
        let bar = waos_bar();
        let titles: Vec<&str> = bar
            .iter()
            .map(|menu| match menu {
                MenuEntry::Submenu { label, .. } => label.as_str(),
                _ => panic!("a top-level menu: {menu:?}"),
            })
            .collect();
        assert_eq!(titles, ["WaOS", "Edit", "Window"]);
        for menu in &bar {
            let MenuEntry::Submenu { entries, .. } = menu else {
                unreachable!("a top-level menu");
            };
            assert!(
                entries.iter().all(|entry| matches!(
                    entry,
                    MenuEntry::Provided { .. } | MenuEntry::Separator
                )),
                "{entries:?}"
            );
        }
    }
}
