//! The deck's tabs with real programs in them, on a phone of the tests' own (`test_phone`).
//!
//! A program opened from the home gets the home's tab and its view as it starts, measured as the
//! tab's page, and its tab shows it, so one that asks for a view only when given none asks for
//! none; a tab keeps the last picture of a program that ended and says how it ended until it is
//! closed; only the page in sight is on screen, the home's taking a program's out of sight; a
//! program opened again brings its tab back into sight; and closing a tab stops its program, the
//! last one leaving the home. A running program's icon offers Close, which closes its tab, and a
//! package opened while a tab is in sight is asked about in an alert over the tab. A request in a
//! tab is refused (`logic::program_places`'s test). What fingers do to the deck is `ui::deck`'s
//! tests'.

use std::time::Duration;

use futures::FutureExt;
use waft::ui::View;
use waft_cupertino::CupertinoAlertDialog;
use waos::ProgramEnd;
use waos_package::ProgramId;
use waos_program_window::{ProgramCanvas, ProgramPicture};

use super::test_phone::{SURVEY, TRAP, TRIANGLE, TestPhone, with_triangle_running};
use crate::logic::opened_package::tests::write_package;
use crate::logic::tab_page::page_metrics;
use crate::ui::library::icon_actions::{IconAction, actions_offered};

/// How long a test gives an opening that found its program running to say so: it starts nothing,
/// so it answers as soon as the opening before it has.
const SECOND_OPENING: Duration = Duration::from_millis(500);

#[test]
fn a_program_opened_gets_its_tabs_view_measured_as_the_page_and_its_tab_shows_it() {
    let phone = with_triangle_running();
    assert_eq!(phone.tab_kinds(), [TRIANGLE], "the home's tab is its own");
    let view = phone.program_view(TRIANGLE);
    let page = page_metrics(&phone.screen.metrics());
    let metrics = view.metrics();
    assert_eq!(
        [metrics.width as f64, metrics.height as f64],
        page.physical_size,
        "the page above the bar"
    );
    assert_eq!(view.request_id(), None, "a view given, not asked for");
    assert!(
        view.presence().on_screen && view.in_sight(),
        "{:?}",
        view.presence()
    );
    assert!(
        phone.has::<ProgramCanvas>(),
        "the tab shows the program's canvas"
    );
    assert!(
        phone.shows("Triangle"),
        "the bar names it: {:?}",
        phone.labels()
    );
}

#[test]
fn a_program_that_asks_for_a_view_when_given_none_asks_for_none_in_a_tab() {
    let mut phone = TestPhone::with_installed(&[(waos_test_programs::SURVEY, SURVEY, "Survey")]);
    phone.open(SURVEY);
    assert_eq!(
        phone.stage(SURVEY),
        Some("running"),
        "{:?}",
        phone.end(SURVEY)
    );
    assert_eq!(phone.program_view(SURVEY).request_id(), None);
}

#[test]
fn a_tab_says_how_its_program_ended_until_it_is_closed() {
    let mut phone = TestPhone::with_installed(&[(waos_test_programs::TRAP, TRAP, "Trap")]);
    phone.start_opening(TRAP);
    phone.settle_until("trap ends", |phone| phone.end(TRAP).is_some());
    assert!(
        matches!(phone.end(TRAP), Some(ProgramEnd::Failed(_))),
        "{:?}",
        phone.end(TRAP)
    );
    assert!(
        phone
            .labels()
            .iter()
            .any(|label| label == "Trap stopped because of a problem."),
        "{:?}",
        phone.labels()
    );
    assert!(
        phone.has::<ProgramPicture>() && !phone.has::<ProgramCanvas>(),
        "its last picture, out of its reach"
    );
    phone.close(TRAP);
    assert_eq!(phone.stage(TRAP), None);
    assert_eq!(
        phone.tab_kinds(),
        ["home"],
        "the last tab closed leaves the home"
    );
}

#[test]
fn only_the_tab_in_sight_is_on_screen() {
    let mut phone = with_triangle_running();
    let view = phone.program_view(TRIANGLE);
    phone.show(None);
    assert_eq!(phone.tab_kinds(), [TRIANGLE, "home"]);
    assert!(!view.presence().on_screen, "the home is in sight");
    assert!(!phone.has::<ProgramCanvas>(), "nothing places the canvas");
    assert_eq!(phone.picture(0), None);
    phone.show(Some(TRIANGLE));
    assert!(view.presence().on_screen, "back in sight");
    assert!(phone.has::<ProgramCanvas>());
}

