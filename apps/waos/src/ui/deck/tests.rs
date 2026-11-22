//! What fingers do to the deck, on a phone of the tests' own (`state::deck::test_phone`): each
//! gesture end to end, each competition between two, and each interruption, played as the
//! pointer's reports a phone would send and read as what a user would see, where each program's
//! picture is on screen and what the labels say.
//!
//! The tabs in most tests are two programs', triangle then survey, the deck resting on survey's
//! page: so triangle's picture is the first the compositor made and survey's the second. Where a
//! test needs more tabs, each is a triangle of its own.

use waft::{Offset, Rect, Size};
use waft_cupertino::CupertinoIcons;

use crate::logic::deck_geometry::{DeckLayout, PAGE_GAP};
use crate::logic::prompts::SettingsOpened;
use crate::logic::tab_page::BAR_HEIGHT;
use crate::state::deck::test_phone::{
    BACK, HOME_INDICATOR, INCOMPATIBLE, Picture, SCREEN, STATUS_BAR, SURVEY, TRAP, TRIANGLE,
    TestPhone,
};
use crate::state::library::{LibraryPage, open_settings_when_asked};

/// Which picture is which program's, in the order the tests open them.
const TRIANGLES: usize = 0;
const SURVEYS: usize = 1;

/// A point on the bar's pill.
fn on_the_bar() -> Offset {
    Offset::new(150.0, SCREEN[1] - HOME_INDICATOR - BAR_HEIGHT / 2.0)
}

/// The box a page fills at rest.
fn page_box() -> Rect {
    Rect::from_ltwh(0.0, 0.0, SCREEN[0], SCREEN[1] - HOME_INDICATOR - BAR_HEIGHT)
}

/// How far apart two pages are on the strip.
fn page_spacing() -> f64 {
    SCREEN[0] + PAGE_GAP
}

/// The phone with triangle and survey each running in a tab, in that order, resting on survey's.
fn two_programs_running() -> TestPhone {
    let mut phone = TestPhone::with_installed(&[
        (waos_test_programs::TRIANGLE, TRIANGLE, "Triangle"),
        (waos_test_programs::SURVEY, SURVEY, "Survey"),
    ]);
    phone.open(TRIANGLE);
    phone.show(None);
    phone.open(SURVEY);
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY]);
    phone
}

/// Where the last frame drew the left edge of the page of the `nth` program opened.
fn page_left(phone: &TestPhone, nth: usize) -> f64 {
    phone
        .picture(nth)
        .unwrap_or_else(|| panic!("program {nth}'s page is not drawn"))
        .bounds
        .left
}

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.75,
        "{what}: {actual}, not {expected}"
    );
}

fn assert_rect_near(actual: Rect, expected: Rect, what: &str) {
    let near = |a: f64, b: f64| (a - b).abs() < 0.75;
    assert!(
        near(actual.left, expected.left)
            && near(actual.top, expected.top)
            && near(actual.right, expected.right)
            && near(actual.bottom, expected.bottom),
        "{what}: {actual:?}, not {expected:?}"
    );
}

#[test]
fn at_rest_the_current_page_fills_the_view_above_the_bar_which_names_it() {
    let phone = two_programs_running();
    let survey = phone.picture(SURVEYS).expect("survey's page is drawn");
    assert_rect_near(survey.bounds, page_box(), "survey's page");
    assert_rect_near(survey.visible, page_box(), "all of it shows");
    assert_near(survey.scale, 1.0, "at its own size");
    assert_eq!(phone.picture(TRIANGLES), None, "one page in sight");
    let pill = phone
        .label_bounds("Survey")
        .expect("the pill names the page");
    assert!(pill.top > page_box().bottom, "in the bar: {pill:?}");
    assert!(!phone.shows("Triangle"), "{:?}", phone.labels());
    assert!(
        STATUS_BAR > 0.0 && survey.bounds.top == 0.0,
        "under the status bar"
    );
}

#[test]
fn a_sideways_drag_on_the_bar_moves_the_pages_and_their_pills_with_the_finger() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(60.0, 0.0);
    phone.slide(1, start, dragging, 6);
    let (page_before, pill_before) = (
        page_left(&phone, SURVEYS),
        phone.label_bounds("Survey").unwrap().left,
    );
    assert!(page_before > 0.0, "the drag has started: {page_before}");
    let further = dragging + Offset::new(100.0, 0.0);
    phone.slide(1, dragging, further, 10);
    assert_near(
        page_left(&phone, SURVEYS) - page_before,
        100.0,
        "the page moved as far as the finger",
    );
    assert_near(
        phone.label_bounds("Survey").unwrap().left - pill_before,
        100.0,
        "and so did its pill",
    );
    let triangle = phone.picture(TRIANGLES).expect("the page before comes in");
    assert_near(
        page_left(&phone, SURVEYS) - triangle.bounds.right,
        PAGE_GAP,
        "a gap between the pages",
    );
    assert!(phone.shows("Triangle"), "its pill comes with it");
    assert_near(triangle.scale, 1.0, "never resized");
    phone.touch_up(1, further);
}

#[test]
fn a_slow_drag_let_go_settles_on_the_nearest_page() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let short_of_half = start + Offset::new(0.4 * page_spacing(), 0.0);
    phone.drag_and_let_go(start, short_of_half, 30);
    phone.settle();
    assert_eq!(phone.current_tab(), SURVEY, "back where it was");
    assert_rect_near(phone.picture(SURVEYS).unwrap().bounds, page_box(), "survey");

    let past_half = start + Offset::new(0.7 * page_spacing(), 0.0);
    phone.drag_and_let_go(start, past_half, 30);
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert_rect_near(
        phone.picture(TRIANGLES).unwrap().bounds,
        page_box(),
        "triangle",
    );
    assert_eq!(phone.picture(SURVEYS), None);
}

#[test]
fn a_flick_moves_on_to_the_next_page_however_little_the_strip_moved() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.swipe(start, start + Offset::new(70.0, 0.0), 4);
    assert!(
        page_left(&phone, SURVEYS) < 0.3 * page_spacing(),
        "the strip had hardly moved"
    );
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert_rect_near(
        phone.picture(TRIANGLES).unwrap().bounds,
        page_box(),
        "triangle",
    );
}

#[test]
fn a_swipe_that_turns_back_and_lifts_moving_back_returns_to_the_page_it_left() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let out = start + Offset::new(0.75 * page_spacing(), 0.0);
    phone.touch_down(1, start);
    phone.slide(1, start, out, 20);
    let back = out + Offset::new(-0.1 * page_spacing(), 0.0);
    phone.slide(1, out, back, 9);
    phone.touch_up(1, back);
    let released_at = page_left(&phone, SURVEYS);
    for frame in 0..600 {
        let asked = phone.pump();
        let left = page_left(&phone, SURVEYS);
        assert!(
            left <= released_at + 0.75,
            "frame {frame} after the release took survey's page right to {left}, from \
             {released_at}"
        );
        if !asked {
            break;
        }
    }
    phone.settle();
    assert_eq!(phone.current_tab(), SURVEY, "the finger went back last");
}

#[test]
fn the_release_settles_at_the_fingers_speed_without_a_jump() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let end = start + Offset::new(120.0, 0.0);
    phone.touch_down(1, start);
    phone.slide(1, start, end, 6);
    let last_step = 120.0 / 6.0;
    let before = page_left(&phone, SURVEYS);
    phone.touch_up(1, end);
    let mut previous_step = last_step;
    let mut previous = before;
    for frame in 0..12 {
        phone.pump();
        let left = page_left(&phone, SURVEYS);
        let step = left - previous;
        assert!(
            step > 0.0 && (step - previous_step).abs() < 0.6 * last_step,
            "frame {frame} after the release moved {step}, after {previous_step}"
        );
        (previous, previous_step) = (left, step);
    }
}

#[test]
fn the_strip_resists_before_the_first_page_and_springs_back() {
    let mut phone = two_programs_running();
    phone.show(Some(TRIANGLE));
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(40.0, 0.0);
    phone.slide(1, start, dragging, 4);
    let before = page_left(&phone, TRIANGLES);
    let further = dragging + Offset::new(100.0, 0.0);
    phone.slide(1, dragging, further, 10);
    let moved = page_left(&phone, TRIANGLES) - before;
    assert!(
        moved > 10.0 && moved < 70.0,
        "less than the finger's 100: {moved}"
    );
    phone.touch_up(1, further);
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert_rect_near(
        phone.picture(TRIANGLES).unwrap().bounds,
        page_box(),
        "triangle",
    );
}

#[test]
fn a_finger_down_on_the_bar_catches_the_strip_as_it_settles_and_lifting_lets_it_go_on() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.swipe(start, start + Offset::new(70.0, 0.0), 4);
    phone.pump_frames(3);
    let moving = page_left(&phone, SURVEYS);
    phone.touch_down(1, start);
    let caught = page_left(&phone, SURVEYS);
    phone.pump_frames(5);
    assert_eq!(page_left(&phone, SURVEYS), caught, "held where it was");
    assert!(
        (caught - moving).abs() < 40.0,
        "no jump as it was caught: {moving} then {caught}"
    );
    assert!(caught < page_spacing() - 1.0, "caught on the way");
    phone.touch_up(1, start);
    phone.settle();
    assert_eq!(
        phone.current_tab(),
        TRIANGLE,
        "it goes on to where it was heading"
    );
    assert_rect_near(
        phone.picture(TRIANGLES).unwrap().bounds,
        page_box(),
        "triangle",
    );
}

#[test]
fn a_drag_that_catches_the_strip_takes_over_from_where_it_is() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.swipe(start, start + Offset::new(70.0, 0.0), 4);
    phone.pump_frames(3);
    phone.touch_down(1, start);
    let caught = page_left(&phone, SURVEYS);
    let dragging = start + Offset::new(-40.0, 0.0);
    phone.slide(1, start, dragging, 5);
    let started = page_left(&phone, SURVEYS);
    assert!(
        started <= caught + 0.75 && started >= caught - 40.75,
        "the drag starts where the strip was held: {caught} then {started}"
    );
    let back = dragging + Offset::new(-80.0, 0.0);
    phone.slide(1, dragging, back, 8);
    assert_near(
        page_left(&phone, SURVEYS) - started,
        -80.0,
        "with the finger",
    );
    phone.touch_up(1, back);
    phone.settle();
}

#[test]
fn a_second_finger_takes_the_drag_over_and_the_first_lifting_ends_nothing() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(80.0, 0.0);
    phone.slide(1, start, dragging, 8);
    let second = Offset::new(260.0, start.dy());
    phone.touch_down(2, second);
    let held = page_left(&phone, SURVEYS);
    phone.touch_up(1, dragging);
    phone.pump_frames(4);
    assert_eq!(
        page_left(&phone, SURVEYS),
        held,
        "the second finger still holds it"
    );
    let further = second + Offset::new(50.0, 0.0);
    phone.slide(2, second, further, 5);
    assert_near(page_left(&phone, SURVEYS) - held, 50.0, "and drags it");
    phone.touch_up(2, further);
    phone.settle();
    assert!(phone.picture(SURVEYS).is_some() ^ phone.picture(TRIANGLES).is_some());
}

