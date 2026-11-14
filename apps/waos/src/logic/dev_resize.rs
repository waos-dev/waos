//! A size `cargo waos resize` asks for one window of a program a run of `cargo waos run` runs, so
//! a test sees the program's layout at a phone's width or a narrow window's (the design's decision
//! 60): through the engine's view, which sizes its window through the shell as the program's own
//! `set-frame` does (`ProgramView::resize_window`), so each placement answers as it answers the
//! program. A window in WaOS's process and a program's own app on macOS take the size, or what
//! their window system clamps it to; a phone's deck shows a tab in no window, and is answered
//! `InNoWindow`. The connection is answered with the size the view reports after it, once the
//! window shows a picture the program drew at it, or once WaOS has waited [`PICTURE_WAIT`] for
//! each; a window its system left as it was reports nothing, and is answered with its size after
//! the wait.
//!
//! The invariant: the size goes the way a program's request goes, through the shell's window,
//! never to a window of the app's own reached around the engine.

use waos::bindings::wasdk::platform::windows::Size;
use waos_dev_protocol::{Connection, FromWaOS, ServerStream, Unreached};
use waos_package::ProgramId;

use super::dev_input::{PICTURE_WAIT, new_picture_after};
use super::runs::Runs;

/// Answers a connection's `Resize`: the window numbered `window` of the program `id` sized so
/// its view is `size` logical points, then `Resized` with the size it took, or `Unreached` with
/// why it reached none.
pub(crate) async fn answer(
    mut connection: Connection<ServerStream>,
    runs: &Runs,
    id: &ProgramId,
    window: u32,
    size: Size,
) {
    let answer = match resize(runs, id, window, size).await {
        Ok(resized) => resized,
        Err(unreached) => {
            tracing::info!(%id, window, ?unreached, "a resize reached no window");
            FromWaOS::Unreached(unreached)
        }
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// Sizes the window numbered `window` of the program `id` so its view is `size`; answers the size
/// the view then reports, and whether the program drew a picture at it.
async fn resize(
    runs: &Runs,
    id: &ProgramId,
    window: u32,
    size: Size,
) -> Result<FromWaOS, Unreached> {
    let run = runs.dev_run(id).await.ok_or(Unreached::NotRunByCargoWaos)?;
    let view = run.window(window)?;
    tracing::info!(%id, window, size.width, size.height, "a resize from cargo waos");
    let reported = view
        .resize_window(size)
        .ok_or(Unreached::InNoWindow { window })?;
    let metrics = tokio::time::timeout(PICTURE_WAIT, reported)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| view.metrics());
    Ok(FromWaOS::Resized {
        width: metrics.width,
        height: metrics.height,
        scale: metrics.device_pixel_ratio,
        new_picture: new_picture_after(&view).await,
    })
}

/// Run with `examples/input`, which draws where the engine makes hardware buffers, on Apple's
/// systems.
#[cfg(all(test, target_vendor = "apple"))]
mod tests {
    use waos_dev_protocol::ToWaOS;

    use super::*;
    use crate::logic::dev_input::tests::{INPUT, run_input};
    use crate::logic::dev_server::tests::TestServer;
    use crate::logic::test_engine::drawing::{WINDOW_HEIGHT, WINDOW_WIDTH};

    fn resize(window: u32, width: f64, height: f64) -> ToWaOS {
        ToWaOS::Resize {
            id: INPUT.parse().unwrap(),
            window,
            width,
            height,
        }
    }

    #[tokio::test]
    async fn a_window_takes_the_size_and_answers_it_once_the_program_drew_at_it() {
        let server = TestServer::drawing("resize");
        let (_client, _frames) = run_input(&server).await;
        assert_eq!(
            server.ask(&resize(1, 100.0, 80.0)).await,
            FromWaOS::Resized {
                width: 200,
                height: 160,
                scale: 2.0,
                new_picture: true,
            }
        );
        let screenshot = ToWaOS::Screenshot {
            id: INPUT.parse().unwrap(),
        };
        let FromWaOS::Screenshots(pictures) = server.ask(&screenshot).await else {
            panic!("a screenshot");
        };
        assert_eq!((pictures[0].width, pictures[0].height), (200, 160));
    }

    #[tokio::test]
    async fn a_view_in_no_window_keeps_its_size_and_says_so() {
        let server = TestServer::drawing_in_no_window("resize-deck");
        let (_client, _frames) = run_input(&server).await;
        assert_eq!(
            server.ask(&resize(1, 100.0, 80.0)).await,
            FromWaOS::Unreached(Unreached::InNoWindow { window: 1 })
        );
        let screenshot = ToWaOS::Screenshot {
            id: INPUT.parse().unwrap(),
        };
        let FromWaOS::Screenshots(pictures) = server.ask(&screenshot).await else {
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
            server.ask(&resize(1, 100.0, 80.0)).await,
            FromWaOS::Unreached(Unreached::NotRunByCargoWaos)
        );
        let (_client, _frames) = run_input(&server).await;
        for window in [0, 2] {
            assert_eq!(
                server.ask(&resize(window, 100.0, 80.0)).await,
                FromWaOS::Unreached(Unreached::NoWindow { window, windows: 1 })
            );
        }
    }
}
