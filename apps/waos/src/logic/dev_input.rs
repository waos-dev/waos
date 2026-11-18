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
use waos_dev_protocol::{
    Connection, Input, InputAnswer, InputDone, Refusal, ServerStream, WindowId,
};
use waos_package::ProgramId;

pub(crate) use mouse::DevMouse;
use mouse::DragPath;

use super::asking_now::AskingNow;
use super::dev_refusal;
use super::dev_server::BUILD;
use super::runs::{DevRun, Runs};

/// How long WaOS waits for the window's picture after an input: many frames of a program that
/// redraws what the input changed, debug builds among them.
pub(crate) const PICTURE_WAIT: Duration = Duration::from_secs(2);

/// Answers a connection's `Input`: `input` given to the window `window` of the program `id`, then
/// `InputDone` with whether the window was in sight and drew a new picture, or why it reached
/// none.
pub(crate) async fn answer(
    mut connection: Connection<ServerStream>,
    runs: &Runs,
    asking: &AskingNow,
    id: &ProgramId,
    window: Option<WindowId>,
    input: Input,
) {
    let answer = match give(runs, asking, id, window, input).await {
        Ok(given) => InputAnswer::Done(InputDone {
            in_sight: given.in_sight,
            new_picture: given.new_picture,
        }),
        Err(refusal) => {
            tracing::info!(%id, ?window, ?refusal, "an input reached no window");
            InputAnswer::Refused(refusal)
        }
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// What giving an input came to: whether the window was in sight when it was given, and whether
/// it then showed a picture the program drew after it.
struct Given {
    in_sight: bool,
    new_picture: bool,
}

/// Gives `input` to the window `window` (or the program's first open window) of the program `id`.
async fn give(
    runs: &Runs,
    asking: &AskingNow,
    id: &ProgramId,
    window: Option<WindowId>,
    input: Input,
) -> Result<Given, Refusal> {
    if matches!(input, Input::Unknown) {
        return Err(dev_refusal::unsupported(BUILD, "input"));
    }
    let run = runs
        .dev_run(id)
        .await
        .ok_or_else(|| dev_refusal::no_dev_run_now(id))?;
    if let Some(asked) = asking.of(id.as_str()) {
        return Err(dev_refusal::asking(id, &asked, "the input was not given"));
    }
    let (window, view) = run.window(window)?;
    tracing::info!(%id, %window, ?input, "an input from cargo waos");
    // The answer says how it stood when the input was given, not how it stands after the wait.
    let in_sight = view.in_sight();
    deliver(id, window, &run, &view, &input).await?;
    let new_picture = in_sight && new_picture_after(&view).await;
    Ok(Given {
        in_sight,
        new_picture,
    })
}

/// Gives `input` to `view`, one of `run`'s program's windows, over the time it takes.
async fn deliver(
    id: &ProgramId,
    window: WindowId,
    run: &DevRun,
    view: &ProgramView,
    input: &Input,
) -> Result<(), Refusal> {
    match *input {
        Input::Tap { x, y } => {
            within(id, window, view, x, y)?;
            run.mouse.tap(&run.program, view, x, y);
        }
        Input::Press { x, y, duration } => {
            within(id, window, view, x, y)?;
            run.mouse.press(&run.program, view, (x, y), duration).await;
        }
        Input::Drag {
            from,
            to,
            hold,
            steps,
            duration,
        } => {
            within(id, window, view, from.0, from.1)?;
            within(id, window, view, to.0, to.1)?;
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
            within(id, window, view, x, y)?;
            run.mouse.scroll(&run.program, view, (x, y), (dx, dy));
        }
        Input::Key(ref chord) => press(id, window, view, chord)?,
        Input::Text(ref text) => commit(id, window, view, text)?,
        Input::Unknown => return Err(dev_refusal::unsupported(BUILD, "input")),
    }
    Ok(())
}

/// Refuses a point outside `view`, which no mouse over the window is at.
fn within(
    id: &ProgramId,
    window: WindowId,
    view: &ProgramView,
    x: f64,
    y: f64,
) -> Result<(), Refusal> {
    let metrics = view.metrics();
    let (width, height) = (f64::from(metrics.width), f64::from(metrics.height));
    if (0.0..width).contains(&x) && (0.0..height).contains(&y) {
        return Ok(());
    }
    Err(dev_refusal::outside_window(
        id,
        window,
        metrics.width,
        metrics.height,
        x,
        y,
        metrics.device_pixel_ratio,
    ))
}

/// Presses the key or the chord `chord` names in `view`. The program's replies are not awaited:
/// a key it leaves goes no further.
fn press(id: &ProgramId, window: WindowId, view: &ProgramView, chord: &str) -> Result<(), Refusal> {
    let Some(events) = key_chords::key_events(chord, view.now()) else {
        return Err(dev_refusal::bad_input(
            id,
            window,
            &format!(
                "no key is named {chord:?}: name a letter, a digit, one of - = [ ] \\ ; ' ` , . /, or enter, escape, tab, space, backspace, delete, up, down, left, right, home, end, pageup, pagedown or f1 to f12, after any of cmd+, ctrl+, alt+ and shift+"
            ),
        ));
    };
    for event in events {
        drop(view.key(event));
    }
    Ok(())
}

/// Commits `text` into what `view` edits, as an input method commits its text.
fn commit(id: &ProgramId, window: WindowId, view: &ProgramView, text: &str) -> Result<(), Refusal> {
    let Some(editing) = view.editing_value() else {
        return Err(dev_refusal::bad_input(
            id,
            window,
            "nothing in it is being edited: tap a text field first, or send keys with `cargo waos key`",
        ));
    };
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
