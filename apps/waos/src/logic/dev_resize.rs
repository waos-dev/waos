//! A size `waos resize` asks for one window of a program a run of `waos run` runs, by
//! the id the run gave it, so
//! a test sees the program's layout at a phone's width or a narrow window's (the design's decision
//! 60): through the engine's view, which sizes its window through the shell as the program's own
//! `set-frame` does (`ProgramView::resize_window`), so each placement answers as it answers the
//! program. A window in WaOS's process and a program's own app on macOS take the size, or what
//! their window system clamps it to; a phone's deck shows a tab in no window, and is answered
//! `InNoWindow`. The connection is answered with the size the view reports after it; a window its
//! system left as it was reports nothing, and is answered with its size once WaOS has waited
//! [`REPORT_WAIT`] for a report.
//!
//! The invariant: the size goes the way a program's request goes, through the shell's window,
//! never to a window of the app's own reached around the engine.

use std::time::Duration;

use waos::bindings::wasdk::platform::windows::Size;
use waos_dev_protocol::{Connection, Refusal, ResizeAnswer, Resized, ServerStream, WindowId};
use waos_package::ProgramId;

use super::dev_refusal;
use super::runs::Runs;

/// How long WaOS waits for the view to report the size it took: a window its system leaves as it
/// was reports nothing.
const REPORT_WAIT: Duration = Duration::from_secs(2);

/// Answers a connection's `Resize`: the window `window` of the program `id` sized so its view is
/// `size` logical points, then `Resized` with the size it took, or why it reached none.
pub(crate) async fn answer(
    mut connection: Connection<ServerStream>,
    runs: &Runs,
    id: &ProgramId,
    window: Option<WindowId>,
    size: Size,
) {
    let answer = match resize(runs, id, window, size).await {
        Ok(resized) => ResizeAnswer::Done(resized),
        Err(refusal) => {
            tracing::info!(%id, ?window, ?refusal, "a resize reached no window");
            ResizeAnswer::Refused(refusal)
        }
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// Sizes the window `window` (or the program's first open window) of the program `id` so its
/// view is `size`; answers the size the view then reports.
async fn resize(
    runs: &Runs,
    id: &ProgramId,
    window: Option<WindowId>,
    size: Size,
) -> Result<Resized, Refusal> {
    let run = runs
        .dev_run(id)
        .await
        .ok_or_else(|| dev_refusal::no_dev_run_now(id))?;
    let (window, view) = run.window(window)?;
    tracing::info!(%id, %window, size.width, size.height, "a resize from waos");
    let reported = view
        .resize_window(size)
        .ok_or_else(|| dev_refusal::in_no_window(id, window))?;
    let metrics = tokio::time::timeout(REPORT_WAIT, reported)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| view.metrics());
    Ok(Resized {
        width: metrics.width,
        height: metrics.height,
        scale: metrics.device_pixel_ratio,
    })
}

/// Run with `examples/input`, which draws where the engine makes hardware buffers, on Apple's
/// systems.
#[cfg(all(test, any(target_vendor = "apple", target_os = "linux")))]
mod tests {
    use waos_dev_protocol::{
        Refusal, RefusalKind, Request, ResizeAnswer, Resized, ScreenshotAnswer, WindowId,
    };

    use crate::logic::dev_input::tests::{INPUT, run_input};
    use crate::logic::dev_server::tests::TestServer;
    use crate::logic::test_engine::drawing::{WINDOW_HEIGHT, WINDOW_WIDTH};

    fn resize(window: WindowId, width: f64, height: f64) -> Request {
        Request::Resize {
            id: INPUT.parse().unwrap(),
            window: Some(window),
            width,
            height,
        }
    }

    /// What WaOS answers a resize, which the tests here expect to size the window.
    async fn done(server: &TestServer, window: WindowId, width: f64, height: f64) -> Resized {
        match server
            .ask::<ResizeAnswer>(&resize(window, width, height))
            .await
        {
            ResizeAnswer::Done(resized) => resized,
            other => panic!("the window sized: {other:?}"),
        }
    }

    /// What WaOS refuses a resize with.
    async fn refused(server: &TestServer, window: WindowId, width: f64, height: f64) -> Refusal {
        match server
            .ask::<ResizeAnswer>(&resize(window, width, height))
            .await
        {
            ResizeAnswer::Refused(refusal) => refusal,
            other => panic!("the resize refused: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_window_takes_the_size_and_answers_it() {
        let server = TestServer::drawing("resize");
        let (_client, _frames) = run_input(&server).await;
        assert_eq!(
            done(&server, WindowId(1), 100.0, 80.0).await,
            Resized {
                width: 200,
                height: 160,
                scale: 2.0,
            }
        );
        let screenshot = Request::Screenshot {
            id: INPUT.parse().unwrap(),
            window: None,
        };
        let ScreenshotAnswer::Pictures(pictures) =
            server.ask::<ScreenshotAnswer>(&screenshot).await
        else {
            panic!("a screenshot");
        };
        assert_eq!((pictures[0].width, pictures[0].height), (200, 160));
    }

    #[tokio::test]
    async fn a_view_in_no_window_keeps_its_size_and_says_so() {
        let server = TestServer::drawing_in_no_window("resize-deck");
        let (_client, _frames) = run_input(&server).await;
        assert_eq!(
            refused(&server, WindowId(1), 100.0, 80.0).await.kind,
            RefusalKind::InNoWindow {
                window: WindowId(1)
            }
        );
        let screenshot = Request::Screenshot {
            id: INPUT.parse().unwrap(),
            window: None,
        };
        let ScreenshotAnswer::Pictures(pictures) =
            server.ask::<ScreenshotAnswer>(&screenshot).await
        else {
            panic!("a screenshot");
        };
        assert_eq!(
            (pictures[0].width, pictures[0].height),
            (WINDOW_WIDTH, WINDOW_HEIGHT)
        );
    }

    #[tokio::test]
    async fn a_resize_of_a_window_not_there_or_of_a_program_no_run_runs_reaches_nothing() {
        let server = TestServer::drawing("resize-not-there");
        assert_eq!(
            refused(&server, WindowId(1), 100.0, 80.0).await.kind,
            RefusalKind::NoDevRunNow
        );
        let (_client, _frames) = run_input(&server).await;
        for window in [WindowId(0), WindowId(2)] {
            assert_eq!(
                refused(&server, window, 100.0, 80.0).await.kind,
                RefusalKind::NoWindow { window }
            );
        }
    }
}