#[test]
fn past_the_last_tab_is_the_home_which_opens_where_the_strip_settles_on_it() {
    let mut phone = two_programs_running();
    let start = Offset::new(300.0, on_the_bar().dy());
    phone.touch_down(1, start);
    let dragging = start + Offset::new(-100.0, 0.0);
    phone.slide(1, start, dragging, 8);
    let home = phone
        .label_bounds("WaOS")
        .expect("the home's page comes in");
    let survey = phone.picture(SURVEYS).unwrap();
    assert!(
        home.left > survey.bounds.right,
        "after the last page: {home:?}"
    );
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY], "not a tab yet");
    let further = dragging + Offset::new(-180.0, 0.0);
    phone.slide(1, dragging, further, 10);
    phone.touch_up(1, further);
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY, "home"]);
    phone.settle();
    assert_eq!(phone.current_tab(), "home");
    assert_eq!(phone.picture(SURVEYS), None);
    let title = phone.label_bounds("WaOS").unwrap();
    assert!(title.left > 0.0 && title.right < SCREEN[0], "{title:?}");
}

#[test]
fn past_the_home_the_strip_resists_since_there_is_one_home_at_most() {
    let mut phone = two_programs_running();
    phone.show(None);
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY, "home"]);
    let start = Offset::new(300.0, on_the_bar().dy());
    phone.swipe(start, start + Offset::new(-250.0, 0.0), 6);
    phone.settle();
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY, "home"]);
    assert_eq!(phone.current_tab(), "home");
}

#[test]
fn a_swipe_between_two_programs_changes_who_is_on_screen_at_the_release_and_at_rest() {
    let mut phone = two_programs_running();
    let (triangle, survey) = (phone.program_view(TRIANGLE), phone.program_view(SURVEY));
    assert!(survey.presence().on_screen && survey.presence().focused);
    assert!(!triangle.presence().on_screen);
    let start = on_the_bar();
    let end = start + Offset::new(0.7 * page_spacing(), 0.0);
    phone.touch_down(1, start);
    phone.slide(1, start, end, 20);
    assert!(
        survey.presence().focused && !triangle.presence().on_screen,
        "nothing changes while the finger drags"
    );
    phone.touch_up(1, end);
    assert!(triangle.presence().on_screen, "the page headed to, at once");
    assert!(
        !triangle.presence().focused,
        "not until the deck rests on it"
    );
    assert!(
        survey.presence().on_screen,
        "the page left, until the deck arrives"
    );
    assert!(!survey.presence().focused);
    phone.settle();
    assert!(triangle.presence().on_screen && triangle.presence().focused);
    assert!(!survey.presence().on_screen && !survey.presence().focused);
}

#[test]
fn a_drag_let_go_on_the_page_it_started_from_changes_nobodys_presence() {
    let mut phone = two_programs_running();
    let survey = phone.program_view(SURVEY);
    let start = on_the_bar();
    phone.drag_and_let_go(start, start + Offset::new(0.3 * page_spacing(), 0.0), 20);
    assert!(survey.presence().on_screen && survey.presence().focused);
    phone.settle();
    assert!(survey.presence().on_screen && survey.presence().focused);
    assert!(!phone.program_view(TRIANGLE).presence().on_screen);
}

#[test]
fn a_program_opened_from_outside_mid_drag_takes_the_strip_from_the_finger() {
    let mut phone = two_programs_running();
    phone.show(None);
    phone.show(Some(SURVEY));
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(-90.0, 0.0);
    phone.slide(1, start, dragging, 8);
    phone.start_opening(TRIANGLE);
    assert_eq!(phone.current_tab(), TRIANGLE);
    let further = dragging + Offset::new(-90.0, 0.0);
    phone.slide(1, dragging, further, 8);
    phone.touch_up(1, further);
    phone.settle();
    assert_eq!(
        phone.current_tab(),
        TRIANGLE,
        "the finger still down did nothing more"
    );
    assert_rect_near(
        phone.picture(TRIANGLES).unwrap().bounds,
        page_box(),
        "triangle",
    );
}

#[test]
fn the_tab_under_the_finger_closing_leaves_the_strip_on_the_tab_that_takes_its_place() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(80.0, 0.0);
    phone.slide(1, start, dragging, 8);
    phone.close(SURVEY);
    assert_eq!(phone.tab_kinds(), [TRIANGLE]);
    phone.slide(1, dragging, dragging + Offset::new(60.0, 0.0), 6);
    phone.touch_up(1, dragging + Offset::new(60.0, 0.0));
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert_rect_near(
        phone.picture(TRIANGLES).unwrap().bounds,
        page_box(),
        "triangle",
    );
}

#[test]
fn a_program_ending_under_the_finger_leaves_the_drag_going() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(60.0, 0.0);
    phone.slide(1, start, dragging, 6);
    let before = page_left(&phone, SURVEYS);
    phone.program_view(SURVEY);
    let survey: waos_package::ProgramId = SURVEY.parse().unwrap();
    futures::executor::block_on(phone.runs.running(&survey))
        .expect("it runs")
        .stop();
    phone.settle_until("survey ends in its tab", |phone| {
        phone.stage(SURVEY) == Some("ended")
    });
    let further = dragging + Offset::new(50.0, 0.0);
    phone.slide(1, dragging, further, 5);
    assert_near(
        page_left(&phone, SURVEYS) - before,
        50.0,
        "its last picture follows",
    );
    phone.touch_up(1, further);
    phone.settle();
}

#[test]
fn the_view_turning_mid_drag_ends_the_drag_and_the_deck_settles_in_the_new_size() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(100.0, 0.0);
    phone.slide(1, start, dragging, 8);
    phone.resize(SCREEN[1], SCREEN[0]);
    phone.pump_frames(2);
    phone.slide(1, dragging, dragging + Offset::new(200.0, 0.0), 8);
    phone.touch_up(1, dragging + Offset::new(200.0, 0.0));
    phone.settle();
    assert_eq!(
        phone.current_tab(),
        SURVEY,
        "the finger still down did nothing more"
    );
    let turned = Rect::from_ltwh(0.0, 0.0, SCREEN[1], SCREEN[0] - HOME_INDICATOR - BAR_HEIGHT);
    assert_rect_near(
        phone.picture(SURVEYS).unwrap().bounds,
        turned,
        "survey, turned",
    );
    let view = phone.program_view(SURVEY).metrics();
    assert_eq!(
        [f64::from(view.width), f64::from(view.height)],
        [turned.width() * 3.0, turned.height() * 3.0],
        "its program is told the new size, and no other"
    );
}

#[test]
fn a_drag_on_a_programs_page_is_the_programs_and_moves_nothing_of_the_deck() {
    let mut phone = two_programs_running();
    let on_the_page = Offset::new(200.0, 400.0);
    phone.swipe(on_the_page, on_the_page + Offset::new(150.0, 0.0), 6);
    assert_rect_near(phone.picture(SURVEYS).unwrap().bounds, page_box(), "survey");
    phone.swipe(on_the_page, on_the_page + Offset::new(0.0, -300.0), 6);
    phone.settle();
    assert_rect_near(phone.picture(SURVEYS).unwrap().bounds, page_box(), "survey");
    assert_eq!(phone.current_tab(), SURVEY);
    assert!(!phone.overview_open());
}

/// A point on the tabs button, at the bar's trailing end.
fn on_the_tabs_button() -> Offset {
    Offset::new(SCREEN[0] - 34.0, on_the_bar().dy())
}

#[test]
fn the_tabs_buttons_tap_loses_to_the_drag_once_the_finger_moves() {
    let mut phone = two_programs_running();
    let start = on_the_tabs_button();
    let end = start + Offset::new(-0.7 * page_spacing(), 0.0);
    phone.drag_and_let_go(start, end, 20);
    phone.settle();
    assert!(!phone.overview_open(), "no tap");
    assert_eq!(phone.current_tab(), "home", "the drag moved the strip on");

    phone.tap(start);
    phone.settle();
    assert!(phone.overview_open(), "a tap is the button's");
}

#[test]
fn a_page_lies_under_the_systems_top_bar_and_the_bar_sits_clear_of_its_bottom_one() {
    let mut phone = two_programs_running();
    phone.cover(60.0, 20.0, 0.0);
    phone.settle();
    let page = Rect::from_ltwh(0.0, 0.0, SCREEN[0], SCREEN[1] - 20.0 - BAR_HEIGHT);
    assert_rect_near(
        phone.picture(SURVEYS).unwrap().bounds,
        page,
        "survey's page",
    );
    let pill = phone.label_bounds("Survey").unwrap();
    assert!(
        pill.top > page.bottom && pill.bottom < SCREEN[1] - 20.0,
        "the pill, between the page and the system's bar: {pill:?}"
    );
    let told = phone.program_view(SURVEY).metrics();
    assert_eq!(f64::from(told.height), page.height() * 3.0);
    assert_eq!(told.padding.top, 180.0, "told what covers its top");
    assert_eq!(told.padding.bottom, 0.0, "the bottom is the bar's");
}

/// On a phone like a Pixel 2 XL, whose gesture zone along the bottom, 112 pixels, reaches above
/// its navigation bar, 84: the bar sits clear of the zone, so a drag on it is never the system's.
#[test]
fn the_bar_sits_clear_of_the_systems_gesture_zone_at_the_bottom() {
    let (navigation_bar, gesture_zone) = (84.0 / 3.0, 112.0 / 3.0);
    let mut phone = two_programs_running();
    phone.cover(STATUS_BAR, navigation_bar, 0.0);
    phone.take_swipes_along_the_bottom(gesture_zone);
    phone.settle();
    let page = Rect::from_ltwh(0.0, 0.0, SCREEN[0], SCREEN[1] - gesture_zone - BAR_HEIGHT);
    assert_rect_near(
        phone.picture(SURVEYS).unwrap().bounds,
        page,
        "survey's page",
    );
    let pill = phone.label_bounds("Survey").unwrap();
    assert!(
        pill.top > page.bottom && pill.bottom < SCREEN[1] - gesture_zone,
        "the pill, between the page and the gesture zone: {pill:?}"
    );
    let told = phone.program_view(SURVEY).metrics();
    assert_eq!(f64::from(told.height), (page.height() * 3.0).round());
    assert_eq!(told.padding.bottom, 0.0, "the bottom is the bar's");
}

/// The deck's layout on the tests' phone, for where the overview's cards are.
fn layout() -> DeckLayout {
    DeckLayout::new(Size::new(SCREEN[0], SCREEN[1]), STATUS_BAR, HOME_INDICATOR)
}

/// Where the card in `slot` is in the open overview, its grid not scrolled.
fn card_slot(slot: usize) -> Rect {
    layout().card_rect(slot as f64, 0.0)
}

/// What shows of the page of the `nth` program opened, as the last frame drew it.
fn shown_part(phone: &TestPhone, nth: usize) -> Rect {
    phone
        .picture(nth)
        .unwrap_or_else(|| panic!("program {nth}'s page is not drawn"))
        .visible
}

/// The id and the name of the `nth` of the triangles a phone of many tabs runs.
fn nth_triangle(nth: usize) -> (String, String) {
    (
        format!("dev.example.triangle{nth}"),
        format!("Triangle {nth}"),
    )
}

/// The phone with `count` programs each running in a tab, resting on the last one's page.
fn phone_with_tabs(count: usize) -> TestPhone {
    let mut phone = TestPhone::new();
    for nth in 0..count {
        let (id, name) = nth_triangle(nth);
        phone.install(waos_test_programs::TRIANGLE, &id, &name);
        if nth > 0 {
            phone.show(None);
        }
        phone.open(&id);
    }
    phone
}

/// The phone with triangle and survey running and the overview open, at rest.
fn overview_of_two() -> TestPhone {
    let mut phone = two_programs_running();
    phone.tap(on_the_tabs_button());
    phone.settle();
    assert!(phone.overview_open());
    phone
}

