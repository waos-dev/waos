//! `examples/survey` under the test shell: it asks for a view when it is given none, with no title
//! bar of macOS's own, sees it arrive with its request's id and what the request attached, keeps
//! its window between the size limits it sets, hears a change of geometry, moves, maximizes and
//! minimizes its window from the band it treats as its title bar, as the system's setting for a
//! double-click says, reads its user's time zone and hears it change, answers a request to end,
//! and ends when its last view is gone.
//!
//! Every test here reads what survey printed in the log, and two surveys at once would print
//! alike, so each takes survey's turn (`shell::turns`).

use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::views::{PointerButtons, PointerChange};
use waos::bindings::wasdk::platform::windows::{MacosTitleBar, TitleBarAction};
use waos::{HeldModifiers, ProgramEnd, ProgramEnvironment, ProgramView};

use crate::shell::{self, Output, WITHIN, metrics, mouse, output, wait_until};

/// A point in the band survey treats as its title bar, 28 logical points tall, in the view's
/// physical pixels at the test shell's 2x.
const ON_TITLE_BAR: (f64, f64) = (200.0, 20.0);

/// A point below that band.
const BELOW_TITLE_BAR: (f64, f64) = (200.0, 400.0);

/// A millisecond on the program's clock, in nanoseconds.
const MS: u64 = 1_000_000;

#[test]
fn asks_for_a_view_sees_it_arrive_and_ends_when_it_is_taken_away() {
    let _survey = shell::turns::take("survey");
    let output = output();
    let mark = output.mark();
    let (engine, shell, arrivals) = shell::engine();
    let survey = shell::load(&engine, waos_test_programs::SURVEY);
    let (running, given) = engine
        .run(&survey, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    assert!(given.is_empty());

    let view = arrivals
        .recv_timeout(WITHIN)
        .expect("survey asks for a view");
    assert_eq!(view.request_id(), Some(1));
    assert_eq!(shell.requested_titles(), ["Survey"]);
    output.wait_for(mark, "survey", |line| line == "asked for a view: request 1");
    output.wait_for(mark, "survey", |line| {
        line == "view 1: 1600x1000 at 2x, on screen Some(1), in a window: true, for request Some(1) with Some(\"opened at start\")"
    });

    output.wait_for(mark, "survey", |line| {
        line == "view 1: its window keeps between 320x240 and 1920x1200"
    });
    let window = &shell.placed_windows()[0];
    assert_eq!(window.min_size(), Some((320.0, 240.0)));
    assert_eq!(window.max_size(), Some((1920.0, 1200.0)));

    view.metrics_changed(metrics(800, 600));
    output.wait_for(mark, "survey", |line| line == "view 1: now 800x600");

    engine.set_time_zone(Some("America/Sao_Paulo".into()));
    output.wait_for(mark, "survey", |line| {
        line == "system: time zone now Some(\"America/Sao_Paulo\")"
    });

    running.remove_view(view.id());
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    let printed = output.lines_of(mark, "survey");
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
            &"screen 1: \"Test Display\", 1600x1000 at 2x, 60000 mHz, frame (0, 0) 800x500, visible frame (64, 24) 736x476"
                .to_owned()
        ),
        "{printed:#?}"
    );
}

