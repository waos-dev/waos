//! `wasdk:platform/menus`: a program's menu bar, its context menus and its tray icons, in the
//! places the shell has them.
//!
//! The engine keeps each program's menu bar as the program set it last, whole
//! ([`ProgramMenus`]), which the app reads to show the bar of the program it shows, and tells the
//! shell of each; a choice the shell reports in it reaches every stream of choices the program
//! opened. A context menu, and a menu at a tray icon, is the shell's to show, its choice coming
//! back as the call's answer. A tray icon is the shell's while the program holds its handle
//! (`tray_icons`). The interface is served to a program by `host`.
//!
//! The invariant: a choice or an event on its way to a program is news (`news`), which holds it
//! unfrozen until the program has handled it, so a frozen program answers a click on its tray icon
//! as it answers a paste of a form it promised; and only one that a stream will carry holds it, so
//! a program that reads nothing is not unfrozen for nothing.

pub(crate) mod host;
mod tray_icons;

use std::sync::{Arc, Mutex};

use futures::channel::mpsc;

use crate::bindings::wasdk::platform::menus::MenuEntry;
use crate::lifecycle::Lifecycle;
use crate::news::{Inhibiting, News};
use crate::running::RunningProgram;
use crate::shell::{Shell, ShellContextMenus, ShellMenuBar, ShellTrayIcons};
use crate::sync::locked;
pub use tray_icons::TrayIconReports;
pub use tray_icons::{AttachedMenu, MenuAtIcon, TrayIcon, TrayIcons};

/// The shell's places for menus, as a program reaches them: each none where the shell has none.
#[derive(Clone)]
pub(crate) struct MenuPlaces {
    menu_bar: Option<Arc<dyn ShellMenuBar>>,
    context_menus: Option<Arc<dyn ShellContextMenus>>,
    tray_icons: Option<Arc<dyn ShellTrayIcons>>,
}

impl MenuPlaces {
    /// The places `shell` has.
    pub(crate) fn of(shell: &Shell) -> MenuPlaces {
        MenuPlaces {
            menu_bar: shell.menu_bar.clone(),
            context_menus: shell.context_menus.clone(),
            tray_icons: shell.tray_icons.clone(),
        }
    }

    /// No places at all, as a shell that has none of them.
    pub(crate) fn none() -> MenuPlaces {
        MenuPlaces {
            menu_bar: None,
            context_menus: None,
            tray_icons: None,
        }
    }
}

/// A program's menu bar as it set it last, and the choices the user makes in it.
pub(crate) struct ProgramMenus {
    /// What the program's unfreeze for a choice is owed to.
    lifecycle: Lifecycle,
    /// None while the program has set none: the platform's standard bar is its own.
    bar: Mutex<Option<Arc<[MenuEntry]>>>,
    chosen: News<u32>,
}

impl ProgramMenus {
    /// No menu bar yet, for the program whose lifecycle is `lifecycle`.
    pub(crate) fn new(lifecycle: Lifecycle) -> ProgramMenus {
        ProgramMenus {
            lifecycle,
            bar: Mutex::default(),
            chosen: News::default(),
        }
    }

    /// The menu bar the program set last; none while it has set none.
    pub(crate) fn bar(&self) -> Option<Arc<[MenuEntry]>> {
        locked(&self.bar).clone()
    }

    /// The program set its menu bar to `entries`, whole; answers it as kept.
    fn set_bar(&self, entries: Vec<MenuEntry>) -> Arc<[MenuEntry]> {
        let bar: Arc<[MenuEntry]> = entries.into();
        *locked(&self.bar) = Some(Arc::clone(&bar));
        bar
    }

    /// The user chose the item `id` in the program's menu bar.
    pub(crate) fn chosen(&self, id: u32) {
        self.chosen.tell(id, &self.lifecycle);
    }

    /// The choices made in the program's menu bar from now on, on a stream of their own.
    fn choices(&self) -> mpsc::UnboundedReceiver<Inhibiting<u32>> {
        self.chosen.subscribe()
    }
}

/// `menus.menu-bar`: the shell's menu bar, as the program sets its own.
pub struct MenuBar(Arc<dyn ShellMenuBar>);

impl MenuBar {
    /// `program`'s menu bar is `entries` from now on: kept, then told to the shell.
    fn set(&self, program: &RunningProgram, entries: Vec<MenuEntry>) {
        let bar = program.menus().set_bar(entries);
        self.0.set(program, &bar);
    }
}

/// `menus.context-menus`: the shell's menus opened at a point of a view.
pub struct ContextMenus(Arc<dyn ShellContextMenus>);
