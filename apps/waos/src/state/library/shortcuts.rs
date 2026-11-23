//! The installed programs' shortcuts, where the system keeps them (`logic::program_shortcuts`), on
//! Android: a program installed for the first time is offered a place on the home screen, as Play
//! offers an installed app's icon, and so is any installed program from its held icon; each program
//! opened goes first among the recent programs on WaOS's icon's menu; and what the system keeps is
//! made to match what is installed, and where programs open, as WaOS starts, after every change of
//! what is installed, and whenever the user chooses where programs open.
//!
//! The invariant: a shortcut is made from what is installed as the library last read it, and its
//! icon is decoded and shaped on the library's runtime, never on the main thread, since a package's
//! icon is a large picture.

use std::rc::Rc;

use waft::{App, Context};
use waos_package::ProgramId;

use super::Library;
use crate::logic::program_shortcuts::{
    KeptShortcut, ProgramShortcut, ShortcutOpens, SystemShortcuts, changes_for, shortcut_icon,
};

impl Library {
    /// From now on the installed programs' shortcuts are kept in `system`, and what it keeps now
    /// is made to match what is installed.
    pub(crate) fn keep_shortcuts(
        &mut self,
        system: Rc<dyn SystemShortcuts>,
        cx: &mut Context<Library>,
    ) {
        self.shortcuts = Some(system);
        self.match_shortcuts(cx);
    }

    /// Whether the installed program `id` can be put on the home screen now: where the launcher
    /// pins shortcuts on request.
    pub(crate) fn adds_to_home_screen(&self, id: &ProgramId) -> bool {
        self.installed_program(id).is_some()
            && self.shortcuts.as_ref().is_some_and(|system| system.pins())
    }

    /// Asks the launcher to put the installed program `id` on the home screen, which asks the
    /// user; a request the launcher did not take is the library's problem.
    pub(crate) fn add_to_home_screen(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        self.request_pin(id, PinAsked::ByTheUser, cx);
    }

    /// Offers the program `id`, installed just now for the first time, a place on the home
    /// screen, unless its shortcut is there already, as for a program removed and installed again,
    /// whose greyed-out icon the install enables; a request the launcher did not take is only said
    /// in the log, the install having succeeded.
    pub(super) fn offer_home_screen(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        let Some(system) = self.shortcuts.clone() else {
            return;
        };
        if !self.adds_to_home_screen(id) {
            return;
        }
        let kept = system.kept();
        let (library, id) = (cx.weak_entity(), id.clone());
        // The task runs to its end whether or not its handle is kept.
        let _offering = cx.spawn(async move |cx| {
            let pinned = match kept.await {
                Ok(kept) => kept
                    .iter()
                    .any(|shortcut| shortcut.pinned && shortcut.id == id.as_str()),
                Err(error) => {
                    tracing::warn!(%error, "the shortcuts the system keeps cannot be read");
                    false
                }
            };
            if pinned {
                tracing::info!(%id, "the program is on the home screen already");
                return;
            }
            cx.update(|app: &mut App| {
                if let Some(library) = library.upgrade() {
                    library.update(app, |library, cx| {
                        library.request_pin(&id, PinAsked::AtInstall, cx);
                    });
                }
            });
        });
    }

    /// Puts the program `id` first among the recent programs on WaOS's icon's menu, where the
    /// system has one; nothing for a program not installed.
    pub(super) fn add_recent(&self, id: &ProgramId, cx: &mut Context<Library>) {
        if self.shortcuts.is_none() {
            return;
        }
        self.with_shortcuts(
            vec![id.clone()],
            |library, shortcuts, _cx| {
                if let (Some(system), Some(shortcut)) = (&library.shortcuts, shortcuts.first()) {
                    system.add_recent(shortcut);
                }
            },
            cx,
        );
    }

    /// Makes what the system keeps match what is installed, and where programs open: removed
    /// programs' shortcuts disabled, installed ones' enabled where WaOS disabled them, and every
    /// installed one's updated. One match runs at a time, so an older one's update never lands
    /// after a newer one's; asked again while one runs, another runs once it is done. What cannot
    /// be read or updated now is said in the log, and done at the next match.
    pub(super) fn match_shortcuts(&mut self, cx: &mut Context<Library>) {
        let Some(system) = self.shortcuts.clone() else {
            return;
        };
        if self.shortcuts_matching != ShortcutsMatching::Idle {
            self.shortcuts_matching = ShortcutsMatching::AskedAgain;
            return;
        }
        self.shortcuts_matching = ShortcutsMatching::Running;
        let kept = system.kept();
        let library = cx.weak_entity();
        // The task runs to its end whether or not its handle is kept.
        let _matching = cx.spawn(async move |cx| {
            let kept = kept.await;
            cx.update(|app: &mut App| {
                if let Some(library) = library.upgrade() {
                    library.update(app, |library, cx| match kept {
                        Ok(kept) => library.carry_out_changes(&kept, cx),
                        Err(error) => {
                            tracing::warn!(%error, "the shortcuts the system keeps cannot be read");
                            library.shortcuts_matched(cx);
                        }
                    });
                }
            });
        });
    }

