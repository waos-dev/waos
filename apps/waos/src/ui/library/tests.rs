//! The library in a window of a phone's size, on a desktop of the tests' own
//! (`state::library::test_desktop`): it takes Cupertino's layout, its home saying nothing is
//! installed with a button to get programs; a package opened with WaOS is installed once the user
//! says so in the alert over the home, and shows as an icon; a held icon offers the program's
//! actions, and Remove asks before it removes; a tap opens a program, whose icon then has the
//! running dot; an icon whose program installs is dimmed, with Installing… in place of its name,
//! in either layout, and a tap meanwhile opens the program once its code is ready; a source's
//! program installs from the page behind the plus, which says Installing… until its code is ready, and an icon whose program a source lists newer
//! has a badge; a package picked from a file on that page, a path or a document without one, is
//! asked about as one opened with WaOS, in either layout, and a cancelled pick shows nothing. A
//! window made as wide as a tablet's takes Fluent's.

use std::rc::Rc;
use std::sync::Arc;

use waft::ui::{EmbedderClient as _, OpenOptions, View as _, ViewId};
use waft::{Icon, IntoWidget};
use waft_cupertino::{
    CupertinoActionSheet, CupertinoAlertDialog, CupertinoIcons, CupertinoNavigationBar,
};
use waft_test::{AskedDialog, MemoryFile, TestWindow};

use super::get_programs::fluent::FluentGetPrograms;
use super::home::cupertino::CupertinoHome;
use super::home::fluent::FluentHome;
use super::home::{IconTile, RunningDot};
use super::program_icon::ProgramIcon;
use super::{LaidOutLibrary, words};
use crate::logic::opened_package::package_file_type;
use crate::state::library::LibraryPage;
use crate::state::library::test_desktop::LibraryDesktop;
use crate::state::library::test_server::Server;
use crate::state::phone_window::{PHONE_SCREEN, in_phone_window};

const TRIANGLE: &str = "dev.example.triangle";

/// The desktop with the library in a window of a phone's size, as WaOS starts it for
/// `waos phone`.
fn desktop() -> LibraryDesktop {
    let (desktop, ()) = LibraryDesktop::new(|app, library, recordings, _view_windows| {
        let page = LaidOutLibrary {
            library: library.clone(),
            recordings: recordings.clone(),
            questions_here: true,
        };
        (Some(in_phone_window(app, page.into_widget())), ())
    });
    desktop
}

fn phone_window(desktop: &LibraryDesktop) -> Rc<TestWindow> {
    let windows = desktop.windows();
    assert_eq!(windows.len(), 1, "the phone's window alone");
    Rc::clone(&windows[0])
}

fn phone_view(desktop: &LibraryDesktop) -> ViewId {
    phone_window(desktop).test_view().id()
}

/// The package of triangle at 1.0.0, made of `component`, installed in the phone's layout.
fn install_on_the_phone(desktop: &mut LibraryDesktop, component: &[u8]) {
    let package = desktop.package(TRIANGLE, "Triangle", "1.0.0", component);
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    assert!(desktop.shows("Install Triangle?"), "{:?}", desktop.labels());
    let view = phone_view(desktop);
    desktop.tap(view, words::INSTALL);
}

/// The source at `index` added, serving triangle at `version`, once it is read.
fn source_listing_triangle(desktop: &mut LibraryDesktop, server: &Server, version: &str) -> String {
    let package = desktop.package(TRIANGLE, "Triangle", version, b"\0asm");
    server.publish(TRIANGLE, &package, version);
    let index = server.url("waos/index.json");
    desktop.update(|library, cx| library.add_source(&index, cx));
    desktop.settle_until("the source's listing", |desktop| {
        desktop.read(|library| {
            library
                .sources()
                .iter()
                .all(|read| !matches!(read.listing, crate::state::library::Listing::Reading))
        })
    });
    index
}

#[test]
fn a_window_of_a_phones_size_takes_cupertinos_layout_and_says_nothing_is_installed() {
    let desktop = desktop();
    assert_eq!(phone_window(&desktop).config().size, PHONE_SCREEN);
    assert!(desktop.has::<CupertinoHome>(), "{:?}", desktop.labels());
    assert!(!desktop.has::<FluentHome>());
    assert!(desktop.shows(words::HOME_TITLE));
    assert!(desktop.shows(words::NO_PROGRAMS) && desktop.shows(words::GET_PROGRAMS));
}

#[test]
fn the_empty_homes_button_gets_programs() {
    let mut desktop = desktop();
    let view = phone_view(&desktop);
    desktop.tap(view, words::GET_PROGRAMS);
    assert!(desktop.shows(words::ADD_SOURCE), "{:?}", desktop.labels());
    assert!(desktop.shows(words::NO_SOURCES));
    desktop.tap(view, words::HOME_TITLE);
    assert!(desktop.shows(words::NO_PROGRAMS), "back on the home");
}

