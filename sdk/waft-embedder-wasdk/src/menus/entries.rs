//! Waft's menus as the WIT's: a tree as a flat list whose entries name the submenu they are in,
//! each entry keeping the id Waft's app gave it, which a choice comes back by.
//!
//! The invariant: every entry of a flat menu has an id no other entry of it has. Waft gives a
//! separator none, so a separator takes one the menu leaves free, counted down from the last.

use std::collections::HashSet;

use waft_embedder::{
    CheckState, IconImage, MenuEntry, MenuId, MenuItem, MenuItemCheck, MenuShortcut, ProvidedItem,
};
use wasdk::menus::{self, MenuEntryKind};

/// `menu`, Waft's tree, as the WIT's flat list: each submenu before what it holds.
pub(crate) fn flat_menu(menu: &[MenuEntry]) -> Vec<menus::MenuEntry> {
    let mut separator_ids = FreeIds::left_by(menu);
    let mut flat = Vec::new();
    flatten(menu, None, &mut separator_ids, &mut flat);
    flat
}

/// Pushes `entries`, in the submenu `parent`, onto `flat`, and what each submenu holds after it.
fn flatten(
    entries: &[MenuEntry],
    parent: Option<MenuId>,
    separator_ids: &mut FreeIds,
    flat: &mut Vec<menus::MenuEntry>,
) {
    for entry in entries {
        let (id, kind) = match entry {
            MenuEntry::Submenu {
                id,
                label,
                enabled,
                entries,
            } => {
                let submenu = menus::Submenu {
                    label: label.clone(),
                    enabled: *enabled,
                };
                flat.push(flat_entry(*id, parent, MenuEntryKind::Submenu(submenu)));
                flatten(entries, Some(*id), separator_ids, flat);
                continue;
            }
            MenuEntry::Item { id, item } => (*id, MenuEntryKind::Item(platform_item(item))),
            MenuEntry::Provided { id, item, enabled } => {
                let provided = menus::Provided {
                    item: platform_provided_item(*item),
                    enabled: *enabled,
                };
                (*id, MenuEntryKind::Provided(provided))
            }
            MenuEntry::Separator => (separator_ids.take(), MenuEntryKind::Separator),
        };
        flat.push(flat_entry(id, parent, kind));
    }
}

fn flat_entry(id: MenuId, parent: Option<MenuId>, kind: MenuEntryKind) -> menus::MenuEntry {
    menus::MenuEntry { id, parent, kind }
}

/// The ids a menu leaves free, the last first.
struct FreeIds {
    used: HashSet<MenuId>,
    next: MenuId,
}

impl FreeIds {
    /// The ids no entry of `menu` has.
    fn left_by(menu: &[MenuEntry]) -> FreeIds {
        let mut used = HashSet::new();
        collect_ids(menu, &mut used);
        FreeIds {
            used,
            next: MenuId::MAX,
        }
    }

    /// The next free id; a menu with more entries than ids has none to give, and repeats one.
    fn take(&mut self) -> MenuId {
        while self.used.contains(&self.next) && self.next > 0 {
            self.next -= 1;
        }
        let id = self.next;
        self.used.insert(id);
        id
    }
}

/// Every id `entries` and their submenus give.
fn collect_ids(entries: &[MenuEntry], used: &mut HashSet<MenuId>) {
    for entry in entries {
        match entry {
            MenuEntry::Submenu { id, entries, .. } => {
                used.insert(*id);
                collect_ids(entries, used);
            }
            MenuEntry::Item { id, .. } | MenuEntry::Provided { id, .. } => {
                used.insert(*id);
            }
            MenuEntry::Separator => {}
        }
    }
}

fn platform_item(item: &MenuItem) -> menus::MenuItem {
    menus::MenuItem {
        label: item.label.clone(),
        enabled: item.enabled,
        check: platform_check(item.check),
        shortcut: item.shortcut.map(platform_shortcut),
        icon: item.icon.as_ref().map(platform_image),
    }
}

fn platform_check(check: MenuItemCheck) -> menus::ItemCheck {
    match check {
        MenuItemCheck::Plain => menus::ItemCheck::Plain,
        MenuItemCheck::Checkbox(state) => menus::ItemCheck::Checkbox(match state {
            CheckState::Off => menus::CheckState::Off,
            CheckState::On => menus::CheckState::On,
            CheckState::Mixed => menus::CheckState::Mixed,
        }),
        MenuItemCheck::Radio(on) => menus::ItemCheck::Radio(on),
    }
}

fn platform_shortcut(shortcut: MenuShortcut) -> menus::Shortcut {
    let held = shortcut.modifiers;
    let mut modifiers = menus::ShortcutModifiers::empty();
    modifiers.set(menus::ShortcutModifiers::META, held.meta);
    modifiers.set(menus::ShortcutModifiers::CONTROL, held.control);
    modifiers.set(menus::ShortcutModifiers::ALT, held.alt);
    modifiers.set(menus::ShortcutModifiers::SHIFT, held.shift);
    menus::Shortcut {
        logical: shortcut.key,
        modifiers,
    }
}

