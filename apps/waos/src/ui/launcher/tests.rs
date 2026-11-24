//! The launcher on a desktop of the tests' own (`state::library::test_desktop`), whose one screen
//! leaves a menu bar and a Dock out of its visible frame: summoned at launch, it is a borderless,
//! see-through card with a shadow in the middle of the screen, showing the grid under its search
//! row, and started closed it has no window until it is summoned; typing filters the grid and
//! Enter opens the first program shown, which dismisses it; Escape, the system's close command
//! and a click elsewhere dismiss it; a running program has its dot and one a source lists newer
//! its badge; "…" reaches getting programs and the settings, each with Back to the grid; a package
//! opened with WaOS summons it with the question; on Windows and Linux a dismissed launcher is
//! minimized, its button kept, restored as the user brings it back, its search field given the
//! keyboard again; and with WaOS no longer kept
//! in the Dock or the taskbar, which the settings page offers and WaOS's preferences keep, WaOS
//! has an icon in the menu bar or the tray, whose click summons the launcher, and a dismissed
//! launcher is hidden on every desktop.

use std::rc::Rc;
use std::sync::Arc;

use waft::ui::{
    HostWindow as _, Offset, TargetPlatform, View as _, ViewId, WindowBackground, WindowLevel,
};
use waft::{EditableText, Entity};
use waft_test::TestWindow;
use waft_winui::{FluentSymbol, FontIcon};
use waos_package::ProgramId;

use super::home::LauncherHome;
use super::launcher_window;
use crate::state::launcher::Launcher;
use crate::state::library::test_desktop::{DesktopKind, LibraryDesktop};
use crate::state::library::test_server::Server;
use crate::state::waos_tray::WaosTray;
use crate::ui::library::home::RunningDot;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

const TRIANGLE: &str = "dev.example.triangle";
const GALLERY: &str = "dev.example.gallery";

/// How WaOS starts the launcher.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Start {
    Shown,
    Closed,
}

/// A desktop of `kind`, the library in the launcher, started as `start` says.
pub(crate) fn launcher_desktop(
    kind: DesktopKind,
    start: Start,
) -> (LibraryDesktop, Entity<Launcher>) {
    LibraryDesktop::of(
        kind,
        Box::new(move |app, library, recordings, view_windows| {
            let launcher = Launcher::new(app, library, view_windows.clone());
            WaosTray::follow(app, library, &launcher);
            launcher.update(app, |launcher, cx| match start {
                Start::Shown => launcher.summoned("WaOS started", cx),
                Start::Closed => launcher.started_closed(cx),
            });
            let window = launcher_window(&launcher, library, recordings);
            (Some(window), launcher)
        }),
    )
}

/// macOS's desktop, the launcher summoned at launch.
fn desktop() -> (LibraryDesktop, Entity<Launcher>) {
    launcher_desktop(DesktopKind::default(), Start::Shown)
}

/// The desktop of `system`, the launcher summoned at launch.
fn desktop_of(system: TargetPlatform) -> (LibraryDesktop, Entity<Launcher>) {
    let kind = DesktopKind {
        system,
        ..DesktopKind::default()
    };
    launcher_desktop(kind, Start::Shown)
}

/// The launcher's window: the first window made, before any program opened from it made its
/// own.
pub(crate) fn launcher_window_of(desktop: &LibraryDesktop) -> Rc<TestWindow> {
    let windows = desktop.windows();
    Rc::clone(windows.first().expect("the launcher's window"))
}

pub(crate) fn launcher_view(desktop: &LibraryDesktop) -> ViewId {
    launcher_window_of(desktop).test_view().id()
}

/// Whether the launcher is on screen: shown, and not minimized.
fn is_on_screen(desktop: &LibraryDesktop) -> bool {
    let window = launcher_window_of(desktop);
    window.is_shown() && !window.is_minimized()
}

/// Something summons the launcher, as a click on WaOS's Dock icon does.
fn summon(desktop: &mut LibraryDesktop, launcher: &Entity<Launcher>) {
    launcher.update(&mut desktop.shell.app(), |launcher, cx| {
        launcher.summoned("WaOS's Dock icon was clicked", cx)
    });
    desktop.shell.cell().checkpoint();
    desktop.settle();
}

/// A package of `id`, named `name`, at `version`, made of `component`, installed through the
/// launcher's question.
fn install(desktop: &mut LibraryDesktop, id: &str, name: &str, version: &str, component: &[u8]) {
    let package = desktop.package(id, name, version, component);
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    let view = launcher_view(desktop);
    desktop.tap(view, words::INSTALL);
    desktop.settle_until_code_is_ready();
}