#[test]
fn moves_maximizes_and_minimizes_its_window_from_the_band_it_treats_as_its_title_bar() {
    let _survey = shell::turns::take("survey");
    let output = output();
    let mark = output.mark();
    let (engine, shell, arrivals) = shell::engine();
    let survey = shell::load(&engine, waos_test_programs::SURVEY);
    let (running, _given) = engine
        .run(&survey, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    let view = arrivals
        .recv_timeout(WITHIN)
        .expect("survey asks for a view");
    let asked = &shell.requested_windows()[0];
    assert!(
        matches!(asked.macos_title_bar, MacosTitleBar::Hidden(28.0)),
        "survey asks for no title bar of macOS's own, its buttons in a band 28 points tall"
    );
    output.wait_for(mark, "survey", |line| {
        line == "view 1: its window keeps between 320x240 and 1920x1200"
    });
    let window = &shell.placed_windows()[0];
    let says = |expected: &str| output.wait_for(mark, "survey", |line| line == expected);

    press(&view, ON_TITLE_BAR, 0);
    says("view 1: pressed on its title bar: moving its window");
    assert_eq!(window.drags(), 1, "a press on the band moves the window");

    press(&view, ON_TITLE_BAR, 200 * MS);
    says("view 1: double-clicked its title bar: Some(TitleBarAction::Maximize)");
    wait_until("survey maximizes its window", || {
        window.maximized_asked() == [true]
    });
    assert_eq!(
        window.drags(),
        1,
        "the second press of a double-click moves nothing"
    );

    view.window_maximized_changed(true);
    view.metrics_changed(metrics(3200, 2000));
    says("view 1: its window is maximized");

    double_click(&view, 1000 * MS);
    wait_until("survey returns its window to its size from before", || {
        window.maximized_asked() == [true, false]
    });
    view.window_maximized_changed(false);
    view.metrics_changed(metrics(1600, 1000));
    says("view 1: its window is no longer maximized");

    view.title_bar_double_click_action_changed(Some(TitleBarAction::Minimize));
    double_click(&view, 2000 * MS);
    wait_until("survey minimizes its window", || {
        window.minimized_asked() == [true]
    });

    view.title_bar_double_click_action_changed(Some(TitleBarAction::Nothing));
    double_click(&view, 3000 * MS);
    says("view 1: double-clicked its title bar: Some(TitleBarAction::Nothing)");
    view.title_bar_double_click_action_changed(None);
    double_click(&view, 4000 * MS);
    says("view 1: double-clicked its title bar: None");

    press(&view, BELOW_TITLE_BAR, 5000 * MS);
    press(&view, ON_TITLE_BAR, 6000 * MS);
    wait_until("survey moves its window once more", || window.drags() == 6);
    assert_eq!(
        moves(output, mark),
        6,
        "a press below the band is not on the title bar"
    );
    assert_eq!(window.maximized_asked(), [true, false]);
    assert_eq!(window.minimized_asked(), [true]);

    running.remove_view(view.id());
    let end = block_on(running.ended());
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

/// Two presses on survey's title bar, 100 ms apart, from `time` on the program's clock.
fn double_click(view: &ProgramView, time: u64) {
    press(view, ON_TITLE_BAR, time);
    press(view, ON_TITLE_BAR, time + 100 * MS);
}

/// How many moves of its window survey said it started since `mark`.
fn moves(output: &Output, mark: usize) -> usize {
    output
        .lines_of(mark, "survey")
        .iter()
        .filter(|line| *line == "view 1: pressed on its title bar: moving its window")
        .count()
}

#[test]
fn closes_its_window_when_asked_and_ends_once_the_shell_takes_the_view_away() {
    let _survey = shell::turns::take("survey");
    let output = output();
    let mark = output.mark();
    let (engine, shell, _arrivals) = shell::engine();
    let survey = shell::load(&engine, waos_test_programs::SURVEY);
    let (running, given) = engine
        .run(
            &survey,
            ProgramEnvironment::default(),
            vec![shell.placed_view(None)],
        )
        .unwrap();
    let view = &given[0];
    assert_eq!(view.request_id(), None);
    output.wait_for(mark, "survey", |line| line.starts_with("view 1: 1600x1000"));
    let window = &shell.placed_windows()[0];
    shell::window_holds(window, &running, view.id());

    view.close_requested();
    output.wait_for(mark, "survey", |line| line == "view 1: asked to close");
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert_eq!(window.closes(), 1);
}

#[test]
fn answers_a_request_to_end() {
    let _survey = shell::turns::take("survey");
    let output = output();
    let mark = output.mark();
    let (engine, shell, _arrivals) = shell::engine();
    let survey = shell::load(&engine, waos_test_programs::SURVEY);
    let (running, _given) = engine
        .run(
            &survey,
            ProgramEnvironment::default(),
            vec![shell.placed_view(None)],
        )
        .unwrap();
    output.wait_for(mark, "survey", |line| line.starts_with("view 1: 1600x1000"));

    // A request before the program listens is answered exit by nobody: asked again until the
    // program has heard one, which it says.
    let deadline = Instant::now() + WITHIN;
    loop {
        assert_eq!(block_on(running.request_exit()), AppExitResponse::Exit);
        let heard = output
            .lines_of(mark, "survey")
            .iter()
            .any(|line| line == "program: asked whether it may end: yes");
        if heard {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "survey never heard a request to end"
        );
        thread::sleep(Duration::from_millis(10));
    }
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
}