/// `image`, as the WIT carries it: the same pixels.
pub(crate) fn platform_image(image: &IconImage) -> menus::Image {
    menus::Image {
        width: image.width,
        height: image.height,
        rgba: image.rgba.to_vec(),
        template: image.template,
    }
}

fn platform_provided_item(item: ProvidedItem) -> menus::ProvidedItem {
    match item {
        ProvidedItem::About => menus::ProvidedItem::About,
        ProvidedItem::Services => menus::ProvidedItem::Services,
        ProvidedItem::Hide => menus::ProvidedItem::Hide,
        ProvidedItem::HideOthers => menus::ProvidedItem::HideOthers,
        ProvidedItem::ShowAll => menus::ProvidedItem::ShowAll,
        ProvidedItem::Quit => menus::ProvidedItem::Quit,
        ProvidedItem::Minimize => menus::ProvidedItem::Minimize,
        ProvidedItem::Zoom => menus::ProvidedItem::Zoom,
        ProvidedItem::ToggleFullScreen => menus::ProvidedItem::ToggleFullScreen,
        ProvidedItem::BringAllToFront => menus::ProvidedItem::BringAllToFront,
        ProvidedItem::Undo => menus::ProvidedItem::Undo,
        ProvidedItem::Redo => menus::ProvidedItem::Redo,
        ProvidedItem::Cut => menus::ProvidedItem::Cut,
        ProvidedItem::Copy => menus::ProvidedItem::Copy,
        ProvidedItem::Paste => menus::ProvidedItem::Paste,
        ProvidedItem::SelectAll => menus::ProvidedItem::SelectAll,
        ProvidedItem::Delete => menus::ProvidedItem::Delete,
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::ShortcutModifiers;

    use super::*;

    fn ids_and_parents(flat: &[menus::MenuEntry]) -> Vec<(MenuId, Option<MenuId>)> {
        flat.iter().map(|entry| (entry.id, entry.parent)).collect()
    }

    #[test]
    fn a_tree_is_flat_each_submenu_before_what_it_holds_which_names_it() {
        let menu = [
            MenuEntry::Submenu {
                id: 1,
                label: "File".to_owned(),
                enabled: true,
                entries: vec![
                    MenuEntry::Item {
                        id: 2,
                        item: MenuItem::new("Open"),
                    },
                    MenuEntry::Submenu {
                        id: 3,
                        label: "Recent".to_owned(),
                        enabled: false,
                        entries: vec![MenuEntry::Item {
                            id: 4,
                            item: MenuItem::new("notes.txt"),
                        }],
                    },
                ],
            },
            MenuEntry::Provided {
                id: 5,
                item: ProvidedItem::Quit,
                enabled: true,
            },
        ];
        let flat = flat_menu(&menu);
        assert_eq!(
            ids_and_parents(&flat),
            [
                (1, None),
                (2, Some(1)),
                (3, Some(1)),
                (4, Some(3)),
                (5, None)
            ]
        );
        assert!(matches!(
            &flat[2].kind,
            MenuEntryKind::Submenu(menus::Submenu { label, enabled: false }) if label == "Recent"
        ));
        assert!(matches!(
            &flat[4].kind,
            MenuEntryKind::Provided(menus::Provided {
                item: menus::ProvidedItem::Quit,
                enabled: true
            })
        ));
    }

    #[test]
    fn a_separator_takes_an_id_no_other_entry_has() {
        let menu = [
            MenuEntry::Item {
                id: MenuId::MAX,
                item: MenuItem::new("Last"),
            },
            MenuEntry::Separator,
            MenuEntry::Separator,
        ];
        let flat = flat_menu(&menu);
        assert_eq!(
            ids_and_parents(&flat),
            [
                (MenuId::MAX, None),
                (MenuId::MAX - 1, None),
                (MenuId::MAX - 2, None)
            ]
        );
    }

    #[test]
    fn an_items_check_shortcut_and_icon_are_carried_over() {
        let item = MenuItem::new("Bold")
            .check(MenuItemCheck::Checkbox(CheckState::Mixed))
            .shortcut(MenuShortcut {
                key: u64::from('b'),
                modifiers: ShortcutModifiers {
                    meta: true,
                    shift: true,
                    ..ShortcutModifiers::default()
                },
            })
            .icon(IconImage {
                width: 1,
                height: 1,
                rgba: vec![1, 2, 3, 4].into(),
                template: true,
            });
        let carried = platform_item(&item);
        assert!(matches!(
            carried.check,
            menus::ItemCheck::Checkbox(menus::CheckState::Mixed)
        ));
        let shortcut = carried.shortcut.expect("a shortcut");
        assert_eq!(shortcut.logical, u64::from('b'));
        assert_eq!(
            shortcut.modifiers,
            menus::ShortcutModifiers::META | menus::ShortcutModifiers::SHIFT
        );
        let icon = carried.icon.expect("an icon");
        assert_eq!((icon.rgba, icon.template), (vec![1, 2, 3, 4], true));
    }
}