/// Frames until the deck settles, none of which moves the page of the `nth` program by more than
/// `most` points from the frame before.
fn settle_without_a_jump(phone: &mut TestPhone, nth: usize, most: f64) {
    let mut previous = phone.picture(nth);
    for frame in 0..600 {
        let asked = phone.pump();
        let now = phone.picture(nth);
        if let (Some(before), Some(after)) = (previous, now) {
            let moved = [
                after.visible.left - before.visible.left,
                after.visible.top - before.visible.top,
                after.visible.right - before.visible.right,
                after.visible.bottom - before.visible.bottom,
            ]
            .into_iter()
            .fold(0.0, |furthest: f64, edge| furthest.max(edge.abs()));
            assert!(
                moved <= most,
                "frame {frame} moved program {nth}'s page by {moved}: {before:?} to {after:?}"
            );
        }
        previous = now;
        if !asked {
            break;
        }
    }
    phone.settle();
}

#[test]
fn an_upward_drag_on_the_bar_shrinks_the_page_with_its_edge_under_the_finger() {
    let mut phone = two_programs_running();
    let told = phone.program_view(SURVEY).metrics();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(0.0, -40.0);
    phone.slide(1, start, dragging, 5);
    let edge_before = shown_part(&phone, SURVEYS).bottom;
    assert!(
        edge_before < page_box().bottom,
        "the page has started to shrink"
    );
    let further = dragging + Offset::new(0.0, -150.0);
    phone.slide(1, dragging, further, 15);
    let survey = phone.picture(SURVEYS).unwrap();
    assert_near(
        survey.visible.bottom - edge_before,
        -150.0,
        "the page's edge rose as far as the finger",
    );
    assert!(survey.scale < 0.95, "scaled, not resized: {}", survey.scale);
    assert_near(
        survey.bounds.width() / survey.bounds.height(),
        page_box().width() / page_box().height(),
        "in its own shape",
    );
    let now = phone.program_view(SURVEY).metrics();
    assert_eq!(
        (now.width, now.height),
        (told.width, told.height),
        "its program is told of no other size"
    );
    assert_near(
        share_across(&phone, SURVEYS, further.dx()),
        further.dx() / SCREEN[0],
        "it shrinks towards the finger, which went straight up",
    );
    phone.touch_up(1, further);
    phone.settle();
}

/// The bar hands its place to the toolbar as the page flies: the bar's pill is gone before the
/// toolbar's controls show, so the two never show through each other.
#[test]
fn the_bars_pill_is_gone_before_the_toolbars_controls_show() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let mut finger = start;
    loop {
        finger = finger + Offset::new(0.0, -6.0);
        phone.touch_move(1, finger);
        let pill = phone
            .label_bounds("Survey")
            .is_some_and(|pill| pill.top > page_box().bottom);
        let controls = phone.shows("Done");
        assert!(!(pill && controls), "both at once: {:?}", phone.labels());
        if controls {
            break;
        }
        assert!(finger.dy() > 300.0, "the toolbar's controls never showed");
    }
    assert!(phone.shows("2 tabs"), "{:?}", phone.labels());
    phone.slide(1, finger, start, 12);
    assert!(!phone.shows("Done"), "{:?}", phone.labels());
    assert!(
        phone.shows("Survey"),
        "the bar is back: {:?}",
        phone.labels()
    );
    phone.touch_up(1, start);
    phone.settle();
    assert!(phone.program_view(SURVEY).presence().focused);
}

#[test]
fn a_drag_let_go_far_enough_opens_the_overview_on_a_grid_of_every_tabs_card() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.drag_and_let_go(start, start + Offset::new(0.0, -330.0), 30);
    assert!(phone.overview_open());
    phone.settle();
    assert_rect_near(
        shown_part(&phone, TRIANGLES),
        card_slot(0),
        "triangle's card",
    );
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
    assert!(card_slot(1).left > card_slot(0).right, "two columns");
    for said in ["Triangle", "Survey", "2 tabs", "Done"] {
        assert!(phone.shows(said), "{said}: {:?}", phone.labels());
    }
    let title = phone.label_bounds("Survey").unwrap();
    assert!(
        title.top > card_slot(1).bottom,
        "a title under each card: {title:?}"
    );
}

#[test]
fn a_drag_let_go_short_of_it_closes_the_overview_again() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.drag_and_let_go(start, start + Offset::new(0.0, -90.0), 20);
    assert!(!phone.overview_open());
    phone.settle();
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
    assert_eq!(phone.picture(TRIANGLES), None);
    assert!(!phone.shows("Done"), "{:?}", phone.labels());
}

#[test]
fn a_flick_up_opens_the_overview_however_little_the_page_shrank_and_lands_without_a_jump() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.swipe(start, start + Offset::new(0.0, -70.0), 4);
    assert!(phone.overview_open());
    assert!(
        shown_part(&phone, SURVEYS).bottom > page_box().bottom - 80.0,
        "the page had hardly shrunk"
    );
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
}

#[test]
fn a_flick_down_closes_the_overview_it_was_dragged_into() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let up = start + Offset::new(0.0, -350.0);
    phone.touch_down(1, start);
    phone.slide(1, start, up, 20);
    let down = up + Offset::new(0.0, 80.0);
    phone.slide(1, up, down, 4);
    phone.touch_up(1, down);
    assert!(!phone.overview_open());
    phone.settle();
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
}

/// Frames until the deck settles, none of which takes the bottom of what shows of the `nth`
/// program's page above where it was at the release.
fn assert_never_rises_after_the_release(phone: &mut TestPhone, nth: usize) {
    let released_at = shown_part(phone, nth).bottom;
    for frame in 0..600 {
        let asked = phone.pump();
        let bottom = shown_part(phone, nth).bottom;
        assert!(
            bottom >= released_at - 0.75,
            "frame {frame} after the release took the page's edge up to {bottom}, from \
             {released_at}"
        );
        if !asked {
            break;
        }
    }
}

#[test]
fn a_drag_up_that_turns_back_down_and_lifts_moving_down_closes_the_overview_the_fingers_last_way() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let up = start + Offset::new(0.0, -300.0);
    phone.touch_down(1, start);
    phone.slide(1, start, up, 20);
    let down = up + Offset::new(0.0, 90.0);
    phone.slide(1, up, down, 20);
    phone.touch_up(1, down);
    assert_never_rises_after_the_release(&mut phone, SURVEYS);
    phone.settle();
    assert!(!phone.overview_open(), "the finger went down last");
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
}

#[test]
fn the_axis_the_finger_takes_first_on_the_bar_wins() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let up = start + Offset::new(0.0, -60.0);
    phone.slide(1, start, up, 6);
    let shrunk = phone.picture(SURVEYS).unwrap();
    assert!(shrunk.scale < 0.99, "upward: the page shrinks");
    let sideways = up + Offset::new(200.0, 0.0);
    phone.slide(1, up, sideways, 10);
    let moved = phone.picture(SURVEYS).unwrap();
    assert_near(
        moved.scale,
        shrunk.scale,
        "sideways after: no more of the overview",
    );
    assert_near(
        moved.bounds.left - shrunk.bounds.left,
        200.0,
        "the page goes with the finger, and the strip stays",
    );
    assert_eq!(phone.current_tab(), SURVEY);
    phone.touch_up(1, sideways);
    phone.settle();

    let mut phone = two_programs_running();
    phone.touch_down(1, start);
    let sideways = start + Offset::new(60.0, 0.0);
    phone.slide(1, start, sideways, 6);
    let up = sideways + Offset::new(0.0, -200.0);
    phone.slide(1, sideways, up, 10);
    assert_near(
        phone.picture(SURVEYS).unwrap().scale,
        1.0,
        "sideways: no overview",
    );
    assert!(page_left(&phone, SURVEYS) > 20.0);
    phone.touch_up(1, up);
    phone.settle();
    assert!(!phone.overview_open());
}

#[test]
fn the_tabs_button_opens_the_overview_and_done_closes_it() {
    let mut phone = two_programs_running();
    let survey = phone.program_view(SURVEY);
    phone.tap(on_the_tabs_button());
    assert!(phone.overview_open());
    assert!(
        survey.presence().on_screen && !survey.presence().focused,
        "on screen until the deck rests in the overview, the focus gone at once"
    );
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert!(
        !survey.presence().on_screen,
        "a card shows its last picture"
    );
    assert!(!phone.program_view(TRIANGLE).presence().on_screen);
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");

    let done = phone
        .label_bounds("Done")
        .expect("Done, on the toolbar")
        .center();
    phone.tap(done);
    assert!(!phone.overview_open());
    assert!(survey.presence().on_screen && !survey.presence().focused);
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert!(survey.presence().on_screen && survey.presence().focused);
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
}

#[test]
fn a_tap_on_a_card_grows_it_into_its_page() {
    let mut phone = overview_of_two();
    let triangle = phone.program_view(TRIANGLE);
    phone.tap(card_slot(0).center());
    assert!(!phone.overview_open());
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert!(triangle.presence().on_screen && !triangle.presence().focused);
    settle_without_a_jump(&mut phone, TRIANGLES, 60.0);
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
    assert_eq!(phone.picture(SURVEYS), None);
    assert!(triangle.presence().focused);
    assert!(!phone.program_view(SURVEY).presence().on_screen);
}

/// Two pages in flight at once, each on its own: a card tapped while the page before it is still
/// landing in its slot grows into its page, and the other goes on landing where it was going.
#[test]
fn a_card_tapped_while_another_page_still_lands_grows_as_the_other_goes_on_landing() {
    let mut phone = two_programs_running();
    phone.tap(on_the_tabs_button());
    let nearly_landed =
        |phone: &TestPhone| shown_part(phone, SURVEYS).width() < card_slot(1).width() * 1.08;
    let mut frames = 0;
    while !nearly_landed(&phone) {
        phone.pump();
        frames += 1;
        assert!(frames < 200, "the overview never opened");
    }
    let landing = shown_part(&phone, SURVEYS);
    assert!(
        (landing.width() - card_slot(1).width()).abs() > 1.0,
        "survey's page is still on its way: {landing:?}"
    );
    assert_rect_near(
        shown_part(&phone, TRIANGLES),
        card_slot(0),
        "triangle's card stands",
    );
    phone.tap(card_slot(0).center());
    assert_eq!(phone.current_tab(), TRIANGLE, "the tap took");
    let goes_on = shown_part(&phone, SURVEYS);
    assert!(
        (goes_on.left - landing.left).abs() < 12.0 && (goes_on.top - landing.top).abs() < 12.0,
        "the page still landing goes on from where it was: {landing:?} then {goes_on:?}"
    );
    let growing = shown_part(&phone, TRIANGLES);
    assert!(
        (growing.left - card_slot(0).left).abs() < 12.0
            && (growing.top - card_slot(0).top).abs() < 12.0,
        "and the card tapped grows from its slot: {growing:?}"
    );
    settle_without_a_jump(&mut phone, TRIANGLES, 60.0);
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
    assert_eq!(phone.picture(SURVEYS), None, "behind it, no card is drawn");
}

/// How far the page of the `nth` program has flown from its page towards its card in `slot`, as
/// the last frame drew it: 0 on its page, 1 its card.
fn flown(phone: &TestPhone, nth: usize, slot: usize) -> f64 {
    let width = shown_part(phone, nth).width();
    (page_box().width() - width) / (page_box().width() - card_slot(slot).width())
}

/// Triangle's card tapped in the overview of two, its page growing back halfway to its box.
fn triangle_flying_to_its_page() -> TestPhone {
    let mut phone = overview_of_two();
    phone.tap(card_slot(0).center());
    assert!(!phone.overview_open());
    let mut frames = 0;
    while flown(&phone, TRIANGLES, 0) > 0.5 {
        phone.pump();
        frames += 1;
        assert!(frames < 60, "the page never grew");
    }
    phone
}