#[test]
fn a_program_opened_again_brings_its_tab_back_into_sight() {
    let mut phone = with_triangle_running();
    let view = phone.program_view(TRIANGLE);
    phone.show(None);
    phone.open(TRIANGLE);
    assert_eq!(phone.stage(TRIANGLE), Some("running"));
    assert_eq!(
        phone.tab_kinds(),
        [TRIANGLE, "home"],
        "no second tab for it"
    );
    assert!(view.presence().on_screen, "in sight again");
    assert_eq!(
        phone.program_view(TRIANGLE).id(),
        view.id(),
        "the same view"
    );
}

#[test]
fn closing_a_tab_stops_its_program() {
    let mut phone = with_triangle_running();
    let id: ProgramId = TRIANGLE.parse().unwrap();
    let program = futures::executor::block_on(phone.runs.running(&id)).expect("it runs");
    phone.close(TRIANGLE);
    assert_eq!(phone.stage(TRIANGLE), None, "the tab is gone");
    phone.settle_until("triangle stops", |_| {
        program.ended().now_or_never().is_some()
    });
    let end = program.ended().now_or_never();
    assert!(matches!(end, Some(ProgramEnd::Stopped)), "{end:?}");
    assert_eq!(phone.tab_kinds(), ["home"]);
}

/// A tab closed while its program was still starting, and the program opened again at once: the
/// second opening finds the first's program running and starts none, and stops none either.
#[test]
fn a_program_opened_again_while_it_starts_keeps_running_in_its_tab() {
    let mut phone =
        TestPhone::with_installed(&[(waos_test_programs::TRIANGLE, TRIANGLE, "Triangle")]);
    let id: ProgramId = TRIANGLE.parse().unwrap();
    phone
        .library
        .update(&mut phone.shell.app(), |library, cx| library.open(&id, cx));
    phone.tabs.update(&mut phone.shell.app(), |tabs, cx| {
        tabs.close_program(&id, cx)
    });
    phone
        .library
        .update(&mut phone.shell.app(), |library, cx| library.open(&id, cx));
    phone.settle_until("triangle runs in its tab", |phone| {
        phone.stage(TRIANGLE) == Some("running")
    });
    phone.pump_for(SECOND_OPENING);
    assert_eq!(phone.stage(TRIANGLE), Some("running"));
}

/// What triangle's icon offers now, as its held icon's sheet shows it.
fn triangle_offered(phone: &TestPhone) -> Vec<IconAction> {
    let id: ProgramId = TRIANGLE.parse().unwrap();
    let app = phone.shell.app();
    actions_offered(phone.library.read(&app), &id)
}

#[test]
fn a_running_programs_icon_offers_close_which_closes_its_tab() {
    let mut phone =
        TestPhone::with_installed(&[(waos_test_programs::TRIANGLE, TRIANGLE, "Triangle")]);
    assert_eq!(
        triangle_offered(&phone),
        [IconAction::Settings, IconAction::Remove],
        "nothing to close while it does not run"
    );
    phone.open(TRIANGLE);
    phone.settle_until("the library hears it runs", |phone| {
        triangle_offered(phone).contains(&IconAction::Close)
    });
    assert_eq!(
        triangle_offered(&phone),
        [IconAction::Settings, IconAction::Close, IconAction::Remove]
    );
    let id: ProgramId = TRIANGLE.parse().unwrap();
    let library = phone.library.clone();
    IconAction::Close.carry_out(&library, &id, &mut phone.shell.app());
    phone.settle();
    assert_eq!(phone.stage(TRIANGLE), None, "its tab is closed");
    phone.settle_until("Close is no longer offered", |phone| {
        !triangle_offered(phone).contains(&IconAction::Close)
    });
}

#[test]
fn a_package_opened_while_a_tab_is_in_sight_is_asked_about_over_the_tab() {
    let mut phone = with_triangle_running();
    let file = write_package(
        &phone.folder.path().join("packages"),
        SURVEY,
        "Survey",
        "1.0.0",
        b"\0asm",
        &phone.key,
    );
    phone.library.update(&mut phone.shell.app(), |library, cx| {
        library.open_files(vec![file], cx)
    });
    phone.settle();
    assert!(phone.has::<CupertinoAlertDialog>(), "{:?}", phone.labels());
    assert!(phone.shows("Install Survey?"), "{:?}", phone.labels());
    assert!(
        phone.has::<ProgramCanvas>(),
        "over the tab, which stays in sight"
    );
    assert!(phone.back(), "the system's Back goes no further");
    phone.settle();
    assert!(
        phone.shows("Install Survey?"),
        "and does not dismiss a question, which is answered"
    );
    assert!(phone.has::<ProgramCanvas>(), "nor leave the tab under it");
    phone.library.update(&mut phone.shell.app(), |library, cx| {
        library.answer(false, cx)
    });
    phone.settle();
    assert!(
        !phone.has::<CupertinoAlertDialog>(),
        "the alert goes once answered"
    );
    assert!(phone.has::<ProgramCanvas>());
}
