//! Where the programs the library opens open, and the windows the system restores to them: on a
//! desktop in windows each program asks for; where nothing else can show a program, as `waos
//! phone`'s window, in the deck's tabs; and on Android where the user chose, each a task of its
//! own by default, or inside WaOS in the deck's tabs (decisions 1 and 2 of
//! `docs/proposals/2026-11-12-phone-programs-and-library.md`), the choice kept in WaOS's
//! preferences and holding for programs opened after it. A program running opens where it runs.
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
use waos_program_window::placed_view;

use super::Library;
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
        }
    }

    /// Starts the installed program `id` again with `view`, its first window, which the platform
    /// restored to a WaOS started afresh, as its first: opened in `windows` for it, and shown
    /// there once the program runs. Where it cannot start, or runs already with windows of its
    /// own, the window closes, the running program's coming forward; one that cannot start is the
    /// library's problem.
    pub(crate) fn open_restored(
        &mut self,
        id: &ProgramId,
        view: ViewRef,
        windows: &Entity<ViewWindows>,
        cx: &mut Context<Library>,
    ) {
        let view_id = view.id();
        let face = program_face(&self.store, id.as_str());
        let opened = match windows.update(cx, |windows, cx| windows.open_given(view, &face, cx)) {
            Ok(opened) => opened,
            Err(why) => return self.failed(format!("{id} could not be opened: {why}"), cx),
        };
        let Some(component) = self.store.program(id) else {
            windows.update(cx, |windows, _| windows.given_not_started(view_id));
            return self.failed(
                format!("{id} is not installed, or its files cannot be read"),
                cx,
            );
        };
        let (runs, opening) = (self.runs.clone(), id.clone());
        let placed = placed_view(opened, None);
        let windows = windows.clone();
        self.work_then(
            async move {
                runs.open(&opening, component, vec![placed], ProgramPlace::Windows)
                    .await
            },
            move |library, started, cx| match started {
                Ok((program, views)) => match views.into_iter().next() {
                    Some(view) => windows.update(cx, |windows, cx| {
                        windows.given_started(view_id, program, view, cx);
                    }),
                    None => windows.update(cx, |windows, _| {
                        windows.given_not_started(view_id);
                        windows.bring_forward(|running: &RunningProgram| *running == program);
                    }),
                },
                Err(error) => {
                    windows.update(cx, |windows, _| windows.given_not_started(view_id));
                    library.failed(format!("{error:#}"), cx);
                }
            },
            cx,
        );
    }
}
