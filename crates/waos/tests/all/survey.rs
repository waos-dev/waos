//! `examples/survey` under the test shell: it asks for a view when it is given none, with no title
//! bar of macOS's own, sees it arrive with its request's id and what the request attached, keeps
//! its window between the size limits it sets, hears a change of geometry, moves, maximizes and
//! minimizes its window from the band it treats as its title bar, as the system's setting for a
//! double-click says, reads its user's time zone and hears it change, answers a request to end,
//! and ends when its last view is gone. Each test reads what its own survey printed.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::views::{PointerButtons, PointerChange};
use waos::bindings::wasdk::platform::windows::{MacosTitleBar, Size, TitleBarAction};
use waos::{AppInstance, HeldModifiers, ProgramEnd, ProgramView, WaOS};
use waos_headless::HeadlessView;

use crate::test_shell::{RecordedOutput, TestShell, config, metrics, mouse, printing_to, within};

/// A point in the band survey treats as its title bar, 28 logical points tall, in the view's
/// physical pixels at the test shell's 2x.
const ON_TITLE_BAR: (f64, f64) = (200.0, 20.0);

/// A point below that band.
const BELOW_TITLE_BAR: (f64, f64) = (200.0, 400.0);

/// A millisecond on the program's clock, in nanoseconds.
const MS: u64 = 1_000_000;

/// Survey running under a test shell of its own, the view it asked for, and what it prints.
struct Survey {
    shell: TestShell,
    _engine: WaOS,
    running: AppInstance,
    shown: Arc<HeadlessView>,
    view: ProgramView,
    output: Arc<RecordedOutput>,
}

/// Survey, started, once its view has arrived.
fn survey() -> Survey {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let survey = engine
        .load(
            Path::new(waos_test_programs::SURVEY),
            &waos_test_programs::code_of(waos_test_programs::SURVEY),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&survey, printing_to(&output)).unwrap();
    let shown = within(shell.headless.view(0)).expect("survey asks for a view");
    let view = shown.view().expect("survey's view");
    Survey {
        shell,
        _engine: engine,
        running,
        shown,
        view,
        output,
    }
}

#[test]
fn asks_for_a_view_sees_it_arrive_and_ends_when_it_is_taken_away() {
    let Survey {
        running,
        shown,
        view,
        output,
        _engine: engine,
        ..
    } = survey();
    assert_eq!(view.request_id(), 1);
    let window = shown.window().expect("in a window");
    assert_eq!(window.config().title, "Survey");
    output.wait_for("asked for a view: request 1");
    output.wait_for(
        "view 1: 1600x1000 at 2x, on screen Some(1), in a window: true, for request 1 with Some(\"opened at start\")",
    );

    output.wait_for("view 1: its window keeps between 320x240 and 1920x1200");
    let (smallest, largest) = window.size_limits();
    let points = |size: Size| (size.width, size.height);
    assert_eq!(smallest.map(points), Some((320.0, 240.0)));
    assert_eq!(largest.map(points), Some((1920.0, 1200.0)));

    view.metrics_changed(metrics(800, 600));
    output.wait_for("view 1: now 800x600");

    engine.set_time_zone(Some("America/Sao_Paulo".into()));
    output.wait_for("system: time zone now Some(\"America/Sao_Paulo\")");

    running.remove_view(view.id());
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    let printed = output.lines();
    assert!(
        printed.iter().any(|line| line == "view 1 removed"),
        "{printed:#?}"
    );
    assert!(
        printed[0].starts_with("system: OperatingSystem::")
            && printed[0].ends_with(", Brightness::Light, languages en-US"),
        "{printed:#?}"
    );
    assert!(
        printed[1].starts_with("system: time zone Some(\"Asia/Tokyo\"), ")
            && printed[1].ends_with(" UTC+9h"),
        "{printed:#?}"
    );
    assert!(
        printed.contains(
            &"screen 1: \"Headless\", 1600x1000 at 2x, 60000 mHz, frame (0, 0) 800x500, visible frame (0, 0) 800x500"
                .to_owned()
        ),
        "{printed:#?}"
    );
}