    /// Changes what the system keeps, `kept`, to match what is installed.
    fn carry_out_changes(&mut self, kept: &[KeptShortcut], cx: &mut Context<Library>) {
        let Some(system) = self.shortcuts.clone() else {
            return;
        };
        let installed = self
            .installed
            .iter()
            .map(|program| program.manifest.id().clone())
            .collect();
        let changes = changes_for(kept, &installed);
        tracing::info!(
            ?changes,
            "the shortcuts are made to match what is installed"
        );
        if !changes.enable.is_empty() {
            system.enable(&changes.enable);
        }
        if !changes.disable.is_empty() {
            system.disable(&changes.disable);
        }
        if changes.update.is_empty() {
            self.shortcuts_matched(cx);
        } else {
            self.with_shortcuts(changes.update, Library::update_shortcuts, cx);
        }
    }

    /// Updates `shortcuts` where the system keeps them, which ends the match.
    fn update_shortcuts(&mut self, shortcuts: Vec<ProgramShortcut>, cx: &mut Context<Library>) {
        let Some(system) = &self.shortcuts else {
            return;
        };
        let updated = system.update(&shortcuts);
        let library = cx.weak_entity();
        // The task runs to its end whether or not its handle is kept.
        let _updating = cx.spawn(async move |cx| {
            if let Err(error) = updated.await {
                tracing::info!(%error, "the shortcuts are updated at the next match");
            }
            cx.update(|app: &mut App| {
                if let Some(library) = library.upgrade() {
                    library.update(app, |library, cx| library.shortcuts_matched(cx));
                }
            });
        });
    }

    /// A match ended: the one asked for meanwhile runs now.
    fn shortcuts_matched(&mut self, cx: &mut Context<Library>) {
        let again = self.shortcuts_matching == ShortcutsMatching::AskedAgain;
        self.shortcuts_matching = ShortcutsMatching::Idle;
        if again {
            self.match_shortcuts(cx);
        }
    }

    /// Asks the launcher to pin the program `id`'s shortcut, as `asked`.
    fn request_pin(&mut self, id: &ProgramId, asked: PinAsked, cx: &mut Context<Library>) {
        self.with_shortcuts(
            vec![id.clone()],
            move |library, shortcuts, cx| {
                let (Some(system), Some(shortcut)) = (library.shortcuts.clone(), shortcuts.first())
                else {
                    return;
                };
                let (requested, name) = (system.request_pin(shortcut), shortcut.name.clone());
                let library = cx.weak_entity();
                // The task runs to its end whether or not its handle is kept.
                let _requesting = cx.spawn(async move |cx| {
                    let Err(error) = requested.await else {
                        return;
                    };
                    tracing::info!(%error, ?asked, "the launcher did not take the request to pin");
                    if asked == PinAsked::ByTheUser {
                        cx.update(|app: &mut App| {
                            if let Some(library) = library.upgrade() {
                                library.update(app, |library, cx| {
                                    let why = format!(
                                        "{name} could not be added to the home screen: {error}"
                                    );
                                    library.failed(why, cx);
                                });
                            }
                        });
                    }
                });
            },
            cx,
        );
    }

    /// Makes the shortcuts of the installed programs among `ids`, their icons decoded and shaped
    /// on the library's runtime, and hands them to `then` on the main thread.
    fn with_shortcuts(
        &self,
        ids: Vec<ProgramId>,
        then: impl FnOnce(&mut Library, Vec<ProgramShortcut>, &mut Context<Library>) + 'static,
        cx: &mut Context<Library>,
    ) {
        let Some(system) = &self.shortcuts else {
            return;
        };
        let visible = system.icon_size();
        let opens = self.shortcut_opens();
        let made = ids
            .into_iter()
            .filter_map(|id| {
                let name = self.installed_program(&id)?.name();
                let png = self.icons.get(&id).cloned();
                Some((id, name, png))
            })
            .collect::<Vec<_>>();
        self.work_then(
            async move {
                made.into_iter()
                    .map(|(id, name, png)| ProgramShortcut {
                        icon: png.and_then(|png| shortcut_icon(&png, visible)),
                        id,
                        name,
                        opens,
                    })
                    .collect()
            },
            then,
            cx,
        );
    }

    /// What a tap on a shortcut opens: the program's own task, or WaOS with the program in a tab,
    /// as programs open now.
    fn shortcut_opens(&self) -> ShortcutOpens {
        if self.programs_open_in_tabs() {
            ShortcutOpens::InsideWaos
        } else {
            ShortcutOpens::OwnTask
        }
    }
}

/// Who asked for a program's place on the home screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PinAsked {
    /// The user, from the program's held icon.
    ByTheUser,
    /// The library, as the program was installed for the first time.
    AtInstall,
}

/// Whether the shortcuts are being made to match what is installed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ShortcutsMatching {
    #[default]
    Idle,
    Running,
    /// Running, and asked for again since it started.
    AskedAgain,
}
