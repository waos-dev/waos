//! The refusals the dev server gives, each in words a person can act on, beside a kind an agent
//! acts on: the program no run of `cargo waos run` runs, the window that is not there or is a tab
//! in no window, the point outside it, the input that cannot be given, and the component that does
//! not load.

use waos_dev_protocol::{Asked, Refusal, RefusalKind, WindowId};
use waos_package::ProgramId;

/// No run of `cargo waos run` runs `id` now.
pub(crate) fn no_dev_run_now(id: &ProgramId) -> Refusal {
    Refusal::new(
        RefusalKind::NoDevRunNow,
        format!(
            "{id} is not running from `cargo waos run`: run it with `cargo waos run` in another shell first; a program opened from WaOS's library is neither read nor driven"
        ),
    )
}

/// No run of `cargo waos run` on this machine has run `id`, so it has no settings to read.
pub(crate) fn never_dev_run(id: &ProgramId) -> Refusal {
    Refusal::new(
        RefusalKind::NeverDevRun,
        format!(
            "no `cargo waos run` on this machine has run {id}: run it once with `cargo waos run`; a program only installed from WaOS's library keeps its settings to its settings page"
        ),
    )
}

/// `id` has no window `window`: it has `windows` open, or none.
pub(crate) fn no_window(id: &ProgramId, window: WindowId, windows: usize) -> Refusal {
    let message = if windows == 0 {
        format!("{id} has no window open")
    } else {
        format!(
            "{id} has no window {window}: it has {windows} open, each with the id it was given as it opened"
        )
    };
    Refusal::new(RefusalKind::NoWindow { window }, message)
}

/// The point `x`, `y` is outside window `window` of `id`, which is `width` by `height` pixels at
/// `scale`.
pub(crate) fn outside_window(
    id: &ProgramId,
    window: WindowId,
    width: u32,
    height: u32,
    x: f64,
    y: f64,
    scale: f64,
) -> Refusal {
    Refusal::new(
        RefusalKind::OutsideWindow {
            width,
            height,
            x,
            y,
            scale,
        },
        format!(
            "the point {x},{y} is outside window {window} of {id}, which is {width}x{height} pixels ({}x{} points at scale {scale}); input points are pixels, as a screenshot's",
            f64::from(width) / scale,
            f64::from(height) / scale
        ),
    )
}

/// Window `window` of `id` is a view in no window, as a phone's deck shows a tab.
pub(crate) fn in_no_window(id: &ProgramId, window: WindowId) -> Refusal {
    Refusal::new(
        RefusalKind::InNoWindow { window },
        format!(
            "window {window} of {id} is a view in no window, as a phone's deck shows a tab: its size is the screen's, which no tool sets"
        ),
    )
}

/// WaOS asks the program's user something over `id`'s windows, so `why` an input was not given.
pub(crate) fn asking(id: &ProgramId, asked: &Asked, why: &str) -> Refusal {
    Refusal::new(
        RefusalKind::Asking(asked.clone()),
        format!(
            "WaOS is asking over {id}'s windows, so {why}: answer what it asks first (what it asks is in the refusal's kind)"
        ),
    )
}

/// Window `window` of `id` cannot take the input: `why`.
pub(crate) fn bad_input(id: &ProgramId, window: WindowId, why: &str) -> Refusal {
    Refusal::new(
        RefusalKind::BadInput,
        format!("window {window} of {id} cannot take the input: {why}"),
    )
}

/// `id`'s component is not one, or does not load: `error`.
pub(crate) fn cannot_load(id: &ProgramId, error: &anyhow::Error) -> Refusal {
    Refusal::new(
        RefusalKind::CannotLoad,
        format!("{id} could not be loaded or run: {error:#}"),
    )
}

/// `id`'s files cannot be opened on this machine, which `Open` is for: a network connection names
/// no path of ours.
pub(crate) fn local_only(id: &ProgramId) -> Refusal {
    Refusal::new(
        RefusalKind::LocalOnly,
        format!(
            "opening {id}'s files names paths on this machine, and this connection is not on this machine"
        ),
    )
}
