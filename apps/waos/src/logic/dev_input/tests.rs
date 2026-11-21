//! Input from `waos` given to `examples/input` through the dev server, over an engine whose
//! windows draw: a tap and a scroll reach it as a mouse of the run's own, coming into the window
//! once; a press holds the button down for its time, and a drag moves with it held; text is
//! committed into what it edits, once it edits; a chord reaches it as its keys; each is done once
//! the window shows the picture after it, or after the wait when none comes; and an input to a
//! program no run runs, to a window it has not, at a point outside it, or naming no key, reaches
//! nothing. The program draws, so these run where the engine makes hardware buffers, on Apple's
//! systems.

use std::path::Path;
use std::time::{Duration, Instant};

use waos::Presence;
use waos_dev_protocol::{
    ClientStream, Connection, Input, InputAnswer, InputDone, Refusal, RefusalKind, Request,
    RunEvent, ScreenshotAnswer, WindowId,
};

use super::mouse::DEVICE;
use crate::logic::dev_server::tests::{TestServer, WITHIN, next, stdout_of};
use crate::logic::test_engine::drawing::{WINDOW_HEIGHT, WINDOW_WIDTH};

/// The id `examples/input` runs as.
pub(crate) const INPUT: &str = "dev.waos.examples.input";

/// `examples/input` running under `server`, its window showing its first picture; its run's
/// connection, and what came on it so far.
pub(crate) async fn run_input(server: &TestServer) -> (Connection<ClientStream>, Vec<RunEvent>) {
    let mut client = server
        .run(INPUT, Path::new(waos_test_programs::INPUT))
        .await;
    let mut frames = Vec::new();
    wait_for_line(&mut client, &mut frames, "window: open").await;
    wait_for_first_picture(server).await;
    (client, frames)
}

