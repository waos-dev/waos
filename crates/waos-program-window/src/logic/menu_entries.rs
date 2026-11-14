//! A program's menu, the WIT's flat list, as Waft's tree: each entry under the submenu it names
//! as its parent, in the list's order, keeping the id the program gave it, which a choice comes
//! back by.
//!
//! The invariant: what no menu can show is dropped, never shown somewhere else: an entry whose
//! parent is no submenu of the list, and a submenu inside itself, which a list naming one id twice
//! makes.

use std::sync::Arc;

use waft::ui::{self, CheckState, IconImage, MenuItemCheck, MenuShortcut, ShortcutModifiers};
use waos::bindings::wasdk::platform::menus::{
    self, Image, ItemCheck, MenuEntry, MenuEntryKind, MenuItem, Provided, ProvidedItem, Shortcut,
    Submenu,
};

/// `entries` as Waft's tree: the top level's, each with what it holds.
pub fn waft_menu(entries: &[MenuEntry]) -> Vec<ui::MenuEntry> {
    entries_in(entries, None, &mut Vec::new())
}

/// The entries of `entries` in the submenu `parent`, each with what it holds; `inside` are the
/// submenus around them.
fn entries_in(
    entries: &[MenuEntry],
    parent: Option<u32>,
    inside: &mut Vec<u32>,
) -> Vec<ui::MenuEntry> {
    entries
        .iter()
        .filter(|entry| entry.parent == parent)
        .filter_map(|entry| waft_entry(entries, entry, inside))
        .collect()
}

/// `entry` of `entries` as Waft's; none for a submenu inside itself.
fn waft_entry(
    entries: &[MenuEntry],
    entry: &MenuEntry,
    inside: &mut Vec<u32>,
) -> Option<ui::MenuEntry> {
    let id = entry.id;
    Some(match &entry.kind {
        MenuEntryKind::Submenu(Submenu { label, enabled }) => {
            if inside.contains(&id) {
                return None;
            }
            inside.push(id);
            let held = entries_in(entries, Some(id), inside);
            inside.pop();
            ui::MenuEntry::Submenu {
                id,
                label: label.clone(),
                enabled: *enabled,
                entries: held,
            }
        }
        MenuEntryKind::Item(item) => ui::MenuEntry::Item {
            id,
            item: waft_item(item),
        },
        MenuEntryKind::Provided(Provided { item, enabled }) => ui::MenuEntry::Provided {
            id,
            item: waft_provided_item(*item),
            enabled: *enabled,
        },
        MenuEntryKind::Separator => ui::MenuEntry::Separator,
    })
}

fn waft_item(item: &MenuItem) -> ui::MenuItem {
    ui::MenuItem {
        label: item.label.clone(),
        enabled: item.enabled,
        check: waft_check(&item.check),
        shortcut: item.shortcut.as_ref().map(waft_shortcut),
        icon: item.icon.as_ref().map(waft_image),
    }
}

fn waft_check(check: &ItemCheck) -> MenuItemCheck {
    match check {
        ItemCheck::Plain => MenuItemCheck::Plain,
        ItemCheck::Checkbox(state) => MenuItemCheck::Checkbox(match state {
            menus::CheckState::Off => CheckState::Off,
            menus::CheckState::On => CheckState::On,
            menus::CheckState::Mixed => CheckState::Mixed,
        }),
        ItemCheck::Radio(on) => MenuItemCheck::Radio(*on),
    }
}

fn waft_shortcut(shortcut: &Shortcut) -> MenuShortcut {
    let held = shortcut.modifiers;
    MenuShortcut {
        key: shortcut.logical,
        modifiers: ShortcutModifiers {
            meta: held.contains(menus::ShortcutModifiers::META),
            control: held.contains(menus::ShortcutModifiers::CONTROL),
            alt: held.contains(menus::ShortcutModifiers::ALT),
            shift: held.contains(menus::ShortcutModifiers::SHIFT),
        },
    }
}

/// `image`, as Waft shows it: the same pixels.
pub fn waft_image(image: &Image) -> IconImage {
    IconImage {
        width: image.width,
        height: image.height,
        rgba: Arc::from(image.rgba.as_slice()),
        template: image.template,
    }
}

