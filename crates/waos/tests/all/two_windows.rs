//! `examples/two-windows`, a Waft app with two windows over `wasdk-adapter-waft`, under the test
//! shell, both on its one screen: both windows arrive titled `"first"` and `"second"` and draw
//! pictures; while the second window is hidden, reporting its view off screen, the first keeps
//! presenting frames and the second presents none; shown again, the second presents again; and
//! asking to close a window quits the program, its `main` returning.
//!
//! Hardware buffers are made on Apple's systems, where this test runs.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::{Presence, ProgramEnd, ProgramView, WaOS};
use waos_headless::HeadlessView;

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

/// The titles two-windows asks its windows to have, in order.
const TITLES: [&str; 2] = ["first", "second"];

const OFF_SCREEN: Presence = Presence {
    on_screen: false,
    occluded: false,
    focused: false,
};
const ON_SCREEN: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: false,
};

#[test]
fn a_hidden_window_gets_no_frames_while_the_shown_one_keeps_drawing_and_draws_again_once_shown() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::TWO_WINDOWS))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    let headless_views: Vec<Arc<HeadlessView>> = (0..TITLES.len())
        .map(|index| within(shell.headless.view(index)).expect("two-windows asks for its windows"))
        .collect();
    let titles: Vec<String> = headless_views
        .iter()
        .map(|view| view.window().expect("in a window").config().title.clone())
        .collect();
    assert_eq!(titles, TITLES);
    let views: Vec<ProgramView> = headless_views
        .iter()
        .map(|view| view.view().expect("two-windows' view"))
        .collect();
    let (shown, hidden) = (&headless_views[0], &headless_views[1]);

    wait_until("both windows present a picture", || {
        shown.pictures_shown() >= 1 && hidden.pictures_shown() >= 1
    });
    let first_picture = views[0].picture().unwrap().expect("first window picture");
    let second_picture = views[1].picture().unwrap().expect("second window picture");
    assert!(
        is_not_blank(&first_picture),
        "first window drew its spinner"
    );
    assert!(
        is_not_blank(&second_picture),
        "second window drew its spinner"
    );

    views[1].presence_changed(OFF_SCREEN);
    // Let any frame already queued or in flight for the hidden view before `OFF_SCREEN` finish on
    // the shared event loop and GPU queue before noting the baseline counts.
    let settling = shown.pictures_shown();
    wait_until("any frame already in flight finishes", || {
        shown.pictures_shown() >= settling + 2
    });
    let hidden_presents = hidden.pictures_shown();
    let shown_presents = shown.pictures_shown();
    wait_until(
        "the shown window keeps drawing while the other is hidden",
        || shown.pictures_shown() >= shown_presents + 3,
    );
    assert_eq!(
        hidden.pictures_shown(),
        hidden_presents,
        "the hidden window presents nothing while the shown one keeps drawing"
    );

    views[1].presence_changed(ON_SCREEN);
    wait_until("shown again, the second window presents", || {
        hidden.pictures_shown() > hidden_presents
    });

    views[0].close_requested();
    let end = within(running.ended());
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "asking to close a window quits two-windows, not {end:?}; it printed {:#?}",
        output.lines()
    );
}

/// Whether `picture` holds more than one pixel colour.
fn is_not_blank(picture: &PixelBuffer) -> bool {
    let first = &picture.bytes[..4];
    (0..picture.height as usize).any(|y| {
        let row = &picture.bytes[y * picture.row_bytes as usize..];
        (0..picture.width as usize).any(|x| &row[x * 4..x * 4 + 4] != first)
    })
}
