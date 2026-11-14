use std::time::Duration;

use futures::channel::mpsc;
use waft::Shell;
use waft::ui::EmbedderClient;
use waft_test::{MockMenuBar, MockPopupMenus, TestPlatform};
use waos::bindings::wasdk::platform::menus::{ItemCheck, MenuEntryKind, MenuItem, Submenu};

use super::*;
use crate::connection::tests::{connection, sent};

/// File, with Greet, the program's own item 11.
fn file_menu() -> Vec<MenuEntry> {
    let file = Submenu {
        label: "File".to_owned(),
        enabled: true,
    };
    let greet = MenuItem {
        label: "Greet".to_owned(),
        enabled: true,
        check: ItemCheck::Plain,
        shortcut: None,
        icon: None,
    };
    vec![
        MenuEntry {
            id: 10,
            parent: None,
            kind: MenuEntryKind::Submenu(file),
        },
        MenuEntry {
            id: 11,
            parent: Some(10),
            kind: MenuEntryKind::Item(greet),
        },
    ]
}

#[test]
fn the_programs_bar_is_shown_and_a_choice_in_it_goes_to_waos_by_its_id() {
    let menu_bar = MockMenuBar::new();
    let platform = TestPlatform::new().with_menu_bar(Rc::new(menu_bar.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        send_menu_choices(app, connection);
        show_menu_bar(app, &file_menu());
    });
    assert_eq!(
        menu_bar.id_of(&["File", "Greet"]),
        Some(11),
        "under the id the program gave it"
    );
    shell.menu_selected(11);
    let chosen = sent(&waos);
    assert!(
        matches!(chosen, FromWindow::MenuBarChosen(11)),
        "{chosen:?}"
    );
}

#[test]
fn a_context_menu_opens_where_it_is_asked_and_its_choice_goes_to_waos_once_it_closes() {
    let menus = MockPopupMenus::new().answering(Some(11));
    let platform = TestPlatform::new().with_popup_menus(Rc::new(menus.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let view = ViewId(3);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        let opened = show_at(app, view, Offset::new(5.0, 10.0), &file_menu());
        answer_when_closed(app, connection, 7, opened);
    });
    shell.wake(Duration::ZERO);
    let shown = menus.shown();
    assert_eq!((shown[0].view, shown[0].at), (view, Offset::new(5.0, 10.0)));
    assert_eq!(shown[0].menu, waft_menu(&file_menu()));
    let closed = sent(&waos);
    assert!(
        matches!(
            closed,
            FromWindow::ContextMenuClosed {
                menu: 7,
                chosen: Some(11)
            }
        ),
        "{closed:?}"
    );
}

#[test]
fn a_context_menu_at_a_view_no_window_shows_is_answered_none_at_once() {
    let menus = MockPopupMenus::new().answering(Some(11));
    let platform = TestPlatform::new().with_popup_menus(Rc::new(menus.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let _shell = Shell::new(Rc::new(platform), |app| {
        let (_requests, requested) = mpsc::unbounded();
        let view_windows = ViewWindows::<WaosProgram>::listen(app, requested);
        let asked = AskedContextMenu {
            menu: 8,
            window: 2,
            at: (10.0, 20.0),
            entries: file_menu(),
        };
        open_context_menu(app, &view_windows, connection, asked);
    });
    assert!(menus.shown().is_empty(), "no menu opens");
    let closed = sent(&waos);
    assert!(
        matches!(
            closed,
            FromWindow::ContextMenuClosed {
                menu: 8,
                chosen: None
            }
        ),
        "{closed:?}"
    );
}