/// Taps the button whose icon is `symbol`.
fn tap_symbol(desktop: &mut LibraryDesktop, symbol: FluentSymbol) {
    let view = launcher_view(desktop);
    let glyph = symbol.glyph().to_string();
    desktop.tap_widget::<FontIcon>(view, |icon| icon.glyph == glyph);
}

#[test]
fn summoned_at_launch_the_launcher_is_a_floating_card_in_the_middle_of_the_screen() {
    let (desktop, _launcher) = desktop();
    let window = launcher_window_of(&desktop);
    let config = window.config();
    assert!(!config.decorations && !config.resizable && config.shadow);
    assert_eq!(config.background, WindowBackground::Transparent);
    assert_eq!(config.level, WindowLevel::AlwaysOnTop);
    assert_eq!(
        window.test_view().initial_data().as_deref(),
        Some(crate::state::launcher::LAUNCHER_VIEW)
    );
    assert!(window.is_shown());
    assert_eq!(window.frame().center(), Offset::new(864.0, 540.0));
    assert!(
        desktop.has::<LauncherHome>(),
        "its search row over the grid"
    );
    assert!(
        desktop.shows(words::SEARCH_PROGRAMS),
        "{:?}",
        desktop.labels()
    );
    assert!(desktop.shows(words::NO_PROGRAMS), "{:?}", desktop.labels());
}

#[test]
fn started_closed_on_macos_the_launcher_has_no_window_until_it_is_summoned() {
    let (mut desktop, launcher) = launcher_desktop(DesktopKind::default(), Start::Closed);
    assert!(
        desktop.windows().is_empty(),
        "nothing on screen but programs"
    );
    summon(&mut desktop, &launcher);
    assert!(is_on_screen(&desktop));
    assert!(desktop.has::<LauncherHome>());
}

#[test]
fn typing_filters_the_grid() {
    let (mut desktop, _launcher) = desktop();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    install(&mut desktop, GALLERY, "Gallery", "1.0.0", b"\0asm");
    assert!(desktop.shows("Triangle") && desktop.shows("Gallery"));
    let view = launcher_view(&desktop);
    desktop.type_text(view, "tri");
    assert_eq!(desktop.read(|library| library.search().to_owned()), "tri");
    assert!(desktop.shows("Triangle"), "{:?}", desktop.labels());
    assert!(!desktop.shows("Gallery"), "{:?}", desktop.labels());
    desktop.type_text(view, "nothing like it");
    assert!(
        desktop.shows(words::NO_PROGRAMS_FOUND),
        "{:?}",
        desktop.labels()
    );
}

