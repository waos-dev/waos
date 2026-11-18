//! The refusals the dev server gives, each in words a person can act on, beside a kind an agent
//! acts on: the program no run of `cargo waos run` runs, the window that is not there or is a tab
//! in no window, the point outside it, the input that cannot be given, the component that does not
//! load, and the opening a connection not on this machine asks for, which only one here may have.

use waos_dev_protocol::{Asked, Locality, OpenAnswer, Refusal, RefusalKind, WindowId};
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

/// A request or one of its inner variants came from a newer client that `build` does not know.
pub(crate) fn unsupported(build: &str, what: &str) -> Refusal {
    Refusal::new(
        RefusalKind::Failed,
        format!("{build} does not know this {what}: update WaOS from https://waos.app"),
    )
}

/// `Open`'s answer before its paths are looked at, on a connection of `locality`: `LocalOnly` for
/// `id` on one that is not on this machine, whose paths a network peer does not mean; none on
/// this machine, where the files are handed over.
pub(crate) fn elsewhere(locality: Locality, id: &ProgramId) -> Option<OpenAnswer> {
    (locality != Locality::ThisMachine).then(|| OpenAnswer::Refused(local_only(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes() -> ProgramId {
        "dev.example.notes".parse().unwrap()
    }

    #[test]
    fn each_refusal_says_what_to_do_next() {
        let no_run = no_dev_run_now(&notes());
        assert_eq!(no_run.kind, RefusalKind::NoDevRunNow);
        assert!(no_run.message.contains("cargo waos run"));
        let no_window = no_window(&notes(), WindowId(2), 1);
        assert_eq!(
            no_window.kind,
            RefusalKind::NoWindow {
                window: WindowId(2)
            }
        );
        assert!(
            no_window.message.contains("no window 2"),
            "{}",
            no_window.message
        );
        assert!(never_dev_run(&notes()).message.contains("has run"));
        assert_eq!(
            outside_window(&notes(), WindowId(1), 800, 600, 900.0, 700.0, 1.0).kind,
            RefusalKind::OutsideWindow {
                width: 800,
                height: 600,
                x: 900.0,
                y: 700.0,
                scale: 1.0,
            }
        );
        assert_eq!(
            bad_input(&notes(), WindowId(1), "no key").kind,
            RefusalKind::BadInput
        );
    }

    #[test]
    fn opening_is_local_only() {
        assert_eq!(elsewhere(Locality::ThisMachine, &notes()), None);
        let refused = elsewhere(Locality::Network, &notes()).expect("a refusal");
        let OpenAnswer::Refused(refusal) = refused else {
            panic!("a refusal");
        };
        assert_eq!(refusal.kind, RefusalKind::LocalOnly);
        assert!(refusal.message.contains("not on this machine"));
    }
}
