//! Programs as Android's tasks, on a phone of the tests' own that makes a window for each request
//! (`test_phone`'s `PhoneKind::Tasks`): a program opened from the library asks for its windows,
//! each made with the program's name and icon and naming the program and whether it is its first
//! window, and none takes a tab; one opened again while it runs brings its window forward; a first
//! window the system brings back starts its program again in it, and an extra one closes; Back
//! goes to a program that handles it, and otherwise asks the program to close the window, as a
//! desktop's close button does; a window swiped away ends its view, and the program with its last; and with
//! programs shown inside WaOS, which the user's preferences keep, a program opened after takes a
//! tab, as in the deck's own tests.

use std::time::Duration;

use waft::Offset;
use waft::ui::{HostWindow, View, ViewFocusState};
use waos::Question;
use waos_program_window::{WindowRestoration, WindowRole};
use waos_services::permissions::{App, Prompts};

use super::test_phone::{BACK, PhoneKind, SURVEY, TRIANGLE, TestPhone};
use crate::logic::opened_package::tests::{ICON_PIXEL, ICON_SIDE};
use crate::logic::preferences::PreferencesFile;

/// How long a test gives a program to do what it would do were it to go wrong.
const A_MOMENT: Duration = Duration::from_millis(500);

/// An app that never asks for a view.
const FOREVER: &str = "dev.example.forever";

/// A phone of tasks with `programs` installed, each a test program's component under an id and a
/// name.
fn tasks_with(programs: &[(&str, &str, &str)]) -> TestPhone {
    let mut phone = TestPhone::of(PhoneKind::Tasks);
    for (component, id, name) in programs {
        phone.install(component, id, name);
    }
    phone
}

fn with_triangle_installed() -> TestPhone {
    tasks_with(&[(waos_test_programs::TRIANGLE, TRIANGLE, "Triangle")])
}

/// Opens `id` from the library and waits for its first window to arrive.
fn open_in_a_window(phone: &mut TestPhone, id: &str) {
    phone.start_opening(id);
    phone.settle_until("its window arrives", |phone| {
        phone.runs(id) && !phone.program_views(id).is_empty()
    });
}

fn first_window_of(id: &str) -> String {
    WindowRestoration {
        program: id.to_owned(),
        role: WindowRole::First,
    }
    .initial_data()
}

#[test]
fn a_program_opened_gets_its_window_with_its_name_its_icon_and_what_names_it() {
    let mut phone = with_triangle_installed();
    open_in_a_window(&mut phone, TRIANGLE);
    let windows = phone.windows_of(TRIANGLE);
    assert_eq!(windows.len(), 1);
    let window = &windows[0];
    assert_eq!(window.config().title, "Triangle");
    let icon = window.config().icon.clone().expect("the package's icon");
    assert_eq!((icon.width, icon.height), (ICON_SIDE, ICON_SIDE));
    assert_eq!(&icon.rgba[..4], ICON_PIXEL);
    assert_eq!(
        window.test_view().initial_data(),
        Some(first_window_of(TRIANGLE))
    );
    let views = phone.program_views(TRIANGLE);
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].request_id(), 1, "its first request took it");
    assert_eq!(phone.tab_kinds(), ["home"], "no tab of its own");
}

#[test]
fn a_question_of_an_app_before_its_first_window_is_put_over_the_window_held_for_it() {
    let mut phone = TestPhone::of(PhoneKind::Tasks);
    phone.install(waos_test_programs::FOREVER, FOREVER, "Forever");
    phone.start_opening(FOREVER);
    phone.settle_until("it runs, with its window", |phone| {
        phone.runs(FOREVER) && phone.windows_of(FOREVER).len() == 1
    });
    let app = App {
        id: FOREVER.parse().unwrap(),
        name: "Forever".to_owned(),
    };
    let _answer = phone.prompts.ask(&app, Question::Camera);
    phone.settle_until("the question is put", |phone| {
        phone.shows("Allow Forever to use the camera?")
    });
    assert_eq!(
        phone.windows.made().len(),
        1,
        "no window of the question's own: it is over the app's"
    );
    let held = phone.windows_of(FOREVER)[0].test_view().id();
    let program = phone.runs.running(&app.id).expect("it runs");
    let screen = phone
        .view_windows
        .read(&phone.shell.app())
        .screen_of(&program);
    assert_eq!(screen, Some(held), "its dialogs are shown over it too");
}

#[test]
fn an_app_opened_has_its_window_from_its_launch_though_it_never_asks_for_one() {
    let mut phone = TestPhone::of(PhoneKind::Tasks);
    phone.install(waos_test_programs::FOREVER, FOREVER, "Forever");
    phone.start_opening(FOREVER);
    phone.settle_until("it runs, with its window", |phone| {
        phone.runs(FOREVER) && phone.windows_of(FOREVER).len() == 1
    });
    phone.pump_for(A_MOMENT);
    assert!(
        phone.program_views(FOREVER).is_empty(),
        "the window waits for its first request"
    );
    assert_eq!(phone.windows_of(FOREVER)[0].config().title, "Forever");
}