/// A page flying to its page with no finger on it takes the touches on it as it will at rest:
/// they reach its program where the page will be once it lands, and the deck takes none, so the
/// flight goes on.
#[test]
fn a_touch_on_a_page_flying_to_its_page_reaches_its_program_where_it_lands() {
    let mut phone = triangle_flying_to_its_page();
    let flying = phone.picture(TRIANGLES).unwrap();
    assert!(flying.scale < 0.9, "still small: {flying:?}");
    let on_the_page = flying.visible.center();
    let touched = phone
        .canvas_touched_at(on_the_page)
        .expect("the touch reaches a program");
    assert_rect_near(touched.drawn, flying.bounds, "triangle's canvas");
    assert_near(touched.at.dx(), on_the_page.dx(), "across, where it lands");
    assert_near(touched.at.dy(), on_the_page.dy(), "down, where it lands");
    let not_drawn_there = Offset::new(SCREEN[0] - 20.0, page_box().center().dy());
    assert!(!flying.visible.contains(not_drawn_there));
    assert_eq!(
        phone
            .canvas_touched_at(not_drawn_there)
            .map(|touched| touched.at),
        Some(not_drawn_there),
        "a touch where the page will be, though it is not drawn there yet"
    );
    phone.touch_down(1, on_the_page);
    phone.pump_frames(3);
    assert!(
        phone.picture(TRIANGLES).unwrap().scale > flying.scale,
        "the flight goes on under the finger"
    );
    phone.touch_up(1, on_the_page);
    settle_without_a_jump(&mut phone, TRIANGLES, 60.0);
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
    assert!(phone.program_view(TRIANGLE).presence().focused);
}

/// What the clip still cuts away of a page flying to its page, along its top and bottom, takes no
/// touch until it shows: neither its program nor the deck has it.
#[test]
fn what_the_clip_still_cuts_away_of_a_page_flying_to_its_page_takes_no_touch() {
    let mut phone = triangle_flying_to_its_page();
    let flying = phone.picture(TRIANGLES).unwrap();
    let cut_at_the_top = (flying.visible.top - flying.bounds.top) / flying.scale;
    assert!(cut_at_the_top > 4.0, "the top is cut away: {flying:?}");
    let in_the_cut = Offset::new(page_box().center().dx(), cut_at_the_top / 2.0);
    assert_eq!(phone.canvas_touched_at(in_the_cut), None);
    let below_it = Offset::new(page_box().center().dx(), cut_at_the_top + 2.0);
    assert_eq!(
        phone.canvas_touched_at(below_it).map(|touched| touched.at),
        Some(below_it),
        "what shows of the page is its program's"
    );
    phone.touch_down(1, in_the_cut);
    phone.pump_frames(3);
    assert!(phone.picture(TRIANGLES).unwrap().scale > flying.scale);
    phone.touch_up(1, in_the_cut);
    phone.settle();
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
}

/// The bar keeps its gestures while a page flies to its page: an upward drag on it takes the page
/// over from where it is drawn, and sends it to its card.
#[test]
fn an_upward_drag_on_the_bar_takes_over_a_page_flying_to_its_page() {
    let mut phone = triangle_flying_to_its_page();
    let mut frames = 0;
    while flown(&phone, TRIANGLES, 0) > 0.15 {
        phone.pump();
        frames += 1;
        assert!(frames < 60, "the page never neared its box");
    }
    let start = on_the_bar();
    phone.touch_down(1, start);
    let caught = shown_part(&phone, TRIANGLES);
    phone.pump_frames(3);
    assert_eq!(shown_part(&phone, TRIANGLES), caught, "held where it was");
    let up = start + Offset::new(0.0, -200.0);
    phone.slide(1, start, up, 10);
    let taken = shown_part(&phone, TRIANGLES);
    assert!(
        taken.width() < caught.width() - 20.0,
        "the page shrinks with the finger: {caught:?} then {taken:?}"
    );
    phone.touch_up(1, up);
    assert!(phone.overview_open());
    phone.settle();
    assert_rect_near(
        shown_part(&phone, TRIANGLES),
        card_slot(0),
        "triangle's card",
    );
}

#[test]
fn a_finger_down_on_the_page_in_flight_catches_it_and_lifting_lets_it_go_on() {
    let mut phone = two_programs_running();
    phone.tap(on_the_tabs_button());
    phone.pump_frames(6);
    let moving = shown_part(&phone, SURVEYS);
    assert!(moving.bottom < page_box().bottom && moving.width() > card_slot(1).width() + 20.0);
    let on_the_page = moving.center();
    assert_eq!(
        phone.canvas_touched_at(on_the_page),
        None,
        "a page flying to its card is the deck's to catch, not its program's"
    );
    phone.touch_down(1, on_the_page);
    let caught = shown_part(&phone, SURVEYS);
    phone.pump_frames(6);
    assert_eq!(shown_part(&phone, SURVEYS), caught, "held where it was");
    phone.touch_up(1, on_the_page);
    phone.settle();
    assert!(phone.overview_open(), "it goes on to where it was heading");
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
}

#[test]
fn a_vertical_drag_that_catches_the_page_in_flight_takes_its_edge_from_where_it_is() {
    let mut phone = two_programs_running();
    phone.tap(on_the_tabs_button());
    phone.pump_frames(6);
    let anywhere = shown_part(&phone, SURVEYS).center();
    phone.touch_down(1, anywhere);
    let caught = shown_part(&phone, SURVEYS).bottom;
    let dragging = anywhere + Offset::new(0.0, 40.0);
    phone.slide(1, anywhere, dragging, 5);
    let started = shown_part(&phone, SURVEYS).bottom;
    assert!(
        started >= caught - 0.75 && started <= caught + 40.75,
        "the drag starts where the transition was held: {caught} then {started}"
    );
    let further = dragging + Offset::new(0.0, 60.0);
    phone.slide(1, dragging, further, 6);
    assert_near(
        shown_part(&phone, SURVEYS).bottom - started,
        60.0,
        "the edge follows the finger down",
    );
    phone.slide(1, further, further + Offset::new(0.0, 400.0), 10);
    phone.touch_up(1, further + Offset::new(0.0, 400.0));
    assert!(!phone.overview_open(), "dragged back down to its page");
    phone.settle();
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
    assert!(phone.program_view(SURVEY).presence().focused);
}

#[test]
fn a_card_thrown_sideways_closes_its_tab_and_ends_its_program() {
    let mut phone = overview_of_two();
    let on_survey = card_slot(1).center();
    phone.swipe(on_survey, on_survey + Offset::new(-140.0, 0.0), 5);
    assert_eq!(
        phone.tab_kinds(),
        [TRIANGLE, SURVEY],
        "until its card is gone"
    );
    phone.settle();
    assert_eq!(phone.tab_kinds(), [TRIANGLE]);
    assert!(phone.overview_open(), "the overview stays");
    assert_eq!(
        phone.current_tab(),
        TRIANGLE,
        "the tab that takes its place"
    );
    assert!(phone.shows("1 tab"), "{:?}", phone.labels());
    let survey: waos_package::ProgramId = SURVEY.parse().unwrap();
    phone.settle_until("survey stops", |phone| {
        futures::executor::block_on(phone.runs.running(&survey)).is_none()
    });
}

#[test]
fn a_card_dragged_a_little_and_let_go_springs_back() {
    let mut phone = overview_of_two();
    let on_survey = card_slot(1).center();
    phone.touch_down(1, on_survey);
    let dragged = on_survey + Offset::new(-50.0, 0.0);
    phone.slide(1, on_survey, dragged, 8);
    assert!(
        shown_part(&phone, SURVEYS).left < card_slot(1).left - 20.0,
        "the card follows the finger"
    );
    for _ in 0..12 {
        phone.touch_move(1, dragged);
    }
    phone.touch_up(1, dragged);
    phone.settle();
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY]);
    assert!(phone.overview_open(), "no tap after the drag");
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
}

#[test]
fn a_card_resists_a_drag_the_other_way() {
    let mut phone = overview_of_two();
    let on_triangle = card_slot(0).center();
    phone.touch_down(1, on_triangle);
    let dragged = on_triangle + Offset::new(140.0, 0.0);
    phone.slide(1, on_triangle, dragged, 10);
    let moved = shown_part(&phone, TRIANGLES).left - card_slot(0).left;
    assert!(
        moved > 5.0 && moved < 90.0,
        "less than the finger's: {moved}"
    );
    phone.touch_up(1, dragged);
    phone.settle();
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY]);
    assert_rect_near(
        shown_part(&phone, TRIANGLES),
        card_slot(0),
        "triangle's card",
    );
}

#[test]
fn a_finger_that_catches_a_thrown_card_keeps_its_tab() {
    let mut phone = overview_of_two();
    let on_survey = card_slot(1).center();
    phone.swipe(on_survey, on_survey + Offset::new(-90.0, 0.0), 4);
    phone.pump_frames(2);
    let flying = shown_part(&phone, SURVEYS);
    assert!(flying.right > 40.0, "still in reach: {flying:?}");
    let caught_at = Offset::new(flying.right - 20.0, flying.center().dy());
    phone.touch_down(1, caught_at);
    let back = caught_at + Offset::new(120.0, 0.0);
    phone.slide(1, caught_at, back, 10);
    for _ in 0..12 {
        phone.touch_move(1, back);
    }
    phone.touch_up(1, back);
    phone.settle();
    assert_eq!(phone.tab_kinds(), [TRIANGLE, SURVEY], "its tab is kept");
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
    assert_eq!(phone.stage(SURVEY), Some("running"));
}

/// A touch alone, with no drag: the card leaves no more, the cards that had started into its place
/// make room for it again, and the touch, a tap, grows nothing.
#[test]
fn a_touch_on_a_thrown_card_in_flight_keeps_its_tab_and_the_others_make_room_again() {
    let mut phone = phone_with_tabs(3);
    phone.tap(on_the_tabs_button());
    phone.settle();
    let on_the_second = card_slot(1).center();
    phone.swipe(on_the_second, on_the_second + Offset::new(-90.0, 0.0), 4);
    phone.pump_frames(2);
    let flying = shown_part(&phone, 1);
    assert!(flying.right > 40.0, "still in reach: {flying:?}");
    assert!(
        shown_part(&phone, 2).top < card_slot(2).top - 1.0,
        "the third card has started into its place"
    );
    let caught_at = Offset::new(flying.right - 20.0, flying.center().dy());
    phone.touch_down(1, caught_at);
    let held = page_left(&phone, 1);
    phone.pump_frames(4);
    assert_eq!(page_left(&phone, 1), held, "held where it was caught");
    phone.touch_up(1, caught_at);
    settle_without_a_jump(&mut phone, 2, 40.0);
    assert_eq!(phone.tab_kinds().len(), 3, "its tab is kept");
    assert_eq!(phone.stage(&nth_triangle(1).0), Some("running"));
    assert!(phone.overview_open(), "the touch only caught it");
    for card in 0..3 {
        assert_rect_near(
            shown_part(&phone, card),
            card_slot(card),
            "each card in its place",
        );
    }
    assert!(phone.shows("3 tabs"), "{:?}", phone.labels());
}

