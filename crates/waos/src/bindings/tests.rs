//! The generated conversions between the WIT's plain types and these bindings, `wit_conversions`.

use waos_capabilities::wit::wasdk::platform::menus::{
    CheckState, Image, ItemCheck, MenuEntry, MenuEntryKind, MenuItem, Shortcut, ShortcutModifiers,
};

use crate::bindings::wasdk::platform::menus as engine;

/// A menu item nested as deep as the WIT's types go: a record in a variant, an option of a record
/// holding flags, a variant holding an enum, and a list.
fn checked_item_with_shortcut_and_icon() -> MenuEntry {
    MenuEntry {
        id: 7,
        parent: Some(2),
        kind: MenuEntryKind::Item(MenuItem {
            label: "Word Wrap".to_owned(),
            enabled: true,
            check: ItemCheck::Checkbox(CheckState::Mixed),
            shortcut: Some(Shortcut {
                logical: 0x77,
                modifiers: ShortcutModifiers {
                    meta: true,
                    control: false,
                    alt: true,
                    shift: false,
                },
            }),
            icon: Some(Image {
                width: 1,
                height: 1,
                rgba: vec![1, 2, 3, 4],
                template: true,
            }),
        }),
    }
}

#[test]
fn a_nested_record_comes_back_from_the_engines_type_unchanged() {
    let entry = checked_item_with_shortcut_and_icon();
    let in_the_engine = engine::MenuEntry::from(entry.clone());
    let engine::MenuEntryKind::Item(item) = &in_the_engine.kind else {
        panic!("an item stays an item");
    };
    let modifiers = item.shortcut.as_ref().unwrap().modifiers;
    assert_eq!(
        modifiers,
        engine::ShortcutModifiers::META | engine::ShortcutModifiers::ALT
    );
    assert_eq!(MenuEntry::from(in_the_engine), entry);
}

#[test]
fn a_nested_record_comes_back_from_its_serialized_form_unchanged() {
    let entry = checked_item_with_shortcut_and_icon();
    let written = serde_json::to_string(&entry).unwrap();
    assert_eq!(serde_json::from_str::<MenuEntry>(&written).unwrap(), entry);
}