#[test]
fn a_window_made_as_wide_as_a_tablets_takes_fluents_layout() {
    let mut desktop = desktop();
    phone_window(&desktop).test_view().resize(768.0, 1024.0);
    let view = phone_view(&desktop);
    desktop.shell.view_metrics_changed(view);
    desktop.settle();
    assert!(desktop.has::<FluentHome>(), "{:?}", desktop.labels());
    assert!(!desktop.has::<CupertinoNavigationBar>());
}

#[test]
fn a_package_opened_with_waos_is_installed_from_the_alert_once_the_user_says_so() {
    let mut desktop = desktop();
    let package = desktop.package(TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    assert!(
        desktop.has::<CupertinoAlertDialog>(),
        "a sheet over the home"
    );
    assert_eq!(
        desktop.count::<ProgramIcon>(|icon| icon.png.is_some()),
        1,
        "the package's icon over the question"
    );
    let view = phone_view(&desktop);
    desktop.tap(view, words::INSTALL);
    assert_eq!(desktop.read(|library| library.installed().len()), 1);
    assert!(!desktop.shows("Install Triangle?"));
    assert!(!desktop.has::<CupertinoAlertDialog>(), "the alert goes");
    assert!(desktop.shows("Triangle"), "{:?}", desktop.labels());
    assert_eq!(
        desktop.count::<ProgramIcon>(|icon| icon.png.is_some()),
        1,
        "its icon, from the store"
    );
    let bar = desktop
        .bounds_of::<CupertinoNavigationBar>(&|_| true)
        .expect("the bar");
    let icon = desktop
        .bounds_of::<ProgramIcon>(&|_| true)
        .expect("the icon");
    assert!(icon.top >= bar.bottom, "below the bar: {icon:?}, {bar:?}");
}

#[test]
fn a_held_icon_offers_settings_and_remove_and_remove_asks_first() {
    let mut desktop = desktop();
    install_on_the_phone(&mut desktop, b"\0asm");
    let view = phone_view(&desktop);
    desktop.long_press(view, "Triangle");
    assert!(
        desktop.has::<CupertinoActionSheet>(),
        "{:?}",
        desktop.labels()
    );
    assert!(desktop.shows(&words::version("1.0.0")));
    assert!(desktop.shows(words::SETTINGS) && desktop.shows(words::REMOVE));
    assert!(
        !desktop.shows(words::CLOSE),
        "nothing to close where the library opens no tabs"
    );
    desktop.tap(view, words::REMOVE);
    assert!(!desktop.has::<CupertinoActionSheet>(), "the sheet goes");
    assert!(desktop.shows("Remove Triangle?"), "{:?}", desktop.labels());
    assert_eq!(
        desktop.read(|library| library.installed().len()),
        1,
        "not yet"
    );
    desktop.tap(view, words::REMOVE);
    assert!(desktop.read(|library| library.installed().is_empty()));
    assert!(desktop.shows(words::NO_PROGRAMS));
}

#[test]
fn a_tapped_icon_opens_its_program_which_is_marked_running() {
    let mut desktop = desktop();
    let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
    install_on_the_phone(&mut desktop, &forever);
    desktop.settle_until_code_is_ready();
    assert!(!desktop.has::<RunningDot>(), "nothing runs yet");
    let view = phone_view(&desktop);
    desktop.tap(view, "Triangle");
    let runs = Arc::clone(&desktop.runs);
    desktop.settle_until("the program runs", |_| {
        futures::executor::block_on(runs.names()) == [TRIANGLE]
    });
    desktop.settle_until("the running dot", |desktop| desktop.has::<RunningDot>());
    futures::executor::block_on(desktop.runs.stop_all());
    desktop.settle_until("the dot gone", |desktop| !desktop.has::<RunningDot>());
}

#[test]
fn an_installing_program_says_so_and_a_tap_meanwhile_opens_it_once_its_code_is_ready() {
    let mut desktop = desktop();
    let held = desktop.hold_compiles();
    let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
    install_on_the_phone(&mut desktop, &forever);
    assert!(desktop.shows(words::INSTALLING), "{:?}", desktop.labels());
    assert!(!desktop.shows("Triangle"), "in place of its name");
    assert_eq!(desktop.count::<IconTile>(|tile| tile.installing), 1);
    let view = phone_view(&desktop);
    desktop.tap(view, words::INSTALLING);
    assert!(
        futures::executor::block_on(desktop.runs.names()).is_empty(),
        "the open waits for its code"
    );
    assert!(desktop.shows(words::INSTALLING));
    drop(held);
    let runs = Arc::clone(&desktop.runs);
    desktop.settle_until("the program runs", |_| {
        futures::executor::block_on(runs.names()) == [TRIANGLE]
    });
    assert!(desktop.shows("Triangle") && !desktop.shows(words::INSTALLING));
    assert_eq!(desktop.count::<IconTile>(|tile| tile.installing), 0);
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn an_installing_program_says_so_in_fluents_layout_too() {
    let mut desktop = desktop();
    let _held = desktop.hold_compiles();
    let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
    install_on_the_phone(&mut desktop, &forever);
    phone_window(&desktop).test_view().resize(768.0, 1024.0);
    let view = phone_view(&desktop);
    desktop.shell.view_metrics_changed(view);
    desktop.settle();
    assert!(desktop.has::<FluentHome>());
    assert!(desktop.shows(words::INSTALLING), "{:?}", desktop.labels());
    assert_eq!(desktop.count::<IconTile>(|tile| tile.installing), 1);
}

#[test]
fn a_sources_program_installs_from_the_page_behind_the_plus() {
    let mut desktop = desktop();
    let view = phone_view(&desktop);
    let plus = CupertinoIcons::add().code_point;
    desktop.tap_widget::<Icon>(view, |icon| {
        icon.icon
            .as_ref()
            .is_some_and(|icon| icon.code_point == plus)
    });
    let server = Server::start();
    let index = source_listing_triangle(&mut desktop, &server, "1.0.0");
    assert!(desktop.shows(&index), "{:?}", desktop.labels());
    desktop.tap(view, words::GET);
    desktop.settle_until("the install", |desktop| desktop.shows(words::INSTALLED));
    assert_eq!(desktop.read(|library| library.installed().len()), 1);
    desktop.tap(view, words::REMOVE_SOURCE);
    assert!(desktop.shows(words::NO_SOURCES), "{:?}", desktop.labels());
}

#[test]
fn the_get_page_says_installing_until_the_programs_code_is_ready() {
    let mut desktop = desktop();
    let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
    let package = desktop.package(TRIANGLE, "Triangle", "1.0.0", &forever);
    let server = Server::start();
    server.publish(TRIANGLE, &package, "1.0.0");
    let index = server.url("waos/index.json");
    desktop.update(|library, cx| library.add_source(&index, cx));
    desktop.update(|library, cx| {
        library.show_page(crate::state::library::LibraryPage::GetPrograms, cx)
    });
    desktop.settle_until("the source's listing", |desktop| desktop.shows(words::GET));
    let held = desktop.hold_compiles();
    let view = phone_view(&desktop);
    desktop.tap(view, words::GET);
    desktop.settle_until("the install", |desktop| {
        desktop.read(|library| !library.installed().is_empty())
    });
    assert!(desktop.shows(words::INSTALLING), "{:?}", desktop.labels());
    assert!(
        !desktop.shows(words::INSTALLED),
        "not before its code is ready"
    );
    drop(held);
    desktop.settle_until("its code", |desktop| desktop.shows(words::INSTALLED));
    assert!(!desktop.shows(words::INSTALLING));
}

#[test]
fn an_icon_whose_program_a_source_lists_newer_is_badged() {
    let mut desktop = desktop();
    install_on_the_phone(&mut desktop, b"\0asm");
    assert_eq!(desktop.count::<ProgramIcon>(|icon| icon.badge.is_some()), 0);
    let server = Server::start();
    source_listing_triangle(&mut desktop, &server, "2.0.0");
    assert_eq!(
        desktop.count::<ProgramIcon>(|icon| icon.badge.is_some()),
        1,
        "{:?}",
        desktop.labels()
    );
    desktop.update(|library, cx| {
        library.show_page(crate::state::library::LibraryPage::GetPrograms, cx)
    });
    assert!(desktop.shows(words::UPDATE), "{:?}", desktop.labels());
    assert!(desktop.shows(&words::update_detail("2.0.0", "1.0.0")));
}

#[test]
fn a_problem_is_an_alert_until_dismissed() {
    let mut desktop = desktop();
    desktop.update(|library, cx| library.add_source("not a source", cx));
    let problem = desktop
        .read(|library| library.problem().map(str::to_owned))
        .expect("a problem");
    assert!(desktop.has::<CupertinoAlertDialog>());
    assert!(desktop.shows(&problem) && desktop.shows(words::PROBLEM));
    let view = phone_view(&desktop);
    desktop.tap(view, words::OK);
    assert!(desktop.read(|library| library.problem().is_none()));
    assert!(!desktop.has::<CupertinoAlertDialog>());
}

#[test]
fn a_long_name_is_laid_out_across_its_tile() {
    let mut desktop = desktop();
    let name = "Notes with a rather long name of many words";
    let package = desktop.package(TRIANGLE, name, "1.0.0", b"\0asm");
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    let view = phone_view(&desktop);
    desktop.tap(view, words::INSTALL);
    let label = desktop
        .bounds_of::<waft::Text>(&|text| text.data.as_deref() == Some(name))
        .expect("the name");
    let icon = desktop
        .bounds_of::<ProgramIcon>(&|_| true)
        .expect("the icon");
    assert!(
        label.width() > icon.width(),
        "the name spans its tile, cut with an ellipsis, not measured to nothing: {label:?}"
    );
}

/// The page that gets programs, shown.
fn on_the_get_page(desktop: &mut LibraryDesktop) {
    desktop.update(|library, cx| library.show_page(LibraryPage::GetPrograms, cx));
}

/// The dialog Install from file asks for, on the window of `view`: one file, of WaOS's packages.
fn package_dialog(view: ViewId) -> AskedDialog {
    AskedDialog::OpenFile(
        Some(view),
        OpenOptions {
            types: vec![package_file_type()],
            ..OpenOptions::default()
        },
    )
}

#[test]
fn a_package_picked_from_a_file_is_asked_about_and_installed_once_the_user_says_so() {
    let mut desktop = desktop();
    let package = desktop.package(TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let dialogs = desktop.file_dialogs().answering_paths(vec![package]);
    on_the_get_page(&mut desktop);
    let view = phone_view(&desktop);
    desktop.tap(view, words::INSTALL_FROM_FILE_ROW);
    assert_eq!(dialogs.asked(), [package_dialog(view)]);
    assert!(
        desktop.has::<CupertinoAlertDialog>() && desktop.shows("Install Triangle?"),
        "{:?}",
        desktop.labels()
    );
    assert!(!desktop.read(|library| library.is_picking()));
    desktop.tap(view, words::INSTALL);
    assert_eq!(desktop.read(|library| library.installed().len()), 1);
    assert!(!desktop.has::<CupertinoAlertDialog>(), "the alert goes");
}

#[test]
fn a_cancelled_pick_shows_nothing() {
    let mut desktop = desktop();
    let dialogs = desktop.file_dialogs();
    on_the_get_page(&mut desktop);
    let view = phone_view(&desktop);
    desktop.tap(view, words::INSTALL_FROM_FILE_ROW);
    assert_eq!(dialogs.asked().len(), 1, "the picker was shown");
    assert!(
        !desktop.has::<CupertinoAlertDialog>(),
        "{:?}",
        desktop.labels()
    );
    assert!(desktop.read(|library| library.question().is_none() && library.problem().is_none()));
    assert!(desktop.shows(words::ADD_SOURCE), "still on the page");
}

#[test]
fn the_library_is_picking_until_the_picker_answers_a_cancelled_one_too() {
    let mut desktop = desktop();
    desktop
        .library
        .update(&mut desktop.shell.app(), |library, cx| {
            library.pick_package(None, cx)
        });
    assert!(desktop.read(|library| library.is_picking()));
    desktop.settle();
    assert!(!desktop.read(|library| library.is_picking()));
}

#[test]
fn a_package_picked_without_a_path_is_copied_and_asked_about() {
    let mut desktop = desktop();
    let package = desktop.package(TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let document = MemoryFile::new("triangle.wapk", std::fs::read(&package).unwrap());
    desktop
        .file_dialogs()
        .answering(move || vec![document.picked()]);
    on_the_get_page(&mut desktop);
    let view = phone_view(&desktop);
    desktop.tap(view, words::INSTALL_FROM_FILE_ROW);
    desktop.settle_until("the question", |desktop| desktop.shows("Install Triangle?"));
    desktop.tap(view, words::INSTALL);
    assert_eq!(desktop.read(|library| library.installed().len()), 1);
}

#[test]
fn a_package_picked_from_a_file_in_fluents_layout_is_asked_about() {
    let mut desktop = desktop();
    phone_window(&desktop).test_view().resize(768.0, 1024.0);
    let view = phone_view(&desktop);
    desktop.shell.view_metrics_changed(view);
    let package = desktop.package(TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let dialogs = desktop.file_dialogs().answering_paths(vec![package]);
    on_the_get_page(&mut desktop);
    assert!(desktop.has::<FluentGetPrograms>(), "{:?}", desktop.labels());
    desktop.tap(view, words::INSTALL_FROM_FILE);
    assert_eq!(dialogs.asked(), [package_dialog(view)]);
    assert!(desktop.shows("Install Triangle?"), "{:?}", desktop.labels());
    assert!(!desktop.has::<CupertinoAlertDialog>(), "Fluent's dialog");
}