fn waft_provided_item(item: ProvidedItem) -> ui::ProvidedItem {
    match item {
        ProvidedItem::About => ui::ProvidedItem::About,
        ProvidedItem::Services => ui::ProvidedItem::Services,
        ProvidedItem::Hide => ui::ProvidedItem::Hide,
        ProvidedItem::HideOthers => ui::ProvidedItem::HideOthers,
        ProvidedItem::ShowAll => ui::ProvidedItem::ShowAll,
        ProvidedItem::Quit => ui::ProvidedItem::Quit,
        ProvidedItem::Minimize => ui::ProvidedItem::Minimize,
        ProvidedItem::Zoom => ui::ProvidedItem::Zoom,
        ProvidedItem::ToggleFullScreen => ui::ProvidedItem::ToggleFullScreen,
        ProvidedItem::BringAllToFront => ui::ProvidedItem::BringAllToFront,
        ProvidedItem::Undo => ui::ProvidedItem::Undo,
        ProvidedItem::Redo => ui::ProvidedItem::Redo,
        ProvidedItem::Cut => ui::ProvidedItem::Cut,
        ProvidedItem::Copy => ui::ProvidedItem::Copy,
        ProvidedItem::Paste => ui::ProvidedItem::Paste,
        ProvidedItem::SelectAll => ui::ProvidedItem::SelectAll,
        ProvidedItem::Delete => ui::ProvidedItem::Delete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submenu(id: u32, parent: Option<u32>, label: &str) -> MenuEntry {
        let submenu = Submenu {
            label: label.to_owned(),
            enabled: true,
        };
        entry(id, parent, MenuEntryKind::Submenu(submenu))
    }

    fn item(id: u32, parent: Option<u32>, label: &str) -> MenuEntry {
        let item = MenuItem {
            label: label.to_owned(),
            enabled: true,
            check: ItemCheck::Plain,
            shortcut: None,
            icon: None,
        };
        entry(id, parent, MenuEntryKind::Item(item))
    }

    fn provided(id: u32, parent: Option<u32>, item: ProvidedItem) -> MenuEntry {
        let provided = Provided {
            item,
            enabled: true,
        };
        entry(id, parent, MenuEntryKind::Provided(provided))
    }

    fn entry(id: u32, parent: Option<u32>, kind: MenuEntryKind) -> MenuEntry {
        MenuEntry { id, parent, kind }
    }

    #[test]
    fn each_entry_goes_under_the_submenu_it_names_in_the_lists_order() {
        let flat = [
            submenu(1, None, "File"),
            submenu(2, None, "Edit"),
            item(3, Some(1), "Open"),
            entry(4, Some(1), MenuEntryKind::Separator),
            provided(5, Some(2), ProvidedItem::Copy),
            item(6, Some(1), "Close"),
        ];
        let tree = waft_menu(&flat);
        let ui::MenuEntry::Submenu { entries: file, .. } = &tree[0] else {
            panic!("File first: {tree:?}");
        };
        assert_eq!(
            file,
            &[
                ui::MenuEntry::Item {
                    id: 3,
                    item: ui::MenuItem::new("Open")
                },
                ui::MenuEntry::Separator,
                ui::MenuEntry::Item {
                    id: 6,
                    item: ui::MenuItem::new("Close")
                },
            ]
        );
        assert!(matches!(
            &tree[1],
            ui::MenuEntry::Submenu { entries, .. } if entries == &[ui::MenuEntry::Provided {
                id: 5,
                item: ui::ProvidedItem::Copy,
                enabled: true,
            }]
        ));
    }

    #[test]
    fn what_no_menu_can_show_is_dropped() {
        let flat = [
            submenu(1, None, "File"),
            item(2, Some(9), "Nowhere"),
            submenu(1, Some(1), "File again"),
        ];
        let tree = waft_menu(&flat);
        assert!(matches!(
            &tree[..],
            [ui::MenuEntry::Submenu { entries, .. }] if entries.is_empty()
        ));
    }

    #[test]
    fn a_shortcut_keeps_its_key_and_modifiers() {
        let shortcut = Shortcut {
            logical: u64::from('g'),
            modifiers: menus::ShortcutModifiers::META | menus::ShortcutModifiers::ALT,
        };
        let carried = waft_shortcut(&shortcut);
        assert_eq!(carried.key, u64::from('g'));
        assert_eq!(
            carried.modifiers,
            ShortcutModifiers {
                meta: true,
                alt: true,
                ..ShortcutModifiers::default()
            }
        );
    }
}
