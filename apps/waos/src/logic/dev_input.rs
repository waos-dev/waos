//! An input `waos tap`, `press`, `drag`, `type`, `key` or `scroll` sends, given to a window
//! of a program a run of `waos run` runs, as its user would give it (the design's decision
//! 60): through the engine's view, as the shell reports its user's input, so the program cannot
//! tell the two apart: the program's own mouse (`RunningProgram::mouse`), a US keyboard and an
//! input method's text (`Keyboard`), a press and a drag taking the time they say before the
//! answer. This is the protocol's side alone: the window the input goes to, what is refused, and
//! the answer. The connection is answered once the input is handed over, saying whether the window
//! was in sight; what the program draws in answer is its own to draw, and whoever expects a
//! picture waits for it, as a test does. While WaOS
//! asks the program's user something over its windows (`asking_now`), the input is not given, as
//! a person's click goes to the question first, and the answer says what is asked.

use waos::{DragPath, Keyboard, ProgramView};
use waos_dev_protocol::{
    Connection, Input, InputAnswer, InputDone, Refusal, ServerStream, WindowId,
};
use waos_package::ProgramId;

use super::asking_now::AskingNow;
use super::dev_refusal;
use super::dev_server::BUILD;
use super::runs::{DevRun, Runs};

/// Answers a connection's `Input`: `input` given to the window `window` of the program `id`, then
/// `InputDone` with whether the window was in sight, or why it reached none.
pub(crate) async fn answer(
    mut connection: Connection<ServerStream>,
    runs: &Runs,
    asking: &AskingNow,
    id: &ProgramId,
    window: Option<WindowId>,
    input: Input,
) {
    let answer = match give(runs, asking, id, window, input).await {
        Ok(in_sight) => InputAnswer::Done(InputDone { in_sight }),
        Err(refusal) => {
            tracing::info!(%id, ?window, ?refusal, "an input reached no window");
            InputAnswer::Refused(refusal)
        }
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// Gives `input` to the window `window` (or the program's first open window) of the program `id`;
/// answers whether the window was in sight as it was given.
async fn give(
    runs: &Runs,
    asking: &AskingNow,
    id: &ProgramId,
    window: Option<WindowId>,
    input: Input,
) -> Result<bool, Refusal> {
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
    tracing::info!(%id, %window, ?input, "an input from waos");
    // The answer says how it stood when the input was given, not after a press's time.
    let in_sight = view.in_sight();
    deliver(id, window, &run, &view, &input).await?;
    Ok(in_sight)
}

/// Gives `input` to `view`, one of `run`'s program's windows, over the time it takes.
async fn deliver(
    id: &ProgramId,
    window: WindowId,
    run: &DevRun,
    view: &ProgramView,
    input: &Input,
) -> Result<(), Refusal> {
    let mouse = run.program.mouse();
    match *input {
        Input::Tap { x, y } => {
            within(id, window, view, x, y)?;
            mouse.tap(view, (x, y));
        }
        Input::Press { x, y, duration } => {
            within(id, window, view, x, y)?;
            mouse.press(view, (x, y), duration).await;
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
            mouse.drag(view, &path).await;
        }
        Input::Scroll { x, y, dx, dy } => {
            within(id, window, view, x, y)?;
            mouse.scroll(view, (x, y), (dx, dy));
        }
        Input::Key(ref chord) => Keyboard::press(view, chord).map_err(|_| {
            dev_refusal::bad_input(
                id,
                window,
                &format!(
                    "no key is named {chord:?}: name a letter, a digit, one of - = [ ] \\ ; ' ` , . /, or enter, escape, tab, space, backspace, delete, up, down, left, right, home, end, pageup, pagedown or f1 to f12, after any of cmd+, ctrl+, alt+ and shift+"
                ),
            )
        })?,
        Input::Text(ref text) => Keyboard::insert_text(view, text).map_err(|_| {
            dev_refusal::bad_input(
                id,
                window,
                "nothing in it is being edited: tap a text field first, or send keys with `waos key`",
            )
        })?,
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

/// Run with `examples/input`, which draws where the engine makes hardware buffers, on Apple's
/// systems.
#[cfg(all(test, any(target_vendor = "apple", target_os = "linux")))]
pub(crate) mod tests;