#[test]
fn the_cards_after_a_closed_one_move_into_its_place_without_a_jump() {
    let mut phone = phone_with_tabs(4);
    phone.tap(on_the_tabs_button());
    phone.settle();
    let on_the_second = card_slot(1).center();
    phone.swipe(on_the_second, on_the_second + Offset::new(-140.0, 0.0), 5);
    settle_without_a_jump(&mut phone, 2, 40.0);
    let (closed, _) = nth_triangle(1);
    assert!(!phone.tab_kinds().contains(&closed));
    assert_eq!(phone.tab_kinds().len(), 3);
    assert_rect_near(shown_part(&phone, 0), card_slot(0), "the first stays");
    assert_rect_near(
        shown_part(&phone, 2),
        card_slot(1),
        "the third takes its place",
    );
    assert_rect_near(
        shown_part(&phone, 3),
        card_slot(2),
        "and the fourth the third's",
    );
}

#[test]
fn several_cards_closed_quickly_leave_the_rest_in_their_places() {
    let mut phone = phone_with_tabs(5);
    phone.tap(on_the_tabs_button());
    phone.settle();
    let throw = Offset::new(-140.0, 0.0);
    for thrown in [0, 3, 2] {
        let on_the_card = shown_part(&phone, thrown).center();
        phone.swipe(on_the_card, on_the_card + throw, 4);
    }
    settle_without_a_jump(&mut phone, 1, 40.0);
    let (second, fifth) = (nth_triangle(1).0, nth_triangle(4).0);
    assert_eq!(phone.tab_kinds(), [second, fifth]);
    assert_rect_near(shown_part(&phone, 1), card_slot(0), "the second, first now");
    assert_rect_near(shown_part(&phone, 4), card_slot(1), "the fifth, second now");
    assert!(phone.shows("2 tabs"), "{:?}", phone.labels());
}

#[test]
fn the_current_tabs_card_thrown_leaves_the_overview_on_the_tab_that_takes_its_place() {
    let mut phone = overview_of_two();
    assert_eq!(phone.current_tab(), SURVEY);
    let on_survey = card_slot(1).center();
    phone.swipe(on_survey, on_survey + Offset::new(-140.0, 0.0), 5);
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert!(phone.overview_open());
    let done = phone.label_bounds("Done").unwrap().center();
    phone.tap(done);
    settle_without_a_jump(&mut phone, TRIANGLES, 60.0);
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
}

#[test]
fn the_last_programs_card_thrown_leaves_the_home_whose_card_cannot_be_thrown() {
    let mut phone =
        TestPhone::with_installed(&[(waos_test_programs::TRIANGLE, TRIANGLE, "Triangle")]);
    phone.open(TRIANGLE);
    phone.tap(on_the_tabs_button());
    phone.settle();
    let on_the_card = card_slot(0).center();
    phone.swipe(on_the_card, on_the_card + Offset::new(-140.0, 0.0), 5);
    phone.settle();
    assert_eq!(
        phone.tab_kinds(),
        ["home"],
        "the last tab closed leaves the home"
    );
    assert!(phone.overview_open());
    assert!(
        phone.shows("Home"),
        "its card's title: {:?}",
        phone.labels()
    );
    phone.swipe(on_the_card, on_the_card + Offset::new(-140.0, 0.0), 5);
    phone.settle();
    assert_eq!(phone.tab_kinds(), ["home"], "the home alone stays");
    let title = phone.label_bounds("Home").unwrap();
    assert_near(
        title.center().dx(),
        card_slot(0).center().dx(),
        "its card is back",
    );
}

#[test]
fn the_grid_scrolls_when_the_cards_do_not_fit_and_resists_past_its_ends() {
    let mut phone = phone_with_tabs(7);
    phone.tap(on_the_tabs_button());
    phone.settle();
    let scrolled = shown_part(&phone, 6).top - card_slot(6).top;
    assert!(
        scrolled < 0.0,
        "scrolled to show the current tab's card: {scrolled}"
    );
    let start = Offset::new(200.0, 300.0);
    phone.touch_down(1, start);
    let dragging = start + Offset::new(0.0, 40.0);
    phone.slide(1, start, dragging, 5);
    let before = shown_part(&phone, 4).top;
    let further = dragging + Offset::new(0.0, 100.0);
    phone.slide(1, dragging, further, 10);
    assert_near(
        shown_part(&phone, 4).top - before,
        100.0,
        "the grid moves with the finger",
    );
    assert!(phone.overview_open());
    phone.slide(1, further, further + Offset::new(0.0, 500.0), 20);
    let past_the_start = shown_part(&phone, 0).top - card_slot(0).top;
    assert!(
        past_the_start > 10.0 && past_the_start < 300.0,
        "past its start the grid resists: {past_the_start}"
    );
    phone.touch_up(1, further + Offset::new(0.0, 500.0));
    phone.settle();
    assert_rect_near(shown_part(&phone, 0), card_slot(0), "and springs back");
    assert_eq!(phone.tab_kinds().len(), 7, "no card was tapped or thrown");
}

#[test]
fn a_cards_throw_and_the_grids_scroll_compete_by_the_axis_the_finger_takes_first() {
    let mut phone = phone_with_tabs(7);
    phone.tap(on_the_tabs_button());
    phone.settle();
    let scroll = shown_part(&phone, 4).top;
    let card = shown_part(&phone, 4);
    let start = card.center();
    phone.touch_down(1, start);
    let sideways = start + Offset::new(-60.0, 0.0);
    phone.slide(1, start, sideways, 6);
    let down_too = sideways + Offset::new(0.0, 80.0);
    phone.slide(1, sideways, down_too, 8);
    assert_near(
        shown_part(&phone, 4).top,
        scroll,
        "sideways first: the grid stays",
    );
    assert!(
        page_left(&phone, 4) < card.left - 20.0,
        "and the card moves"
    );
    for _ in 0..12 {
        phone.touch_move(1, down_too);
    }
    phone.touch_up(1, down_too);
    phone.settle();

    let card = shown_part(&phone, 4);
    let start = card.center();
    phone.touch_down(1, start);
    let down = start + Offset::new(0.0, 60.0);
    phone.slide(1, start, down, 6);
    let sideways_too = down + Offset::new(-120.0, 0.0);
    phone.slide(1, down, sideways_too, 8);
    assert_near(
        shown_part(&phone, 4).left,
        card.left,
        "downward first: the card stays",
    );
    assert!(
        shown_part(&phone, 4).top > card.top + 20.0,
        "and the grid moves"
    );
    phone.touch_up(1, sideways_too);
    phone.settle();
    assert_eq!(phone.tab_kinds().len(), 7);
    assert!(phone.overview_open());
}

#[test]
fn a_program_opened_from_outside_closes_the_overview_onto_its_page() {
    let mut phone = overview_of_two();
    let anywhere = Offset::new(200.0, 700.0);
    phone.touch_down(1, anywhere);
    phone.start_opening(TRIANGLE);
    assert!(!phone.overview_open());
    assert_eq!(phone.current_tab(), TRIANGLE);
    phone.slide(1, anywhere, anywhere + Offset::new(0.0, -200.0), 8);
    phone.touch_up(1, anywhere + Offset::new(0.0, -200.0));
    settle_without_a_jump(&mut phone, TRIANGLES, 60.0);
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
    assert!(phone.program_view(TRIANGLE).presence().focused);
}

#[test]
fn the_view_turning_with_the_overview_open_lays_the_cards_out_again() {
    let mut phone = overview_of_two();
    phone.resize(SCREEN[1], SCREEN[0]);
    phone.settle();
    let turned = DeckLayout::new(Size::new(SCREEN[1], SCREEN[0]), STATUS_BAR, HOME_INDICATOR);
    assert_eq!(turned.columns(), 4);
    assert_rect_near(
        shown_part(&phone, TRIANGLES),
        turned.card_rect(0.0, 0.0),
        "triangle",
    );
    assert_rect_near(
        shown_part(&phone, SURVEYS),
        turned.card_rect(1.0, 0.0),
        "survey",
    );
    assert!(phone.overview_open());
}

#[test]
fn a_program_holds_the_keyboard_only_while_the_deck_rests_on_its_page() {
    let mut phone = two_programs_running();
    assert!(phone.keyboard_is_a_programs(), "survey's, on its page");
    phone.tap(on_the_tabs_button());
    phone.settle();
    assert!(
        !phone.keyboard_is_a_programs(),
        "nothing has the focus in the overview"
    );
    phone.tap(card_slot(0).center());
    phone.settle();
    assert!(
        phone.keyboard_is_a_programs(),
        "triangle's, once the deck rests on its page"
    );
    phone.show(None);
    assert!(!phone.keyboard_is_a_programs(), "the home is no program's");
}

/// A phone with triangle installed, resting on the home.
fn home_with_triangle_installed() -> TestPhone {
    TestPhone::with_installed(&[(waos_test_programs::TRIANGLE, TRIANGLE, "Triangle")])
}

/// Taps the plus on the overview's toolbar.
fn tap_the_plus(phone: &mut TestPhone) {
    let plus = phone
        .bounds_of::<waft::Icon>(|icon| icon.icon == Some(CupertinoIcons::add()))
        .expect("the plus, on the toolbar")
        .center();
    phone.tap(plus);
}

#[test]
fn a_tap_on_a_programs_icon_on_the_home_turns_the_homes_tab_into_the_programs() {
    let mut phone = home_with_triangle_installed();
    assert_eq!(phone.tab_kinds(), ["home"]);
    let icon = phone
        .label_bounds("Triangle")
        .expect("its icon, on the home");
    assert!(
        icon.bottom < page_box().bottom,
        "on the home's page: {icon:?}"
    );
    phone.tap(icon.center());
    phone.settle_until("triangle runs in its tab", |phone| {
        phone.stage(TRIANGLE) == Some("running")
    });
    assert_eq!(
        phone.tab_kinds(),
        [TRIANGLE],
        "the same tab, the program's now"
    );
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
    let pill = phone.label_bounds("Triangle").expect("the pill names it");
    assert!(pill.top > page_box().bottom, "{pill:?}");
    let triangle = phone.program_view(TRIANGLE);
    assert!(triangle.presence().on_screen && triangle.presence().focused);
}

#[test]
fn the_plus_goes_to_the_home_which_it_makes_only_where_none_is_open() {
    let mut phone = home_with_triangle_installed();
    phone.open(TRIANGLE);
    phone.tap(on_the_tabs_button());
    phone.settle();
    tap_the_plus(&mut phone);
    assert_eq!(phone.tab_kinds(), [TRIANGLE, "home"], "made at the end");
    assert!(!phone.overview_open());
    phone.settle();
    assert_eq!(phone.current_tab(), "home");
    assert_eq!(phone.picture(TRIANGLES), None);
    assert!(phone.shows("WaOS"), "the home's page: {:?}", phone.labels());
    assert!(!phone.program_view(TRIANGLE).presence().on_screen);

    phone.tap(on_the_tabs_button());
    phone.settle();
    phone.tap(card_slot(0).center());
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
    phone.tap(on_the_tabs_button());
    phone.settle();
    tap_the_plus(&mut phone);
    phone.settle();
    assert_eq!(
        phone.tab_kinds(),
        [TRIANGLE, "home"],
        "there is one home at most"
    );
    assert_eq!(phone.current_tab(), "home");
}

#[test]
fn a_program_opened_again_from_the_home_brings_its_tab_into_sight() {
    let mut phone = home_with_triangle_installed();
    phone.open(TRIANGLE);
    phone.show(None);
    assert_eq!(phone.current_tab(), "home");
    let view = phone.program_view(TRIANGLE);
    let icon = phone
        .label_bounds("Triangle")
        .expect("its icon, on the home");
    phone.tap(icon.center());
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert_eq!(
        phone.tab_kinds(),
        [TRIANGLE, "home"],
        "no second tab, and the home stays"
    );
    assert!(view.presence().on_screen && !view.presence().focused);
    settle_without_a_jump(&mut phone, TRIANGLES, 60.0);
    assert_rect_near(shown_part(&phone, TRIANGLES), page_box(), "triangle's page");
    assert!(view.presence().focused);
    assert_eq!(
        phone.program_view(TRIANGLE).id(),
        view.id(),
        "the same view"
    );
}

