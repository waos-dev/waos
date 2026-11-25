//! What a program's window carries for the system to hand back: the window's initial data names
//! the program it shows and whether it is the program's first window or an extra one, so a window
//! the system restores to a WaOS started afresh, as Android does a task from Recents, says what
//! goes in it. A program's first window starts the program again; an extra one has no program
//! state behind it (decision 10 of `docs/proposals/2026-11-12-phone-programs-and-library.md`).
//!
//! The invariant: what [`WindowRestoration::initial_data`] writes, [`WindowRestoration::of`] reads
//! back, and nothing else reads as a program's window: a view whose initial data is anything else
//! is not a program's.

/// Whether a window is a program's first, which shows the program, or one it asked for besides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRole {
    First,
    Extra,
}

/// A program's window, as its initial data names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowRestoration {
    /// The name the program runs under: an installed program's id.
    pub program: String,
    pub role: WindowRole,
}

/// What every program's window's initial data starts with, so no other view's is taken for one.
const PREFIX: &str = "waos-program";

impl WindowRestoration {
    /// The window's initial data.
    pub fn initial_data(&self) -> String {
        let role = match self.role {
            WindowRole::First => "first",
            WindowRole::Extra => "extra",
        };
        format!("{PREFIX}:{role}:{}", self.program)
    }

    /// The program's window `initial_data` names; none where it names none.
    pub fn of(initial_data: &str) -> Option<WindowRestoration> {
        let rest = initial_data.strip_prefix(PREFIX)?.strip_prefix(':')?;
        let (role, program) = rest.split_once(':')?;
        let role = match role {
            "first" => WindowRole::First,
            "extra" => WindowRole::Extra,
            _ => return None,
        };
        (!program.is_empty()).then(|| WindowRestoration {
            program: program.to_owned(),
            role,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_initial_data_names_it_again() {
        for role in [WindowRole::First, WindowRole::Extra] {
            let window = WindowRestoration {
                program: "dev.example.gallery".to_owned(),
                role,
            };
            assert_eq!(WindowRestoration::of(&window.initial_data()), Some(window));
        }
    }

    #[test]
    fn other_initial_data_names_no_programs_window() {
        for other in [
            "",
            "inspector",
            "waos-program",
            "waos-program:first:",
            "waos-program:last:dev.example.gallery",
        ] {
            assert_eq!(WindowRestoration::of(other), None, "{other:?}");
        }
    }
}