#[test]
fn a_program_opened_again_brings_its_window_forward() {
    let mut phone = with_triangle_installed();
    open_in_a_window(&mut phone, TRIANGLE);
    let window = phone.windows_of(TRIANGLE).remove(0);
    let focused = || {
        let requests = window.test_view().test_host().focus_requests();
        requests
            .iter()
            .filter(|(state, _)| *state == ViewFocusState::Focused)
            .count()
    };
    let before = focused();
    phone.start_opening(TRIANGLE);
    phone.settle();
    assert_eq!(focused(), before + 1, "its window asked forward once");
    assert_eq!(phone.windows_of(TRIANGLE).len(), 1, "no second window");
}

#[test]
fn a_first_window_brought_back_starts_its_program_in_it() {
    let mut phone = with_triangle_installed();
    let restored = phone.restore_window(&first_window_of(TRIANGLE));
    phone.settle_until("triangle runs", |phone| phone.runs(TRIANGLE));
    phone.settle_until("its view is the window brought back", |phone| {
        !phone.program_views(TRIANGLE).is_empty()
    });
    let views = phone.program_views(TRIANGLE);
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0].request_id(),
        1,
        "its first request took the window brought back"
    );
    assert!(views[0].presence().on_screen);
    assert!(phone.windows_of(TRIANGLE).is_empty(), "it asked for none");
    assert_eq!(restored.title(), "Triangle", "the card names it again");
    assert!(restored.icon().is_some(), "with its icon");
    assert_eq!(restored.closes(), 0);
}

#[test]
fn a_first_window_brought_back_is_followed_as_one_asked_for() {
    let mut phone = with_triangle_installed();
    let restored = phone.restore_window(&first_window_of(TRIANGLE));
    phone.settle_until("triangle runs in it", |phone| {
        phone.runs(TRIANGLE) && !phone.program_views(TRIANGLE).is_empty()
    });
    phone.wait_until_running_still(TRIANGLE);
    restored.set_minimized(true);
    phone.report_window_changed(restored.test_view().id());
    let views = phone.program_views(TRIANGLE);
    assert!(
        !views[0].presence().on_screen,
        "its task went to the background"
    );

    assert!(
        phone.back_in(restored.test_view().id()),
        "Back is WaOS's there too"
    );
    phone.swipe_away(&restored);
    phone.settle_until("triangle ends with its window", |phone| {
        !phone.runs(TRIANGLE)
    });
}

#[test]
fn an_extra_window_brought_back_closes_at_once() {
    let mut phone = with_triangle_installed();
    let extra = WindowRestoration {
        program: TRIANGLE.to_owned(),
        role: WindowRole::Extra,
    };
    let restored = phone.restore_window(&extra.initial_data());
    phone.settle_until("the library's work", |phone| {
        phone.library.read(&phone.shell.app()).is_idle()
    });
    assert_eq!(restored.closes(), 1);
    assert!(!phone.runs(TRIANGLE), "nothing started for it");
}

#[test]
fn back_at_a_programs_first_screen_asks_it_to_close_its_window() {
    let mut phone = with_triangle_installed();
    open_in_a_window(&mut phone, TRIANGLE);
    let window = phone.windows_of(TRIANGLE).remove(0);
    assert!(phone.back_in(window.test_view().id()), "WaOS handled it");
    phone.settle_until("its program closes the window, as it decides", |_| {
        window.closes() == 1
    });
    assert!(
        !HostWindow::is_minimized(&*window),
        "closed, not sent to the background"
    );
}

#[test]
fn back_in_a_program_that_handles_it_is_the_programs() {
    let mut phone = tasks_with(&[(waos_test_programs::BACK, BACK, "Back")]);
    open_in_a_window(&mut phone, BACK);
    let window = phone.windows_of(BACK).remove(0);
    let view = window.test_view().id();
    phone.tap_in(view, Offset::new(100.0, 100.0));
    phone.settle_until("it says it handles Back", |phone| {
        phone.program_views(BACK)[0].handles_back()
    });
    assert!(phone.back_in(view));
    phone.settle_until("it went back to its first page", |phone| {
        !phone.program_views(BACK)[0].handles_back()
    });
    assert_eq!(window.closes(), 0, "the program's own Back");
}

#[test]
fn a_programs_last_window_swiped_away_ends_it() {
    let mut phone = tasks_with(&[(waos_test_programs::SURVEY, SURVEY, "Survey")]);
    open_in_a_window(&mut phone, SURVEY);
    phone.wait_until_running_still(SURVEY);
    let window = phone.windows_of(SURVEY).remove(0);
    phone.swipe_away(&window);
    phone.settle_until("survey ends", |phone| !phone.runs(SURVEY));
}

#[test]
fn with_programs_inside_waos_a_program_opened_takes_a_tab() {
    let mut phone = with_triangle_installed();
    assert_eq!(
        phone.programs_inside_waos(),
        Some(false),
        "tasks by default"
    );
    phone.library.update(&mut phone.shell.app(), |library, cx| {
        library.show_programs_inside_waos(true, cx)
    });
    assert_eq!(phone.programs_inside_waos(), Some(true));
    let kept = PreferencesFile::at(phone.folder.path().join("preferences.toml")).read();
    assert!(kept.programs_inside_waos, "kept in the preferences");
    phone.open(TRIANGLE);
    assert_eq!(phone.tab_kinds(), [TRIANGLE]);
    assert!(phone.windows_of(TRIANGLE).is_empty(), "no window asked for");
    assert_eq!(phone.program_views(TRIANGLE)[0].request_id(), 1);
}