#[test]
fn a_program_that_asks_for_a_switch_sends_the_deck_home_to_its_settings_page() {
    let mut phone = home_with_triangle_installed();
    phone.open(TRIANGLE);
    let id: waos_package::ProgramId = TRIANGLE.parse().unwrap();
    let program = futures::executor::block_on(phone.runs.running(&id)).expect("it runs");
    let (asked, opened) = futures::channel::mpsc::unbounded();
    let (left, mut hears_left) = futures::channel::oneshot::channel();
    open_settings_when_asked(&mut phone.shell.app(), &phone.library, opened);
    asked
        .unbounded_send(SettingsOpened {
            program,
            switch: waos::Switch::Network,
            left,
        })
        .unwrap();
    phone.settle();
    assert_eq!(phone.tab_kinds(), [TRIANGLE, "home"]);
    assert_eq!(phone.current_tab(), "home");
    assert!(
        phone.shows("Triangle's settings"),
        "its page, on the home: {:?}",
        phone.labels()
    );
    assert_eq!(hears_left.try_recv(), Ok(None), "the user is still there");

    let start = on_the_bar();
    phone.swipe(start, start + Offset::new(120.0, 0.0), 5);
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
    assert_eq!(
        hears_left.try_recv(),
        Ok(Some(())),
        "the user left the page"
    );
}

#[test]
fn a_home_opened_after_the_last_one_went_starts_on_the_grid_of_programs() {
    let mut phone = home_with_triangle_installed();
    phone.library.update(&mut phone.shell.app(), |library, cx| {
        library.show_page(LibraryPage::GetPrograms, cx)
    });
    phone.settle();
    assert!(phone.shows("Get programs"), "{:?}", phone.labels());
    phone.open(TRIANGLE);
    phone.show(None);
    assert_eq!(phone.current_tab(), "home");
    assert!(
        phone
            .label_bounds("Triangle")
            .is_some_and(|icon| icon.bottom < page_box().bottom),
        "the grid of programs: {:?}",
        phone.labels()
    );
}

/// A phone with triangle and survey installed, resting on the home.
fn home_with_two_installed() -> TestPhone {
    TestPhone::with_installed(&[
        (waos_test_programs::TRIANGLE, TRIANGLE, "Triangle"),
        (waos_test_programs::SURVEY, SURVEY, "Survey"),
    ])
}

/// Whether the home's page shows the icon of the program named `name`.
fn home_shows(phone: &TestPhone, name: &str) -> bool {
    phone
        .label_bounds(name)
        .is_some_and(|icon| icon.bottom < page_box().bottom)
}

#[test]
fn on_the_home_the_pill_is_a_search_field_that_filters_the_programs_as_the_user_types() {
    let mut phone = home_with_two_installed();
    let pill = phone
        .label_bounds("Search programs")
        .expect("the search field, in the bar");
    assert!(pill.top > page_box().bottom, "{pill:?}");
    assert!(!phone.keyboard_asked_for());
    phone.tap(on_the_bar());
    phone.settle();
    assert!(
        phone.keyboard_asked_for(),
        "a tap gives the field the keyboard"
    );
    phone.type_text("tri");
    assert!(home_shows(&phone, "Triangle") && !home_shows(&phone, "Survey"));
    phone.type_text("SUR");
    assert!(!home_shows(&phone, "Triangle") && home_shows(&phone, "Survey"));
    phone.type_text("neither");
    assert!(!home_shows(&phone, "Triangle") && !home_shows(&phone, "Survey"));
    assert!(phone.shows("No programs found."), "{:?}", phone.labels());
    phone.type_text("");
    assert!(home_shows(&phone, "Triangle") && home_shows(&phone, "Survey"));
}

#[test]
fn the_bar_stays_above_the_keyboard_while_the_search_field_has_it() {
    let mut phone = home_with_two_installed();
    let title_before = phone.label_bounds("WaOS").unwrap();
    phone.tap(on_the_bar());
    phone.settle();
    let keyboard = 300.0;
    phone.cover(STATUS_BAR, HOME_INDICATOR, keyboard);
    phone.settle();
    phone.type_text("tri");
    let typed = phone
        .bounds_of::<waft_cupertino::CupertinoTextField>(|_| true)
        .expect("the search field");
    assert!(
        typed.bottom <= SCREEN[1] - keyboard && typed.top > SCREEN[1] - keyboard - BAR_HEIGHT,
        "just above the keyboard: {typed:?}"
    );
    assert_eq!(
        phone.label_bounds("WaOS").unwrap(),
        title_before,
        "the page stays as it is laid out"
    );
    phone.cover(STATUS_BAR, HOME_INDICATOR, 0.0);
    phone.settle();
    let typed = phone
        .bounds_of::<waft_cupertino::CupertinoTextField>(|_| true)
        .unwrap();
    assert!(
        typed.top > page_box().bottom,
        "back in its place: {typed:?}"
    );
}

#[test]
fn a_program_found_by_searching_takes_the_homes_tab_and_the_next_home_searches_for_nothing() {
    let mut phone = home_with_two_installed();
    phone.tap(on_the_bar());
    phone.settle();
    phone.type_text("tri");
    let icon = phone.label_bounds("Triangle").unwrap().center();
    phone.tap(icon);
    phone.settle_until("triangle runs in its tab", |phone| {
        phone.stage(TRIANGLE) == Some("running")
    });
    assert_eq!(phone.tab_kinds(), [TRIANGLE]);
    assert!(!phone.keyboard_asked_for(), "the field went with the home");
    phone.show(None);
    assert!(
        home_shows(&phone, "Triangle") && home_shows(&phone, "Survey"),
        "every program again: {:?}",
        phone.labels()
    );
    assert!(phone.shows("Search programs"), "an empty field");
}

#[test]
fn a_drag_on_the_bar_takes_the_keyboard_from_the_search_field() {
    let mut phone = home_with_two_installed();
    phone.open(TRIANGLE);
    phone.show(None);
    phone.tap(on_the_bar());
    phone.settle();
    assert!(phone.keyboard_asked_for());
    let start = Offset::new(SCREEN[0] - 34.0, on_the_bar().dy());
    phone.swipe(start, start + Offset::new(160.0, 0.0), 6);
    assert!(!phone.keyboard_asked_for(), "the drag is the bar's");
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
}

#[test]
fn a_sideways_drag_on_the_search_field_moves_the_strip_and_a_tap_does_not() {
    let mut phone = home_with_two_installed();
    phone.open(TRIANGLE);
    phone.show(None);
    let start = on_the_bar();
    phone.swipe(start, start + Offset::new(160.0, 0.0), 6);
    assert!(!phone.keyboard_asked_for(), "a drag is no tap");
    phone.settle();
    assert_eq!(phone.current_tab(), TRIANGLE);
}

#[test]
fn a_keyboard_up_for_a_program_moves_neither_its_page_nor_the_bar() {
    let mut phone = two_programs_running();
    let told = phone.program_view(SURVEY).metrics();
    let pill = phone.label_bounds("Survey").unwrap();
    phone.cover(STATUS_BAR, HOME_INDICATOR, 300.0);
    phone.settle();
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
    assert_eq!(phone.label_bounds("Survey").unwrap(), pill, "the bar stays");
    let now = phone.program_view(SURVEY).metrics();
    assert_eq!((now.width, now.height), (told.width, told.height));
    assert_eq!(
        now.view_insets.bottom,
        (300.0 - HOME_INDICATOR - BAR_HEIGHT) * 3.0,
        "told what the keyboard covers of its page"
    );
}

/// How far across the page of the `nth` program opened the screen's `x` is, as a share of the
/// page's width as the last frame drew it.
fn share_across(phone: &TestPhone, nth: usize, x: f64) -> f64 {
    let page = phone
        .picture(nth)
        .unwrap_or_else(|| panic!("program {nth}'s page is not drawn"))
        .bounds;
    (x - page.left) / page.width()
}

/// Moves the finger `finger` from `from` to `to` over `frames` frames, and at each checks that
/// the page of the `nth` program stays under it: the point `across` the page above the finger,
/// and the page's bottom edge as far from the finger as `beneath` says.
fn slide_with_the_page_under_the_finger(
    phone: &mut TestPhone,
    nth: usize,
    (from, to): (Offset, Offset),
    frames: u32,
    (across, beneath): (f64, f64),
) {
    for frame in 1..=frames {
        let finger = from + (to - from) * (f64::from(frame) / f64::from(frames));
        phone.touch_move(1, finger);
        let page = phone.picture(nth).expect("the page is drawn");
        assert!(
            (share_across(phone, nth, finger.dx()) - across).abs() < 0.004,
            "frame {frame}: the finger at {finger:?} is {} across {:?}, not {across}",
            share_across(phone, nth, finger.dx()),
            page.bounds
        );
        assert_near(
            finger.dy() - page.visible.bottom,
            beneath,
            "the page's edge stays as far above the finger",
        );
    }
}

#[test]
fn a_diagonal_drag_on_the_bar_keeps_the_page_under_the_finger_both_ways() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(10.0, -40.0);
    phone.slide(1, start, dragging, 5);
    let across = share_across(&phone, SURVEYS, dragging.dx());
    let beneath = dragging.dy() - shown_part(&phone, SURVEYS).bottom;
    let up_and_right = Offset::new(300.0, 520.0);
    slide_with_the_page_under_the_finger(
        &mut phone,
        SURVEYS,
        (dragging, up_and_right),
        20,
        (across, beneath),
    );
    let page = phone.picture(SURVEYS).unwrap();
    assert!(
        page.scale < 0.75,
        "shrunk by the upward travel: {}",
        page.scale
    );
    let down_and_left = Offset::new(60.0, 600.0);
    slide_with_the_page_under_the_finger(
        &mut phone,
        SURVEYS,
        (up_and_right, down_and_left),
        20,
        (across, beneath),
    );
    phone.touch_up(1, down_and_left);
    phone.settle();
}

/// The pictures of the first `count` programs opened, as the last frame placed them.
fn pictures(phone: &TestPhone, count: usize) -> Vec<Option<Picture>> {
    (0..count).map(|nth| phone.picture(nth)).collect()
}

/// Moves the finger `finger` from `from` to `to` over `frames` frames, and at each checks that
/// the first programs opened are drawn as `standing` says.
fn slide_over_a_standing_grid(
    phone: &mut TestPhone,
    (from, to): (Offset, Offset),
    frames: u32,
    standing: &[Option<Picture>],
) {
    for frame in 1..=frames {
        let finger = from + (to - from) * (f64::from(frame) / f64::from(frames));
        phone.touch_move(1, finger);
        assert_eq!(
            pictures(phone, standing.len()),
            standing,
            "the other cards as the finger goes to {finger:?}"
        );
    }
}