/// Waits until the program's window shows a picture.
async fn wait_for_first_picture(server: &TestServer) {
    let deadline = Instant::now() + WITHIN;
    let screenshot = Request::Screenshot {
        id: INPUT.parse().unwrap(),
        window: None,
    };
    loop {
        if let ScreenshotAnswer::Pictures(pictures) =
            server.ask::<ScreenshotAnswer>(&screenshot).await
            && pictures
                .first()
                .is_some_and(|picture| picture.png.is_some())
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the window never showed a picture"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Reads the run's connection until the program has written `line`.
async fn wait_for_line(
    client: &mut Connection<ClientStream>,
    frames: &mut Vec<RunEvent>,
    line: &str,
) {
    while !stdout_of(frames).lines().any(|written| written == line) {
        frames.push(next(client).await);
    }
}

/// An input to the window `window` of `examples/input`.
fn input(window: WindowId, input: Input) -> Request {
    Request::Input {
        id: INPUT.parse().unwrap(),
        window: Some(window),
        input,
    }
}

/// What WaOS answers an input, which every input here expects to be given.
async fn done(server: &TestServer, window: WindowId, given: Input) -> InputDone {
    match server.ask::<InputAnswer>(&input(window, given)).await {
        InputAnswer::Done(done) => done,
        other => panic!("the input given: {other:?}"),
    }
}

/// What WaOS refuses an input with.
async fn refused(server: &TestServer, window: WindowId, given: Input) -> Refusal {
    match server.ask::<InputAnswer>(&input(window, given)).await {
        InputAnswer::Refused(refusal) => refusal,
        other => panic!("the input refused: {other:?}"),
    }
}

/// What the program writes of the mouse's event `change` at `(x, y)`, with `buttons` held and a
/// scroll of `scroll`.
fn mouse_line(change: &str, signal: &str, (x, y): (f64, f64), buttons: u8, scroll: &str) -> String {
    format!(
        "pointer: PointerChange::{change} PointerDeviceKind::Mouse PointerSignalKind::{signal} device {DEVICE} at {x},{y} buttons {buttons} pressure 1 scroll {scroll}"
    )
}

#[tokio::test]
async fn a_tap_reaches_the_program_as_a_mouse_of_the_runs_own_done_once_its_next_picture_shows() {
    let server = TestServer::drawing("tap");
    let (mut client, mut frames) = run_input(&server).await;
    let tap = Input::Tap { x: 40.0, y: 30.0 };
    assert_eq!(
        done(&server, WindowId(1), tap.clone()).await,
        InputDone {
            in_sight: true,
            new_picture: true,
        }
    );
    for (change, buttons) in [("Add", 0), ("Hover", 0), ("Down", 1), ("Up", 0)] {
        let line = mouse_line(change, "None", (40.0, 30.0), buttons, "0,0");
        wait_for_line(&mut client, &mut frames, &line).await;
    }
    assert_eq!(
        done(&server, WindowId(1), tap).await,
        InputDone {
            in_sight: true,
            new_picture: true,
        }
    );
    let added = mouse_line("Add", "None", (40.0, 30.0), 0, "0,0");
    let pressed = mouse_line("Down", "None", (40.0, 30.0), 1, "0,0");
    while stdout_of(&frames)
        .lines()
        .filter(|line| *line == pressed)
        .count()
        < 2
    {
        frames.push(next(&mut client).await);
    }
    let said = stdout_of(&frames);
    assert_eq!(
        said.lines().filter(|line| *line == added).count(),
        1,
        "the mouse came into the window once: {said}"
    );
}

#[tokio::test]
async fn text_is_committed_into_what_the_window_edits_once_it_edits() {
    let server = TestServer::drawing("text");
    let (mut client, mut frames) = run_input(&server).await;
    let rejection = refused(&server, WindowId(1), Input::Text("milk".into())).await;
    assert_eq!(
        rejection.kind,
        RefusalKind::BadInput,
        "nothing edited before its first press"
    );
    done(&server, WindowId(1), Input::Tap { x: 1.0, y: 1.0 }).await;
    wait_for_line(&mut client, &mut frames, "text: editing").await;
    assert_eq!(
        done(&server, WindowId(1), Input::Text("milk".into())).await,
        InputDone {
            in_sight: true,
            new_picture: true,
        }
    );
    wait_for_line(
        &mut client,
        &mut frames,
        "text: \"milk\" selection Some(\"4..4\")",
    )
    .await;
    done(&server, WindowId(1), Input::Text(", eggs".into())).await;
    wait_for_line(
        &mut client,
        &mut frames,
        "text: \"milk, eggs\" selection Some(\"10..10\")",
    )
    .await;
}

#[tokio::test]
async fn a_chord_reaches_the_program_as_its_keys_done_without_a_picture_when_none_comes() {
    let server = TestServer::drawing("chord");
    let (mut client, mut frames) = run_input(&server).await;
    assert_eq!(
        done(&server, WindowId(1), Input::Key("cmd+s".into())).await,
        InputDone {
            in_sight: true,
            new_picture: false,
        },
        "the program draws nothing for a key"
    );
    for line in [
        "key: KeyKind::Down physical 0x700e3 logical 0x200000106 character None",
        "key: KeyKind::Down physical 0x70016 logical 0x73 character Some(\"s\")",
        "key: KeyKind::Up physical 0x70016 logical 0x73 character None",
        "key: KeyKind::Up physical 0x700e3 logical 0x200000106 character None",
    ] {
        wait_for_line(&mut client, &mut frames, line).await;
    }
    let rejection = refused(&server, WindowId(1), Input::Key("hyper+q".into())).await;
    assert_eq!(rejection.kind, RefusalKind::BadInput);
    assert!(rejection.message.contains("hyper+q"), "{rejection}");
}

#[tokio::test]
async fn a_scroll_reaches_the_program_where_the_mouse_came() {
    let server = TestServer::drawing("scroll");
    let (mut client, mut frames) = run_input(&server).await;
    let scroll = Input::Scroll {
        x: 10.0,
        y: 20.0,
        dx: 0.0,
        dy: 120.0,
    };
    assert_eq!(
        done(&server, WindowId(1), scroll).await,
        InputDone {
            in_sight: true,
            new_picture: true,
        }
    );
    for line in [
        mouse_line("Add", "None", (10.0, 20.0), 0, "0,0"),
        mouse_line("Hover", "None", (10.0, 20.0), 0, "0,0"),
        mouse_line("Hover", "Scroll", (10.0, 20.0), 0, "0,120"),
    ] {
        wait_for_line(&mut client, &mut frames, &line).await;
    }
}

#[tokio::test]
async fn a_press_holds_the_button_down_for_its_time_then_releases_it() {
    let server = TestServer::drawing("press");
    let (mut client, mut frames) = run_input(&server).await;
    let held = Duration::from_millis(300);
    let press = Input::Press {
        x: 40.0,
        y: 30.0,
        duration: held,
    };
    let started = Instant::now();
    done(&server, WindowId(1), press).await;
    assert!(started.elapsed() >= held, "answered before the release");
    for (change, buttons) in [("Add", 0), ("Hover", 0), ("Down", 1), ("Up", 0)] {
        let line = mouse_line(change, "None", (40.0, 30.0), buttons, "0,0");
        wait_for_line(&mut client, &mut frames, &line).await;
    }
}

#[tokio::test]
async fn a_drag_moves_with_the_button_held_in_its_steps_and_releases_at_its_end() {
    let server = TestServer::drawing("drag");
    let (mut client, mut frames) = run_input(&server).await;
    let drag = Input::Drag {
        from: (10.0, 10.0),
        to: (50.0, 30.0),
        hold: Duration::from_millis(100),
        steps: 2,
        duration: Duration::from_millis(200),
    };
    let started = Instant::now();
    done(&server, WindowId(1), drag).await;
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "answered before the drag's time"
    );
    let lines = [
        mouse_line("Add", "None", (10.0, 10.0), 0, "0,0"),
        mouse_line("Hover", "None", (10.0, 10.0), 0, "0,0"),
        mouse_line("Down", "None", (10.0, 10.0), 1, "0,0"),
        mouse_line("Move", "None", (30.0, 20.0), 1, "0,0"),
        mouse_line("Move", "None", (50.0, 30.0), 1, "0,0"),
        mouse_line("Up", "None", (50.0, 30.0), 0, "0,0"),
    ];
    wait_for_line(&mut client, &mut frames, &lines[5]).await;
    let said = stdout_of(&frames);
    let heard: Vec<&str> = said
        .lines()
        .filter(|line| line.starts_with("pointer:"))
        .collect();
    assert_eq!(heard, lines, "in this order, and nothing else");
}

#[tokio::test]
async fn an_input_to_a_window_or_a_point_not_there_reaches_nothing() {
    let server = TestServer::drawing("not-there");
    let (_client, _frames) = run_input(&server).await;
    let tap = Input::Tap { x: 1.0, y: 1.0 };
    for window in [WindowId(0), WindowId(2)] {
        let rejection = refused(&server, window, tap.clone()).await;
        assert_eq!(rejection.kind, RefusalKind::NoWindow { window });
    }
    let outside = Input::Tap {
        x: f64::from(WINDOW_WIDTH),
        y: 0.0,
    };
    let rejection = refused(&server, WindowId(1), outside).await;
    assert_eq!(
        rejection.kind,
        RefusalKind::OutsideWindow {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            x: f64::from(WINDOW_WIDTH),
            y: 0.0,
            scale: 2.0,
        }
    );
    let drag_out = Input::Drag {
        from: (1.0, 1.0),
        to: (1.0, f64::from(WINDOW_HEIGHT)),
        hold: Duration::ZERO,
        steps: 1,
        duration: Duration::ZERO,
    };
    assert_eq!(
        refused(&server, WindowId(1), drag_out).await.kind,
        RefusalKind::OutsideWindow {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            x: 1.0,
            y: f64::from(WINDOW_HEIGHT),
            scale: 2.0,
        },
        "a drag ends in the window"
    );
}

#[tokio::test]
async fn an_input_to_a_window_out_of_sight_says_it_was_out_of_sight() {
    let server = TestServer::drawing("out-of-sight");
    let (_client, _frames) = run_input(&server).await;
    let view = server.programs().await[0].views().remove(0);
    view.presence_changed(Presence {
        on_screen: false,
        occluded: false,
        focused: false,
    });
    assert!(!view.in_sight());
    assert_eq!(
        done(&server, WindowId(1), Input::Tap { x: 1.0, y: 1.0 }).await,
        InputDone {
            in_sight: false,
            new_picture: false,
        },
        "answered at once, with why no picture came"
    );
}

#[tokio::test]
async fn an_input_to_a_program_no_run_runs_reaches_nothing() {
    let server = TestServer::start("no-run");
    let rejection = refused(&server, WindowId(1), Input::Tap { x: 1.0, y: 1.0 }).await;
    assert_eq!(rejection.kind, RefusalKind::NoDevRunNow);
}