#[test]
fn moves_maximizes_minimizes_and_takes_full_screen_its_window_from_its_title_bar() {
    let Survey {
        shell,
        running,
        shown,
        view,
        output,
        ..
    } = survey();
    let window = shown.window().expect("in a window");
    assert!(
        matches!(window.config().macos_title_bar, MacosTitleBar::Hidden(28.0)),
        "survey asks for no title bar of macOS's own, its buttons in a band 28 points tall"
    );
    output.wait_for("view 1: its window keeps between 320x240 and 1920x1200");
    view.title_bar_double_click_action_changed(Some(TitleBarAction::Maximize));

    press(&view, ON_TITLE_BAR, 0);
    output.wait_for("view 1: pressed on its title bar: moving its window");
    // Survey says so before it asks its window to move.
    shell
        .changes
        .wait_until("a press on the band moves the window", || {
            window.drags() == 1
        });

    press(&view, ON_TITLE_BAR, 200 * MS);
    output.wait_for("view 1: double-clicked its title bar: Some(TitleBarAction::Maximize)");
    shell
        .changes
        .wait_until("survey maximizes its window", || window.is_maximized());
    assert_eq!(
        window.drags(),
        1,
        "the second press of a double-click moves nothing"
    );

    view.window_maximized_changed(true);
    view.metrics_changed(metrics(3200, 2000));
    output.wait_for("view 1: its window is maximized");

    double_click(&view, 1000 * MS);
    shell
        .changes
        .wait_until("survey returns its window to its size from before", || {
            !window.is_maximized()
        });
    view.window_maximized_changed(false);
    view.metrics_changed(metrics(1600, 1000));
    output.wait_for("view 1: its window is no longer maximized");

    secondary_press(&view, ON_TITLE_BAR, 1500 * MS);
    output.wait_for("view 1: secondary press on its title bar: full screen true");
    shell
        .changes
        .wait_until("survey asks for its window full screen", || {
            window.is_fullscreen()
        });
    view.window_fullscreen_changed(true);
    output.wait_for("view 1: its window is full screen");
    // The user leaves it by the system's own means: survey hears, having asked for nothing.
    view.window_fullscreen_changed(false);
    output.wait_for("view 1: its window is no longer full screen");
    assert_eq!(
        output.count("view 1: its window is full screen"),
        1,
        "a change is told once"
    );

    view.title_bar_double_click_action_changed(Some(TitleBarAction::Minimize));
    double_click(&view, 2000 * MS);
    shell
        .changes
        .wait_until("survey minimizes its window", || window.is_minimized());

    view.title_bar_double_click_action_changed(Some(TitleBarAction::Nothing));
    double_click(&view, 3000 * MS);
    output.wait_for("view 1: double-clicked its title bar: Some(TitleBarAction::Nothing)");
    view.title_bar_double_click_action_changed(None);
    double_click(&view, 4000 * MS);
    output.wait_for("view 1: double-clicked its title bar: None");

    press(&view, BELOW_TITLE_BAR, 5000 * MS);
    press(&view, ON_TITLE_BAR, 6000 * MS);
    shell
        .changes
        .wait_until("survey moves its window once more", || window.drags() == 6);
    assert_eq!(
        output.count("view 1: pressed on its title bar: moving its window"),
        6,
        "a press below the band is not on the title bar"
    );
    assert!(!window.is_maximized());
    assert!(window.is_minimized());

    running.remove_view(view.id());
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

/// A mouse's press and release at `at`, in `view`'s physical pixels, going down at `time` on the
/// program's clock.
fn press(view: &ProgramView, at: (f64, f64), time: u64) {
    let held = HeldModifiers::default();
    view.pointer(
        mouse(PointerChange::Down, PointerButtons::PRIMARY, at, time),
        &held,
    );
    view.pointer(
        mouse(
            PointerChange::Up,
            PointerButtons::empty(),
            at,
            time + 50 * MS,
        ),
        &held,
    );
}

/// A press of the mouse's secondary button at `at`, and its release.
fn secondary_press(view: &ProgramView, at: (f64, f64), time: u64) {
    let held = HeldModifiers::default();
    view.pointer(
        mouse(PointerChange::Down, PointerButtons::SECONDARY, at, time),
        &held,
    );
    view.pointer(
        mouse(
            PointerChange::Up,
            PointerButtons::empty(),
            at,
            time + 50 * MS,
        ),
        &held,
    );
}

/// Two presses on survey's title bar, 100 ms apart, from `time` on the program's clock.
fn double_click(view: &ProgramView, time: u64) {
    press(view, ON_TITLE_BAR, time);
    press(view, ON_TITLE_BAR, time + 100 * MS);
}

#[test]
fn closes_its_window_when_asked_and_ends_once_the_window_has_closed() {
    let Survey {
        running,
        view,
        output,
        ..
    } = survey();
    assert_eq!(
        view.request_id(),
        1,
        "the first request takes the reserved screen"
    );
    output.wait_for("view 1: 1600x1000 at 2x, on screen Some(1), in a window: true, for request 1 with Some(\"opened at start\")");

    view.close_requested();
    output.wait_for("view 1: asked to close");
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

#[test]
fn answers_a_request_to_end() {
    let Survey {
        running, output, ..
    } = survey();
    output.wait_for("view 1: 1600x1000 at 2x, on screen Some(1), in a window: true, for request 1 with Some(\"opened at start\")");

    // Survey opens its requests to end before it says anything, so it hears this one.
    assert_eq!(within(running.request_exit()), AppExitResponse::Exit);
    output.wait_for("program: asked whether it may end: yes");
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}