/// The grid stands still: from the first frame of a page's flight every other tab is its card, in
/// its slot at its size, and nothing the finger does to its page, or the page does once let go,
/// moves one of them.
#[test]
fn every_other_card_stands_still_while_a_page_is_dragged_up_and_about_and_let_go() {
    let mut phone = phone_with_tabs(4);
    let flying = 3;
    let start = on_the_bar();
    phone.touch_down(1, start);
    let mut finger = start;
    while shown_part(&phone, flying) == page_box() {
        finger = finger + Offset::new(0.0, -6.0);
        phone.touch_move(1, finger);
        assert!(finger.dy() > start.dy() - 120.0, "the drag never started");
    }
    let standing = pictures(&phone, flying);
    for (slot, card) in standing.iter().enumerate() {
        let card = card.expect("every other tab is drawn, behind the page");
        assert_rect_near(card.visible, card_slot(slot), "a card in its slot");
    }
    let far_up = Offset::new(start.dx(), 60.0);
    slide_over_a_standing_grid(&mut phone, (finger, far_up), 40, &standing);
    let up_and_right = Offset::new(340.0, 300.0);
    slide_over_a_standing_grid(&mut phone, (far_up, up_and_right), 20, &standing);
    let down_and_left = Offset::new(40.0, 560.0);
    slide_over_a_standing_grid(&mut phone, (up_and_right, down_and_left), 20, &standing);
    slide_over_a_standing_grid(&mut phone, (down_and_left, down_and_left), 12, &standing);
    assert_eq!(
        phone.pictures_in_paint_order().last(),
        Some(&flying),
        "the page in flight is drawn above every card"
    );
    phone.touch_up(1, down_and_left);
    assert!(phone.overview_open());
    for frame in 0..600 {
        let asked = phone.pump();
        assert_eq!(
            pictures(&phone, flying),
            standing,
            "frame {frame} after the release"
        );
        if !asked {
            break;
        }
    }
    assert_rect_near(
        shown_part(&phone, flying),
        card_slot(flying),
        "its own slot",
    );
}

/// The other cards stand still to the end of a flight back, too: the page covers them where they
/// are.
#[test]
fn every_other_card_stands_still_while_a_page_let_go_low_flies_back() {
    let mut phone = phone_with_tabs(3);
    let flying = 2;
    let start = on_the_bar();
    let end = start + Offset::new(-90.0, -120.0);
    phone.touch_down(1, start);
    phone.slide(1, start, end, 12);
    let standing = pictures(&phone, flying);
    assert!(standing.iter().all(Option::is_some));
    slide_over_a_standing_grid(&mut phone, (end, end), 12, &standing);
    phone.touch_up(1, end);
    assert!(!phone.overview_open());
    while shown_part(&phone, flying) != page_box() {
        assert_eq!(pictures(&phone, flying), standing, "as the page flies back");
        assert!(phone.pump(), "the page never got back");
    }
    phone.settle();
    assert_eq!(
        pictures(&phone, flying),
        [None, None],
        "behind the page, nothing"
    );
}

/// A page is never drawn smaller than its card, but a little while a finger holds it past its
/// card's size against resistance; and no other card's size is the finger's to change.
#[test]
fn a_page_dragged_far_up_is_its_cards_size_and_only_a_little_smaller_against_resistance() {
    let mut phone = two_programs_running();
    let card = card_slot(1).size();
    let start = on_the_bar();
    let top = Offset::new(start.dx(), 4.0);
    phone.touch_down(1, start);
    let mut width = page_box().width();
    for frame in 1..=60 {
        phone.touch_move(1, start + (top - start) * (f64::from(frame) / 60.0));
        let page = shown_part(&phone, SURVEYS);
        assert!(
            page.width() <= width && page.width() > card.width() * 0.85,
            "frame {frame}: {page:?}, after a page {width} wide, for a card {card:?}"
        );
        width = page.width();
    }
    assert!(
        width < card.width() - 4.0,
        "a little smaller than its card, far past it: {width}"
    );
    assert_rect_near(
        shown_part(&phone, TRIANGLES),
        card_slot(0),
        "the other card, its own size",
    );
    phone.touch_up(1, top);
    assert!(phone.overview_open());
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
}

/// The finger carries its page alone: the grid behind it stands still, and a finger that only
/// moves sideways moves nothing else.
#[test]
fn the_other_cards_stay_in_their_places_while_a_finger_carries_its_page_sideways() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let dragging = start + Offset::new(0.0, -40.0);
    phone.slide(1, start, dragging, 5);
    let across = share_across(&phone, SURVEYS, dragging.dx());
    let beneath = dragging.dy() - shown_part(&phone, SURVEYS).bottom;
    let up = Offset::new(start.dx(), 500.0);
    slide_with_the_page_under_the_finger(
        &mut phone,
        SURVEYS,
        (dragging, up),
        20,
        (across, beneath),
    );
    let beside = phone
        .picture(TRIANGLES)
        .expect("the card before it has come into view at its place in the grid");
    let survey_in_place = phone.picture(SURVEYS).unwrap();

    for sideways in [Offset::new(330.0, up.dy()), Offset::new(30.0, up.dy())] {
        let from = Offset::new(start.dx(), up.dy());
        for frame in 1..=12 {
            let finger = from + (sideways - from) * (f64::from(frame) / 12.0);
            phone.touch_move(1, finger);
            assert_eq!(
                phone.picture(TRIANGLES),
                Some(beside),
                "the other card stays where it is as the finger goes to {finger:?}"
            );
            assert!(
                (share_across(&phone, SURVEYS, finger.dx()) - across).abs() < 0.004,
                "while its own page goes with the finger"
            );
        }
        let carried = phone.picture(SURVEYS).unwrap();
        assert_near(
            carried.bounds.left - survey_in_place.bounds.left,
            sideways.dx() - start.dx(),
            "the page is carried as far as the finger went",
        );
        assert_eq!(
            phone.pictures_in_paint_order(),
            [TRIANGLES, SURVEYS],
            "and is drawn above the cards it passes over"
        );
        assert!(
            clips_the_page(&phone, SURVEYS) && clips_the_page(&phone, TRIANGLES),
            "each clipped to its own card, where it is"
        );
        for frame in 1..=12 {
            let finger = sideways + (from - sideways) * (f64::from(frame) / 12.0);
            phone.touch_move(1, finger);
        }
    }
    assert_eq!(phone.picture(TRIANGLES), Some(beside));
    phone.touch_up(1, Offset::new(start.dx(), up.dy()));
    phone.settle();
}

#[test]
fn a_page_dragged_sideways_goes_to_its_cards_slot_at_the_release_without_a_jump() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let end = Offset::new(60.0, 420.0);
    phone.touch_down(1, start);
    phone.slide(1, start, end, 24);
    let dragged = phone.picture(SURVEYS).unwrap();
    assert!(
        dragged.bounds.left < card_slot(1).left - 100.0,
        "far from its slot, under the finger: {dragged:?}"
    );
    phone.touch_up(1, end);
    assert!(phone.overview_open());
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
    assert_rect_near(
        shown_part(&phone, TRIANGLES),
        card_slot(0),
        "triangle's card",
    );
}

#[test]
fn a_page_flicked_sideways_and_up_carries_the_fingers_speed_into_the_settle() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let end = start + Offset::new(-120.0, -160.0);
    phone.touch_down(1, start);
    phone.slide(1, start, end, 8);
    let before = phone.picture(SURVEYS).unwrap().bounds;
    phone.touch_up(1, end);
    phone.pump();
    let after = phone.picture(SURVEYS).unwrap().bounds;
    assert!(
        after.left < before.left && after.left > before.left - 30.0,
        "it goes on leftwards as the finger was going: {} then {}",
        before.left,
        after.left
    );
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
}

#[test]
fn a_page_dragged_sideways_and_let_go_low_goes_back_to_fill_its_box() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let end = start + Offset::new(140.0, -80.0);
    phone.drag_and_let_go(start, end, 20);
    assert!(!phone.overview_open());
    assert!(
        !phone.program_view(SURVEY).presence().focused || phone.picture(SURVEYS).is_some(),
        "still on its way"
    );
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
    assert!(phone.program_view(SURVEY).presence().focused);
    assert!(phone.keyboard_is_a_programs());
}

/// The phone with survey's page dragged up and to the left and let go moving upward, two frames
/// into its settle: the overview opening, the page on its way back to its slot.
fn a_carried_page_settling() -> TestPhone {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    let end = Offset::new(70.0, 600.0);
    phone.touch_down(1, start);
    phone.slide(1, start, end, 20);
    phone.touch_up(1, end);
    assert!(phone.overview_open());
    phone.pump_frames(2);
    phone
}

#[test]
fn a_touch_on_the_carried_page_as_it_settles_takes_it_over_from_where_it_is() {
    let mut phone = a_carried_page_settling();
    let finger = shown_part(&phone, SURVEYS).center();
    phone.touch_down(1, finger);
    let caught = phone.picture(SURVEYS).unwrap();
    let across = share_across(&phone, SURVEYS, finger.dx());
    phone.pump_frames(5);
    assert_eq!(phone.picture(SURVEYS).unwrap(), caught, "held where it was");
    let dragging = finger + Offset::new(0.0, 30.0);
    phone.slide(1, finger, dragging, 4);
    let taken = phone.picture(SURVEYS).unwrap();
    let lowered = taken.visible.bottom - caught.visible.bottom;
    assert!(
        (-0.75..=30.75).contains(&lowered),
        "the drag takes its edge from where it was held: {caught:?} then {taken:?}"
    );
    assert!(
        (share_across(&phone, SURVEYS, dragging.dx()) - across).abs() < 0.004,
        "and the point of the page the finger came down on stays under it"
    );
    let beneath = dragging.dy() - taken.visible.bottom;
    let down_and_right = Offset::new(320.0, dragging.dy() + 40.0);
    slide_with_the_page_under_the_finger(
        &mut phone,
        SURVEYS,
        (dragging, down_and_right),
        15,
        (across, beneath),
    );
    phone.touch_up(1, down_and_right);
    phone.settle();
}

/// A finger on the overview away from the page in flight has no page under it: it catches
/// nothing, and the page goes on to its slot whatever that finger does.
#[test]
fn a_touch_elsewhere_on_the_overview_leaves_the_page_in_flight_going() {
    let mut phone = a_carried_page_settling();
    let elsewhere = Offset::new(300.0, SCREEN[1] - 60.0);
    let in_flight = phone.picture(SURVEYS).unwrap();
    assert!(
        !in_flight.visible.contains(elsewhere),
        "no page under the finger"
    );
    phone.touch_down(1, elsewhere);
    phone.pump_frames(3);
    let going = phone.picture(SURVEYS).unwrap();
    assert!(
        going.scale < in_flight.scale && going.bounds.left > in_flight.bounds.left,
        "the page goes on shrinking, towards its slot: {in_flight:?} then {going:?}"
    );
    let dragged = elsewhere + Offset::new(-200.0, 120.0);
    phone.slide(1, elsewhere, dragged, 10);
    phone.touch_up(1, dragged);
    assert!(
        phone.overview_open(),
        "the finger sent the deck nowhere else"
    );
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert_rect_near(shown_part(&phone, SURVEYS), card_slot(1), "survey's card");
}

#[test]
fn a_card_in_a_fingers_hand_springs_back_when_the_deck_is_sent_elsewhere() {
    let mut phone = phone_with_tabs(3);
    phone.tap(on_the_tabs_button());
    phone.settle();
    let on_the_second = card_slot(1).center();
    phone.touch_down(1, on_the_second);
    let dragged = on_the_second + Offset::new(-60.0, 0.0);
    phone.slide(1, on_the_second, dragged, 6);
    assert!(
        page_left(&phone, 1) < card_slot(1).left - 30.0,
        "the card follows the finger"
    );
    phone.close(&nth_triangle(2).0);
    assert_eq!(
        phone.tab_kinds().len(),
        2,
        "the current tab was closed from outside"
    );
    assert!(phone.overview_open());
    assert_rect_near(
        shown_part(&phone, 1),
        card_slot(1),
        "the card is back in its place",
    );
    phone.slide(1, dragged, dragged + Offset::new(-80.0, 0.0), 6);
    phone.touch_up(1, dragged + Offset::new(-80.0, 0.0));
    phone.settle();
    assert_eq!(
        phone.tab_kinds().len(),
        2,
        "the finger still down did nothing more"
    );
    assert_rect_near(shown_part(&phone, 1), card_slot(1), "and its card stays");
}

