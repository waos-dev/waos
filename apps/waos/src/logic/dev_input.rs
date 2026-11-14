//! An input `cargo waos tap`, `press`, `drag`, `type`, `key` or `scroll` sends, given to a window
//! of a program a run of `cargo waos run` runs, as its user would give it (the design's decision
//! 60): through the engine's view, as the shell reports its user's input, so the program cannot
//! tell the two apart. A tap, a press, a drag and a scroll are a mouse's, a mouse of the run's own
//! (`mouse`), a press and a drag taking the time they say before the answer; a key or a chord is a
//! keyboard's key events (`key_chords`); text is committed into what the window edits, as an input
//! method commits its text (`text_commit`). The connection is answered once the window shows a
//! picture the program drew after reading the input, or once WaOS has waited [`PICTURE_WAIT`] for
//! one, and at once for a window out of sight, whose frames wait until it is in sight. While WaOS
//! asks the program's user something over its windows (`asking_now`), the input is not given, as
//! a person's click goes to the question first, and the answer says what is asked.
//!
//! The invariant: nothing here goes past the engine's view. What the shell does with what the
//! program leaves, a key offered to the menu bar, is not done for these: a key the program leaves
//! goes no further.

mod key_chords;
mod mouse;
mod text_commit;

use std::time::Duration;

use waos::ProgramView;
use waos_dev_protocol::{Connection, FromWaOS, Input, ServerStream, Unreached};
use waos_package::ProgramId;

pub(crate) use mouse::DevMouse;
use mouse::DragPath;

use super::asking_now::AskingNow;
use super::runs::{DevRun, Runs};

/// How long WaOS waits for the window's picture after an input: many frames of a program that
/// redraws what the input changed, debug builds among them.
pub(crate) const PICTURE_WAIT: Duration = Duration::from_secs(2);

/// Answers a connection's `Input`: `input` given to the window numbered `window` of the program
/// `id`, then `InputDone`, or `Unreached` with why it reached none.
pub(crate) async fn answer(
    mut connection: Connection<ServerStream>,
    runs: &Runs,
    asking: &AskingNow,
    id: &ProgramId,
    window: u32,
    input: Input,
) {
    let answer = match give(runs, asking, id, window, input).await {
        Ok(new_picture) => FromWaOS::InputDone { new_picture },
        Err(unreached) => {
            tracing::info!(%id, window, ?unreached, "an input reached no window");
            FromWaOS::Unreached(unreached)
        }
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// Gives `input` to the window numbered `window` of the program `id`; answers whether the window
/// then showed a picture drawn after it.
async fn give(
    runs: &Runs,
    asking: &AskingNow,
    id: &ProgramId,
    window: u32,
    input: Input,
) -> Result<bool, Unreached> {
    let run = runs.dev_run(id).await.ok_or(Unreached::NotRunByCargoWaos)?;
    if let Some(asked) = asking.of(id.as_str()) {
        return Err(Unreached::Asking(asked));
    }
    let view = run.window(window)?;
    tracing::info!(%id, window, ?input, "an input from cargo waos");
    deliver(&run, &view, &input).await?;
    Ok(new_picture_after(&view).await)
}

/// Gives `input` to `view`, one of `run`'s program's windows, over the time it takes.
async fn deliver(run: &DevRun, view: &ProgramView, input: &Input) -> Result<(), Unreached> {
    match *input {
        Input::Tap { x, y } => {
            within(view, x, y)?;
            run.mouse.tap(&run.program, view, x, y);
        }
        Input::Press { x, y, duration } => {
            within(view, x, y)?;
            run.mouse.press(&run.program, view, (x, y), duration).await;
        }
        Input::Drag {
            from,
            to,
            hold,
            steps,
            duration,
        } => {
            within(view, from.0, from.1)?;
            within(view, to.0, to.1)?;
            let path = DragPath {
                from,
                to,
                hold,
                steps,
                duration,
            };
            run.mouse.drag(&run.program, view, &path).await;
        }
        Input::Scroll { x, y, dx, dy } => {
            within(view, x, y)?;
            run.mouse.scroll(&run.program, view, (x, y), (dx, dy));
        }
        Input::Key(ref chord) => press(view, chord)?,
        Input::Text(ref text) => commit(view, text)?,
    }
    Ok(())
}

/// Refuses a point outside `view`, which no mouse over the window is at.
fn within(view: &ProgramView, x: f64, y: f64) -> Result<(), Unreached> {
    let metrics = view.metrics();
    let (width, height) = (f64::from(metrics.width), f64::from(metrics.height));
    if (0.0..width).contains(&x) && (0.0..height).contains(&y) {
        return Ok(());
    }
    Err(Unreached::OutsideWindow {
        width: metrics.width,
        height: metrics.height,
    })
}

/// Presses the key or the chord `chord` names in `view`. The program's replies are not awaited:
/// a key it leaves goes no further.
fn press(view: &ProgramView, chord: &str) -> Result<(), Unreached> {
    let events = key_chords::key_events(chord, view.now())
        .ok_or_else(|| Unreached::UnknownKey(chord.to_owned()))?;
    for event in events {
        drop(view.key(event));
    }
    Ok(())
}

/// Commits `text` into what `view` edits, as an input method commits its text.
fn commit(view: &ProgramView, text: &str) -> Result<(), Unreached> {
    let editing = view.editing_value().ok_or(Unreached::NotEditing)?;
    view.text_input_editing_value(text_commit::committed(&editing, text));
    Ok(())
}

/// Whether `view` shows a picture the program drew after reading every event queued on it now,
/// within [`PICTURE_WAIT`]; no at once for a view out of sight, which is served no frame.
pub(crate) async fn new_picture_after(view: &ProgramView) -> bool {
    let shown = view.picture_after_events();
    if !view.in_sight() {
        return false;
    }
    tokio::time::timeout(PICTURE_WAIT, shown)
        .await
        .unwrap_or(false)
}

/// Run with `examples/input`, which draws where the engine makes hardware buffers, on Apple's
/// systems.
#[cfg(all(test, target_vendor = "apple"))]
pub(crate) mod tests;