#[test]
fn enter_opens_the_first_program_shown_and_dismisses_the_launcher() {
    let (mut desktop, _launcher) = desktop();
    let forever = std::fs::read(waos_test_programs::FOREVER_SHOWN).unwrap();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", &forever);
    install(&mut desktop, GALLERY, "Gallery", "1.0.0", b"\0asm");
    let view = launcher_view(&desktop);
    desktop.type_text(view, "tri");
    desktop.press_enter();
    assert!(!is_on_screen(&desktop));
    let runs = Arc::clone(&desktop.runs);
    desktop.settle_until("the program runs", |_| {
        futures::executor::block_on(runs.names()) == [TRIANGLE]
    });
    assert_eq!(
        desktop.read(|library| library.search().to_owned()),
        "",
        "the launcher comes back with nothing searched"
    );
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_click_on_a_program_opens_it_and_dismisses_the_launcher() {
    let (mut desktop, _launcher) = desktop();
    let forever = std::fs::read(waos_test_programs::FOREVER_SHOWN).unwrap();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", &forever);
    let view = launcher_view(&desktop);
    desktop.tap(view, "Triangle");
    assert!(!is_on_screen(&desktop));
    let runs = Arc::clone(&desktop.runs);
    desktop.settle_until("the program runs", |_| {
        futures::executor::block_on(runs.names()) == [TRIANGLE]
    });
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn escape_dismisses_the_launcher() {
    let (mut desktop, launcher) = desktop();
    desktop.press_escape();
    assert!(!is_on_screen(&desktop));
    summon(&mut desktop, &launcher);
    assert!(is_on_screen(&desktop));
}

#[test]
fn the_systems_close_command_hides_the_launcher_and_keeps_its_window() {
    let (mut desktop, _launcher) = desktop();
    let view = launcher_view(&desktop);
    desktop.request_close(view);
    assert!(!is_on_screen(&desktop));
    assert_eq!(launcher_window_of(&desktop).closes(), 0);
}

#[test]
fn a_click_elsewhere_dismisses_the_launcher_once_it_has_had_the_focus() {
    let (mut desktop, _launcher) = desktop();
    let view = launcher_view(&desktop);
    desktop.focus(view, false);
    assert!(
        is_on_screen(&desktop),
        "not before the launcher had the focus"
    );
    desktop.focus(view, true);
    desktop.focus(view, false);
    assert!(!is_on_screen(&desktop));
}

#[test]
fn a_running_program_has_its_dot_and_one_a_source_lists_newer_its_badge() {
    let (mut desktop, _launcher) = desktop();
    let forever = std::fs::read(waos_test_programs::FOREVER_SHOWN).unwrap();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", &forever);
    assert_eq!(desktop.count::<RunningDot>(|_| true), 0);
    let server = Server::start();
    let newer = desktop.package(TRIANGLE, "Triangle", "1.1.0", b"\0asm");
    server.publish(TRIANGLE, &newer, "1.1.0");
    let index = server.url("waos/index.json");
    desktop.update(|library, cx| library.add_source(&index, cx));
    desktop.settle_until("the update badged", |desktop| {
        desktop.count::<ProgramIcon>(|icon| icon.badge.is_some()) == 1
    });
    let id: ProgramId = TRIANGLE.parse().unwrap();
    desktop.update(|library, cx| library.open(&id, cx));
    desktop.settle_until("the running dot", |desktop| {
        desktop.count::<RunningDot>(|_| true) == 1
    });
    futures::executor::block_on(desktop.runs.stop_all());
}

/// Whether the launcher's search field draws its caret now.
fn search_draws_its_caret(desktop: &LibraryDesktop) -> bool {
    desktop.count::<EditableText>(EditableText::effective_show_cursor) == 1
}

/// No caret blinks in the launcher's empty search field, which has the keyboard all the same:
/// what is typed goes into it, and its caret shows while it holds something.
#[test]
fn the_empty_search_field_shows_no_caret_and_takes_what_is_typed() {
    let (mut desktop, _launcher) = desktop();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let view = launcher_view(&desktop);
    assert!(!search_draws_its_caret(&desktop), "empty");
    desktop.type_text(view, "tri");
    assert_eq!(desktop.read(|library| library.search().to_owned()), "tri");
    assert!(search_draws_its_caret(&desktop), "holding what was typed");
    desktop.type_text(view, "");
    assert!(!search_draws_its_caret(&desktop), "empty again");
}

/// The grid lists the apps from A to Z by the names their user reads, whatever their case, not
/// by their ids.
#[test]
fn the_grid_lists_the_apps_from_a_to_z_by_name() {
    let (mut desktop, _launcher) = desktop();
    install(&mut desktop, "dev.example.a", "Zebra", "1.0.0", b"\0asm");
    install(&mut desktop, "dev.example.b", "apple", "1.0.0", b"\0asm");
    install(&mut desktop, "dev.example.c", "Mango", "1.0.0", b"\0asm");
    let across = |name: &str| desktop.find(name).expect("in the grid").dx();
    assert!(
        across("apple") < across("Mango") && across("Mango") < across("Zebra"),
        "{:?}",
        desktop.labels()
    );
}

#[test]
fn more_reaches_getting_programs_and_the_settings_each_with_back_to_the_grid() {
    let (mut desktop, _launcher) = desktop();
    // So the menu's Get programs is the one label saying so: the home has no button for it.
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let view = launcher_view(&desktop);
    tap_symbol(&mut desktop, FluentSymbol::More);
    desktop.tap(view, words::GET_PROGRAMS);
    assert!(desktop.shows(words::NO_SOURCES), "{:?}", desktop.labels());
    assert!(!desktop.has::<LauncherHome>());
    tap_symbol(&mut desktop, FluentSymbol::Back);
    assert!(desktop.has::<LauncherHome>());
    tap_symbol(&mut desktop, FluentSymbol::More);
    desktop.tap(view, words::SETTINGS);
    assert!(
        desktop.shows(words::GLOBAL_SETTINGS),
        "{:?}",
        desktop.labels()
    );
    tap_symbol(&mut desktop, FluentSymbol::Back);
    assert!(desktop.has::<LauncherHome>());
}

/// A page opened from the launcher enters as WinUI's `Frame` navigates: it rises into place from
/// below.
#[test]
fn a_page_rises_into_place_over_the_grid_as_winuis_pages_enter() {
    let (mut desktop, _launcher) = desktop();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let view = launcher_view(&desktop);
    tap_symbol(&mut desktop, FluentSymbol::More);
    desktop.tap_in_motion(view, words::GET_PROGRAMS);
    desktop.frames(5);
    let rising = desktop
        .find(words::NO_SOURCES)
        .expect("the page, on its way in");
    desktop.settle();
    let settled = desktop.find(words::NO_SOURCES).expect("the page, in place");
    assert!(
        rising.dy() > settled.dy() + 10.0,
        "from below: at {rising:?}, then {settled:?}"
    );
    assert!(!desktop.has::<LauncherHome>(), "the grid, covered");
}

#[test]
fn a_dismissed_launcher_comes_back_on_its_grid() {
    let (mut desktop, launcher) = desktop();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let view = launcher_view(&desktop);
    tap_symbol(&mut desktop, FluentSymbol::More);
    desktop.tap(view, words::GET_PROGRAMS);
    assert!(!desktop.has::<LauncherHome>());
    desktop.press_escape();
    summon(&mut desktop, &launcher);
    assert!(desktop.has::<LauncherHome>());
}

#[test]
fn a_package_opened_with_waos_summons_the_launcher_with_the_question() {
    let (mut desktop, _launcher) = desktop();
    desktop.press_escape();
    let package = desktop.package(TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    assert!(is_on_screen(&desktop));
    assert!(desktop.shows("Install Triangle?"), "{:?}", desktop.labels());
    let view = launcher_view(&desktop);
    desktop.tap(view, words::INSTALL);
    desktop.settle_until_code_is_ready();
    assert!(desktop.shows("Triangle"), "{:?}", desktop.labels());
}

#[test]
fn a_program_is_removed_from_its_icons_menu_once_the_user_says_so() {
    let (mut desktop, _launcher) = desktop();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let view = launcher_view(&desktop);
    desktop.right_click(view, "Triangle");
    desktop.tap(view, words::ASK_TO_REMOVE);
    assert!(desktop.shows("Remove Triangle?"), "{:?}", desktop.labels());
    desktop.tap(view, words::REMOVE);
    assert!(desktop.read(|library| library.installed().is_empty()));
}

#[test]
fn a_program_that_cannot_be_opened_summons_the_launcher_again_with_why() {
    let (mut desktop, _launcher) = desktop();
    install(&mut desktop, TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    let view = launcher_view(&desktop);
    desktop.tap(view, "Triangle");
    desktop.settle_until("the launcher shows again", is_on_screen);
    let problem = desktop
        .read(|library| library.problem().map(str::to_owned))
        .unwrap();
    assert!(problem.contains("could not be opened"), "{problem}");
    assert!(desktop.shows(&problem), "{:?}", desktop.labels());
}

#[test]
fn on_windows_and_linux_a_dismissed_launcher_is_minimized_and_comes_back_restored() {
    for system in [TargetPlatform::Windows, TargetPlatform::Linux] {
        let (mut desktop, launcher) = desktop_of(system);
        desktop.press_escape();
        let window = launcher_window_of(&desktop);
        assert!(
            window.is_shown() && window.is_minimized(),
            "{system:?}: its button kept"
        );
        summon(&mut desktop, &launcher);
        assert!(is_on_screen(&desktop), "{system:?}");
    }
}

#[test]
fn a_minimized_launcher_the_user_restores_is_summoned() {
    let (mut desktop, _launcher) = desktop_of(TargetPlatform::Windows);
    let view = launcher_view(&desktop);
    desktop.focus(view, true);
    desktop.focus(view, false);
    assert!(
        launcher_window_of(&desktop).is_minimized(),
        "a click elsewhere"
    );
    launcher_window_of(&desktop).set_minimized(false);
    desktop.focus(view, true);
    desktop.press_escape();
    assert!(
        launcher_window_of(&desktop).is_minimized(),
        "restored, it is the launcher shown, which Escape dismisses"
    );
}

/// The field has the keyboard again because the view's focus scope gives it back to what had it,
/// as the view regains the focus: nothing of the launcher's is needed for it.
#[test]
fn a_minimized_launcher_the_user_restores_gives_its_search_field_the_keyboard() {
    let (mut desktop, _launcher) = desktop_of(TargetPlatform::Windows);
    let view = launcher_view(&desktop);
    desktop.focus(view, true);
    desktop.focus(view, false);
    launcher_window_of(&desktop).set_minimized(false);
    desktop.focus(view, true);
    desktop.type_text(view, "tri");
    assert_eq!(desktop.read(|library| library.search().to_owned()), "tri");
}

#[test]
fn started_closed_on_linux_the_launcher_is_minimized_from_the_start() {
    let kind = DesktopKind {
        system: TargetPlatform::Linux,
        ..DesktopKind::default()
    };
    let (desktop, _launcher) = launcher_desktop(kind, Start::Closed);
    let window = launcher_window_of(&desktop);
    assert!(window.is_shown() && window.is_minimized());
}

#[test]
fn the_launcher_shows_in_the_middle_of_the_screen_wherever_the_pointer_is() {
    let (mut desktop, launcher) = desktop();
    desktop.press_escape();
    desktop.move_pointer(Some(Offset::new(300.0, 1080.0)));
    summon(&mut desktop, &launcher);
    assert_eq!(
        launcher_window_of(&desktop).frame().center(),
        Offset::new(864.0, 540.0),
        "not above the Dock icon clicked"
    );
}

/// The desktop of `system`, keeping WaOS's preferences, the launcher summoned at launch.
fn keeping_preferences_on(system: TargetPlatform) -> (LibraryDesktop, Entity<Launcher>) {
    let kind = DesktopKind {
        system,
        own_preferences: true,
        ..DesktopKind::default()
    };
    launcher_desktop(kind, Start::Shown)
}

/// Turns "Keep WaOS in the Dock" or "in the taskbar" off or on in the launcher's settings, as the
/// user does, and goes back to the grid.
fn keep_in_dock(desktop: &mut LibraryDesktop, kept: bool, system: TargetPlatform) {
    let view = launcher_view(desktop);
    tap_symbol(desktop, FluentSymbol::More);
    desktop.tap(view, words::SETTINGS);
    assert!(
        desktop.shows(words::keep_in_dock(system)),
        "{:?}",
        desktop.labels()
    );
    assert_eq!(desktop.read(|library| library.kept_in_dock()), Some(!kept));
    desktop.update(|library, cx| library.keep_in_dock(kept, cx));
    tap_symbol(desktop, FluentSymbol::Back);
}

#[test]
fn waos_is_kept_in_the_dock_with_no_tray_icon_by_default() {
    let (desktop, _launcher) = keeping_preferences_on(TargetPlatform::MacOS);
    assert_eq!(desktop.read(|library| library.kept_in_dock()), Some(true));
    assert!(desktop.tray_icons().is_empty());
}

#[test]
fn not_kept_in_the_dock_waos_has_a_tray_icon_whose_click_summons_the_launcher() {
    let (mut desktop, _launcher) = keeping_preferences_on(TargetPlatform::MacOS);
    keep_in_dock(&mut desktop, false, TargetPlatform::MacOS);
    let icons = desktop.tray_icons();
    assert_eq!(icons.len(), 1, "WaOS's icon in the menu bar");
    assert_eq!(icons[0].tooltip.as_deref(), Some("WaOS"));
    desktop.press_escape();
    assert!(!is_on_screen(&desktop));
    desktop.click_tray_icon(icons[0].id);
    assert!(is_on_screen(&desktop));
    keep_in_dock(&mut desktop, true, TargetPlatform::MacOS);
    assert!(desktop.tray_icons().is_empty(), "gone with the choice");
}

#[test]
fn the_choice_is_kept_in_waos_preferences() {
    let (mut desktop, _launcher) = keeping_preferences_on(TargetPlatform::MacOS);
    keep_in_dock(&mut desktop, false, TargetPlatform::MacOS);
    let file = crate::logic::preferences::PreferencesFile::at(
        desktop.folder.path().join("preferences.toml"),
    );
    assert!(!file.read().kept_in_dock);
}

#[test]
fn the_choice_switches_windows_and_linux_between_minimizing_the_launcher_and_hiding_it() {
    for system in [TargetPlatform::Windows, TargetPlatform::Linux] {
        let (mut desktop, _launcher) = keeping_preferences_on(system);
        keep_in_dock(&mut desktop, false, system);
        desktop.press_escape();
        let window = launcher_window_of(&desktop);
        assert!(
            !window.is_shown() && !window.is_minimized(),
            "{system:?}: hidden, its tray icon left to summon it"
        );
        let icon = desktop.tray_icons()[0].id;
        desktop.click_tray_icon(icon);
        assert!(is_on_screen(&desktop), "{system:?}");
        keep_in_dock(&mut desktop, true, system);
        desktop.press_escape();
        let window = launcher_window_of(&desktop);
        assert!(
            window.is_shown() && window.is_minimized(),
            "{system:?}: minimized again, its taskbar button kept"
        );
    }
}