/// The size of the track the bar's pills slide in.
const PILL_TRACK: [f64; 2] = [314.0, 42.0];

/// Whether the last frame draws a clip of about `size`.
fn clips_to(phone: &TestPhone, size: [f64; 2]) -> bool {
    phone
        .clips_drawn()
        .iter()
        .any(|clip| (clip[0] - size[0]).abs() < 0.75 && (clip[1] - size[1]).abs() < 0.75)
}

/// Whether the last frame clips the page of the `nth` program opened to what shows of it, a clip
/// recorded in the page's own points, as the page is laid out before it is scaled.
fn clips_the_page(phone: &TestPhone, nth: usize) -> bool {
    let shown = shown_part(phone, nth);
    let scale = phone.picture(nth).expect("the page is drawn").scale;
    clips_to(phone, [shown.width() / scale, shown.height() / scale])
}

/// A clip costs the renderer a pass over the whole picture, so at rest the deck draws none of its
/// own: what is left is the program's icon in its pill, rounded.
#[test]
fn at_rest_on_a_programs_page_the_deck_clips_neither_the_page_nor_the_pills() {
    let phone = two_programs_running();
    assert_eq!(
        phone.clips_drawn(),
        [[20.0, 20.0]],
        "the icon in the pill alone"
    );
    assert_rect_near(
        shown_part(&phone, SURVEYS),
        page_box(),
        "the page shows whole",
    );
}

/// What is left on the home is the library's own: a program's icon, rounded, and its navigation
/// bar.
#[test]
fn at_rest_on_the_home_the_deck_clips_neither_the_page_nor_the_pills() {
    let phone = home_with_triangle_installed();
    assert_eq!(
        phone.clips_drawn(),
        [[60.0, 60.0], [SCREEN[0], STATUS_BAR + 44.0]],
        "the program's icon and the library's navigation bar alone"
    );
}

#[test]
fn a_page_is_clipped_again_from_the_first_frame_the_overview_opens() {
    let mut phone = two_programs_running();
    assert!(!clips_the_page(&phone, SURVEYS));
    phone.tap(on_the_tabs_button());
    let mut frames = 0;
    while shown_part(&phone, SURVEYS) == page_box() {
        phone.pump();
        frames += 1;
        assert!(frames < 30, "the overview never started to open");
    }
    assert!(
        clips_the_page(&phone, SURVEYS),
        "the first frame it shrinks: {:?} in {:?}",
        shown_part(&phone, SURVEYS),
        phone.clips_drawn()
    );
    phone.settle();
    assert!(clips_the_page(&phone, SURVEYS), "and as a card");
    let done = phone.label_bounds("Done").unwrap().center();
    phone.tap(done);
    phone.settle();
    assert_eq!(
        phone.clips_drawn(),
        [[20.0, 20.0]],
        "at rest on the page again"
    );
}

#[test]
fn a_page_is_clipped_again_from_the_first_frame_a_finger_drags_it_into_the_overview() {
    let mut phone = two_programs_running();
    let start = on_the_bar();
    phone.touch_down(1, start);
    let mut finger = start;
    while shown_part(&phone, SURVEYS) == page_box() {
        finger = finger + Offset::new(0.0, -6.0);
        phone.touch_move(1, finger);
        assert!(finger.dy() > start.dy() - 120.0, "the drag never started");
    }
    assert!(clips_the_page(&phone, SURVEYS), "{:?}", phone.clips_drawn());
    phone.touch_up(1, finger);
    phone.settle();
    assert_eq!(phone.clips_drawn(), [[20.0, 20.0]], "back at rest");
}

#[test]
fn the_bar_clips_its_pills_again_from_the_first_frame_the_strip_moves() {
    let mut phone = two_programs_running();
    assert!(!clips_to(&phone, PILL_TRACK));
    let start = on_the_bar();
    phone.touch_down(1, start);
    let mut finger = start;
    while page_left(&phone, SURVEYS) == 0.0 {
        finger = finger + Offset::new(6.0, 0.0);
        phone.touch_move(1, finger);
        assert!(finger.dx() < start.dx() + 120.0, "the drag never started");
    }
    assert!(
        clips_to(&phone, PILL_TRACK),
        "the first frame the strip is off its page: {:?}",
        phone.clips_drawn()
    );
    assert!(
        !clips_the_page(&phone, SURVEYS),
        "a page on the strip is whole, and needs no clip"
    );
    phone.touch_up(1, finger);
    phone.pump_frames(3);
    assert!(clips_to(&phone, PILL_TRACK), "while the strip settles");
    phone.settle();
    assert_eq!(
        phone.clips_drawn(),
        [[20.0, 20.0]],
        "at rest on a page again"
    );
}

#[test]
fn back_closes_the_open_overview_as_done_does() {
    let mut phone = overview_of_two();
    assert!(phone.back(), "the deck handled it");
    assert!(!phone.overview_open());
    let survey = phone.program_view(SURVEY);
    settle_without_a_jump(&mut phone, SURVEYS, 45.0);
    assert_eq!(phone.current_tab(), SURVEY);
    assert!(survey.presence().on_screen && survey.presence().focused);
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
}

#[test]
fn back_closes_the_overview_while_it_still_opens() {
    let mut phone = two_programs_running();
    phone.tap(on_the_tabs_button());
    phone.pump_frames(4);
    assert!(phone.overview_open(), "opening");
    assert!(phone.back());
    assert!(!phone.overview_open());
    phone.settle();
    assert_eq!(phone.current_tab(), SURVEY);
    assert_rect_near(shown_part(&phone, SURVEYS), page_box(), "survey's page");
}

/// The phone with `examples/back` running in a tab, a page past its first, so it says it handles
/// Back.
fn back_running_past_its_first_page() -> TestPhone {
    let mut phone = TestPhone::with_installed(&[(waos_test_programs::BACK, BACK, "Back")]);
    phone.open(BACK);
    assert!(!phone.handles_back(BACK), "on its first page");
    phone.tap(page_box().center());
    phone.settle_until("it says it handles Back", |phone| phone.handles_back(BACK));
    phone
}

#[test]
fn back_on_the_page_of_a_program_that_handles_it_is_the_programs() {
    let mut phone = back_running_past_its_first_page();
    let view = phone.program_view(BACK);
    assert!(phone.back(), "the deck handled it, by sending it on");
    phone.settle_until("the program went back to its first page", |phone| {
        !phone.handles_back(BACK)
    });
    assert!(!view.handles_back());
    assert_eq!(phone.current_tab(), BACK, "the deck stays on its page");
    assert_eq!(phone.tab_kinds(), [BACK]);
    assert!(view.presence().on_screen && view.presence().focused);
}

#[test]
fn back_on_the_page_of_a_program_that_does_not_handle_it_goes_home_and_the_program_runs_on() {
    let mut phone = home_with_triangle_installed();
    phone.open(TRIANGLE);
    let triangle = phone.program_view(TRIANGLE);
    assert!(phone.back());
    phone.settle();
    assert_eq!(
        phone.tab_kinds(),
        [TRIANGLE, "home"],
        "the home made at the end"
    );
    assert_eq!(phone.current_tab(), "home");
    assert!(phone.shows("WaOS"), "the home's page: {:?}", phone.labels());
    assert_eq!(phone.stage(TRIANGLE), Some("running"), "it keeps its tab");
    assert!(!triangle.presence().on_screen);

    phone.show(Some(TRIANGLE));
    assert!(phone.back());
    phone.settle();
    assert_eq!(
        phone.tab_kinds(),
        [TRIANGLE, "home"],
        "the home open is the one it goes to"
    );
    assert_eq!(phone.current_tab(), "home");
}

#[test]
fn back_once_a_program_has_nowhere_left_to_go_back_to_goes_home() {
    let mut phone = back_running_past_its_first_page();
    assert!(phone.back());
    phone.settle_until("the program went back to its first page", |phone| {
        !phone.handles_back(BACK)
    });
    assert!(phone.back());
    phone.settle();
    assert_eq!(phone.current_tab(), "home");
    assert_eq!(phone.stage(BACK), Some("running"));
}

#[test]
fn back_on_the_home_goes_to_its_first_page_and_from_there_is_the_systems() {
    let mut phone = home_with_triangle_installed();
    phone.library.update(&mut phone.shell.app(), |library, cx| {
        library.show_page(LibraryPage::GetPrograms, cx)
    });
    phone.settle();
    assert!(phone.shows("Get programs"), "{:?}", phone.labels());
    assert!(phone.back());
    phone.settle();
    assert!(
        home_shows(&phone, "Triangle"),
        "the grid: {:?}",
        phone.labels()
    );

    assert!(!phone.back(), "left to the system");
    phone.settle();
    assert_eq!(phone.tab_kinds(), ["home"]);
    assert!(home_shows(&phone, "Triangle"));
}

/// The phone with the program `id` installed from `component` as `name`, opened, and settled on
/// its page once it has ended.
fn ended_in_its_tab(component: &str, id: &str, name: &str) -> TestPhone {
    let mut phone = TestPhone::with_installed(&[(component, id, name)]);
    phone.start_opening(id);
    phone.settle_until("it ends in its tab", |phone| {
        phone.stage(id) == Some("ended")
    });
    phone.settle();
    phone
}

/// The one label on screen besides `said` that `which` picks, the detail beneath it: below it,
/// smaller and in another colour.
fn detail_beneath(phone: &TestPhone, said: &str, which: impl Fn(&str) -> bool) -> String {
    let detail = phone
        .labels()
        .into_iter()
        .find(|label| which(label))
        .unwrap_or_else(|| panic!("no detail: {:?}", phone.labels()));
    let (said_style, detail_style) = (
        phone.label_style(said).expect("the plain line"),
        phone.label_style(&detail).expect("the detail"),
    );
    assert!(
        phone.label_bounds(&detail).unwrap().top >= phone.label_bounds(said).unwrap().bottom,
        "beneath the plain line"
    );
    assert!(
        detail_style.font_size < said_style.font_size,
        "smaller: {detail_style:?} against {said_style:?}"
    );
    assert_ne!(detail_style.color, said_style.color, "in gray");
    detail
}

#[test]
fn a_program_built_for_another_version_says_so_plainly_over_the_engines_words() {
    let phone = ended_in_its_tab(waos_test_programs::INCOMPATIBLE, INCOMPATIBLE, "Old");
    let said = "Old was built for a different version of WaOS and cannot run in this one. Get an update of Old, or update WaOS.";
    assert!(phone.shows(said), "{:?}", phone.labels());
    let detail = detail_beneath(&phone, said, |label| {
        label.starts_with("component imports instance `wasdk:platform/system@1.0.0`")
    });
    assert!(
        detail.contains("not found in the linker"),
        "whole: {detail}"
    );
}

#[test]
fn a_program_that_failed_says_so_plainly_over_the_engines_words() {
    let phone = ended_in_its_tab(waos_test_programs::TRAP, TRAP, "Trap");
    let said = "Trap stopped because of a problem.";
    assert!(phone.shows(said), "{:?}", phone.labels());
    let detail = detail_beneath(&phone, said, |label| label.contains("unreachable"));
    // The backtrace names the file as the machine that built it does, `\` between names on Windows.
    assert!(
        detail
            .replace('\\', "/")
            .contains("/examples/trap/src/main.rs:"),
        "whole, its backtrace with it: {detail}"
    );
}
