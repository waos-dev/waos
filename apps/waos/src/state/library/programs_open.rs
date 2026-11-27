//! Where the programs the library opens open, and the windows the system restores to them: on a
//! desktop in windows each program asks for; where nothing else can show a program, as `waos
//! phone`'s window, in the deck's tabs; and on Android where the user chose, each a task of its own
//! by default, or inside WaOS in the deck's tabs, the choice kept in WaOS's preferences and holding
//! for programs opened after it. A program running opens where it runs.
//!
//! A program's first window that Android brings back from Recents after the process died starts
//! its program again with that window as its first, as a view the platform gave it.
//!
//! The invariant: what the user chose is kept in WaOS's preferences before it holds
//! (`logic::preferences`), so a WaOS started again opens programs as the user last saw it do.

use waft::ui::ViewRef;
use waft::{Context, Entity};
use waos::RunningProgram;
use waos_package::ProgramId;

use super::{Library, WantsToBeSeen};
use crate::logic::installed_runs::Opening;
use crate::logic::program_faces::program_face;
use crate::logic::program_places::ProgramPlace;
use crate::state::ViewWindows;

/// Where the programs opened from the library open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProgramsOpen {
    /// In windows each program asks for, as on a desktop.
    InWindows,
    /// In the deck's tabs, where nothing else can show a program.
    InTabs,
    /// Where the user chose, as WaOS's preferences say: each in windows of its own, a task each on
    /// Android, or inside WaOS in the deck's tabs.
    AsChosen,
}

impl Library {
    /// Whether a program not running opens in a tab.
    pub(super) fn programs_open_in_tabs(&self) -> bool {
        match self.programs_open {
            ProgramsOpen::InWindows => false,
            ProgramsOpen::InTabs => true,
            ProgramsOpen::AsChosen => self.programs_inside_waos() == Some(true),
        }
    }

    /// Where the program `id` opens: where it runs, or where programs open.
    pub(super) fn place_to_open(&self, id: &ProgramId) -> ProgramPlace {
        if self.is_running(id) {
            return self.runs.places.place_of(id);
        }
        if self.programs_open_in_tabs() {
            ProgramPlace::Tab
        } else {
            ProgramPlace::Windows
        }
    }

    /// Whether programs opened from now on show inside WaOS, in the deck's tabs; none where WaOS
    /// offers no choice.
    pub(crate) fn programs_inside_waos(&self) -> Option<bool> {
        if self.programs_open != ProgramsOpen::AsChosen {
            return None;
        }
        let preferences = self.preferences.as_ref()?;
        Some(preferences.chosen().programs_inside_waos)
    }

    /// Programs opened from now on show inside WaOS, or each as a task of its own, as `inside`
    /// says, kept in WaOS's preferences; where they cannot be kept, nothing changes, and that is
    /// the library's problem. Nothing changes where WaOS offers no choice.
    pub(crate) fn show_programs_inside_waos(&mut self, inside: bool, cx: &mut Context<Library>) {
        if self.programs_open == ProgramsOpen::AsChosen {
            self.choose_preferences(|chosen| chosen.programs_inside_waos = inside, cx);
            self.match_shortcuts(cx);
        }
    }

    /// Starts the installed program `id` again with `view`, its first window, which the platform
    /// brought back to a WaOS started afresh, as Android does a card from Recents: the window waits
    /// in `windows` for the program's first request, however long that takes. Where the program
    /// runs already, the window closes and the running program comes forward. One no longer
    /// installed, as a tap on a removed program's home-screen icon asks for before WaOS disabled
    /// it, closes the window, brings WaOS's own forward to say so, and disables the icon.
    pub(crate) fn open_restored(
        &mut self,
        id: &ProgramId,
        view: ViewRef,
        windows: &Entity<ViewWindows>,
        cx: &mut Context<Library>,
    ) {
        if self.store.program(id).is_none() {
            close_window_of(&view);
            // Brought forward while the window is in sight, which Android requires of an app.
            cx.emit(WantsToBeSeen);
            self.match_shortcuts(cx);
            return self.failed(
                format!("{id} is not installed, or its files cannot be read"),
                cx,
            );
        }
        if self.is_running(id) {
            close_window_of(&view);
            let name = id.as_str().to_owned();
            windows.update(cx, |windows, _| {
                windows.bring_forward(|running: &RunningProgram| running.name() == name);
            });
            return;
        }
        let face = program_face(&self.store, id.as_str());
        windows.update(cx, |windows, _| {
            windows.adopt_window(view, id.as_str(), &face)
        });
        self.start_in_its_window(id, windows.clone(), cx);
    }

    /// Starts the installed program `id`, whose first window is open in `windows`, waiting for its
    /// first request; where it cannot start, the window closes, and that is the library's problem.
    /// A program that starts goes first among WaOS's recent programs.
    pub(crate) fn start_in_its_window(
        &mut self,
        id: &ProgramId,
        windows: Entity<ViewWindows>,
        cx: &mut Context<Library>,
    ) {
        let Some(component) = self.store.program(id) else {
            windows.update(cx, |windows, _| windows.prepared_unused(id.as_str()));
            return self.failed(
                format!("{id} is not installed, or its files cannot be read"),
                cx,
            );
        };
        self.add_recent(id, cx);
        let (runs, opening) = (self.runs.clone(), id.clone());
        let name = id.as_str().to_owned();
        self.work_then(
            async move {
                runs.open(
                    &opening,
                    component,
                    ProgramPlace::Windows,
                    Opening::ByTheUser,
                )
                .await
            },
            move |library, started, cx| match started {
                Ok(opened) => windows.update(cx, |windows, cx| {
                    windows.prepared_started(opened.program().clone(), cx);
                }),
                Err(error) => {
                    windows.update(cx, |windows, _| windows.prepared_unused(&name));
                    library.failed(format!("{error:#}"), cx);
                }
            },
            cx,
        );
    }
}

/// Closes the window `view` is in, where it is in one.
fn close_window_of(view: &ViewRef) {
    if let Some(window) = view.window() {
        window.close();
    }
}
