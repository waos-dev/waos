//! What `cargo waos` says when a screenshot or an input reached no window of the program, or its
//! settings were not reached, and why.

use waos_dev_protocol::Unreached;
use waos_package::ProgramId;

use crate::asked;

/// What `unreached` means for the program `id`, whose window `window` the input was for, in the
/// words `cargo waos` prints.
pub fn said(unreached: &Unreached, id: &ProgramId, window: u32) -> String {
    match unreached {
        Unreached::NotRunByCargoWaos => format!(
            "{id} is not running from `cargo waos run`: run it with `cargo waos run` in another shell first; a program opened from WaOS's library is neither read nor driven"
        ),
        Unreached::NoWindow { windows: 0, .. } => {
            format!("{id} has no window open")
        }
        Unreached::NoWindow { window, windows } => {
            format!("{id} has no window {window}: it has {windows}, numbered from 1")
        }
        Unreached::OutsideWindow {
            width,
            height,
            x,
            y,
            scale,
        } => format!(
            "the point {x},{y} is outside window {window} of {id}, which is {width}x{height} pixels ({}x{} points at scale {scale}); input points are pixels, as a screenshot's",
            f64::from(*width) / scale,
            f64::from(*height) / scale
        ),
        Unreached::NotEditing => format!(
            "nothing in window {window} of {id} is being edited: tap a text field first, or send keys with `cargo waos key`"
        ),
        Unreached::NeverRunByCargoWaos => format!(
            "no `cargo waos run` on this machine has run {id}: run it once with `cargo waos run`; a program only installed from WaOS's library keeps its settings to its settings page"
        ),
        Unreached::Asking(asking) => format!(
            "WaOS is asking over {id}'s windows, so the input was not given: {}",
            asked::said(asking, id.as_str())
        ),
        Unreached::InNoWindow { window } => format!(
            "window {window} of {id} is a view in no window, as a phone's deck shows a tab: its size is the screen's, which no tool sets"
        ),
        Unreached::UnknownKey(chord) => format!(
            "no key is named {chord:?}: name a letter, a digit, one of - = [ ] \\ ; ' ` , . /, or enter, escape, tab, space, backspace, delete, up, down, left, right, home, end, pageup, pagedown or f1 to f12, after any of cmd+, ctrl+, alt+ and shift+"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes() -> ProgramId {
        "dev.example.notes".parse().unwrap()
    }

    #[test]
    fn each_refusal_says_what_to_do_next() {
        let said = |unreached| said(&unreached, &notes(), 2);
        assert!(said(Unreached::NotRunByCargoWaos).contains("cargo waos run"));
        assert_eq!(
            said(Unreached::NoWindow {
                window: 2,
                windows: 1
            }),
            "dev.example.notes has no window 2: it has 1, numbered from 1"
        );
        assert_eq!(
            said(Unreached::NoWindow {
                window: 1,
                windows: 0
            }),
            "dev.example.notes has no window open"
        );
        assert_eq!(
            said(Unreached::OutsideWindow {
                width: 800,
                height: 600,
                x: 900.0,
                y: 700.0,
                scale: 1.0,
            }),
            "the point 900,700 is outside window 2 of dev.example.notes, which is 800x600 pixels (800x600 points at scale 1); input points are pixels, as a screenshot's"
        );
        assert_eq!(
            said(Unreached::OutsideWindow {
                width: 780,
                height: 1688,
                x: 800.0,
                y: 700.0,
                scale: 2.0,
            }),
            "the point 800,700 is outside window 2 of dev.example.notes, which is 780x1688 pixels (390x844 points at scale 2); input points are pixels, as a screenshot's"
        );
        assert!(said(Unreached::NotEditing).contains("tap a text field first"));
        assert!(
            said(Unreached::InNoWindow { window: 2 })
                .starts_with("window 2 of dev.example.notes is a view in no window")
        );
        assert!(
            said(Unreached::UnknownKey("hyper+q".into()))
                .starts_with("no key is named \"hyper+q\"")
        );
    }
}
