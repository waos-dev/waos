//! A program's tray icons: each the shell's from the moment the shell has made it while the
//! program holds its handle, removed when the program drops it or ends, as its store's table drops
//! it; what the user does to it, and chooses in its attached menu, the shell reports into the
//! icon's [`TrayIconReports`]. Which menus an icon offers is the shell's icon's to say.
//!
//! The invariant: once an icon is removed, nothing reaches the shell's icon and nothing reaches
//! the program: its menus' handles, which outlive it in the program's hands, do nothing, a menu
//! opened through one answers none, and every stream of its events and choices ends.

use std::future::{Future, ready};
use std::sync::{Arc, Mutex, Weak};

use futures::channel::mpsc;

use crate::bindings::wasdk::platform::menus::{Image, MenuEntry, TrayIconError, TrayIconEvent};
use crate::lifecycle::Lifecycle;
use crate::news::{Inhibiting, News};
use crate::running::AppInstance;
use crate::shell::{MenuChosenFuture, ShellTrayIcon, ShellTrayIcons};
use crate::sync::locked;

/// `menus.tray-icons`: the shell's tray, where the program adds its icons.
pub struct TrayIcons(pub(super) Arc<dyn ShellTrayIcons>);

impl TrayIcons {
    /// Adds an icon of `program`'s showing `image`, with `tooltip` over it, answered once the
    /// shell has made it, or why it did not.
    pub(super) fn add(
        &self,
        program: &AppInstance,
        image: &Image,
        tooltip: Option<&str>,
    ) -> impl Future<Output = Result<TrayIcon, TrayIconError>> + Send + 'static + use<> {
        let state = Arc::new(IconState {
            shell: Mutex::new(None),
            heard: Mutex::new(Some(Heard::default())),
            lifecycle: program.lifecycle().clone(),
        });
        let reports = TrayIconReports(Arc::downgrade(&state));
        let made = self.0.add(program, image, tooltip, reports);
        async move {
            let shell = made.await?;
            *locked(&state.shell) = Some(shell);
            Ok(TrayIcon(state))
        }
    }
}

/// `menus.tray-icon`: one of the program's icons, removed when its handle is dropped.
pub struct TrayIcon(Arc<IconState>);

/// One icon, shared by its handle, its menus' handles and its reports.
struct IconState {
    /// The shell's icon, until it is removed.
    shell: Mutex<Option<Box<dyn ShellTrayIcon>>>,
    /// Where what the shell reports goes, until the icon is removed.
    heard: Mutex<Option<Heard>>,
    /// What the program's unfreeze for an event or a choice is owed to.
    lifecycle: Lifecycle,
}

/// Where what the shell reports of an icon goes.
#[derive(Default)]
struct Heard {
    /// What the user does to it.
    events: News<TrayIconEvent>,
    /// The choices made in its attached menu.
    attached_chosen: News<u32>,
}

impl TrayIcon {
    pub(super) fn set_image(&self, image: &Image) {
        self.0.request(|icon| icon.set_image(image));
    }

    pub(super) fn set_tooltip(&self, tooltip: Option<&str>) {
        self.0.request(|icon| icon.set_tooltip(tooltip));
    }

    /// The menu the platform opens by itself at the icon; none where it opens none.
    pub(super) fn attached_menu(&self) -> Option<AttachedMenu> {
        let has_one = self.0.request(|icon| icon.attached_menu().is_some());
        has_one?.then(|| AttachedMenu(Arc::clone(&self.0)))
    }

    /// What opens a menu at the icon; none where nothing does.
    pub(super) fn menu_at_icon(&self) -> Option<MenuAtIcon> {
        let has_one = self.0.request(|icon| icon.menu_at_icon().is_some());
        has_one?.then(|| MenuAtIcon(Arc::clone(&self.0)))
    }

    /// What the user does to the icon from now on, on a stream of its own; one that has ended
    /// once the icon is removed.
    pub(super) fn events(&self) -> mpsc::UnboundedReceiver<Inhibiting<TrayIconEvent>> {
        match locked(&self.0.heard).as_ref() {
            Some(heard) => heard.events.subscribe(),
            None => mpsc::unbounded().1,
        }
    }
}

impl Drop for TrayIcon {
    /// The program let the icon go: the shell removes it, and nothing more of it is heard.
    fn drop(&mut self) {
        locked(&self.0.shell).take();
        locked(&self.0.heard).take();
    }
}

impl IconState {
    /// Asks `request` of the shell's icon; none once it is removed.
    fn request<R>(&self, request: impl FnOnce(&dyn ShellTrayIcon) -> R) -> Option<R> {
        locked(&self.shell).as_deref().map(request)
    }
}

/// `menus.attached-menu`: the menu the platform opens by itself at one of the program's icons.
pub struct AttachedMenu(Arc<IconState>);

impl AttachedMenu {
    /// Replaces the menu with `entries`, whole.
    pub(super) fn set(&self, entries: &[MenuEntry]) {
        self.0.request(|icon| {
            if let Some(menu) = icon.attached_menu() {
                menu.set(entries);
            }
        });
    }

    /// The choices made in the menu from now on, on a stream of their own; one that has ended
    /// once the icon is removed.
    pub(super) fn chosen(&self) -> mpsc::UnboundedReceiver<Inhibiting<u32>> {
        match locked(&self.0.heard).as_ref() {
            Some(heard) => heard.attached_chosen.subscribe(),
            None => mpsc::unbounded().1,
        }
    }
}

/// `menus.menu-at-icon`: what opens a menu at one of the program's icons.
pub struct MenuAtIcon(Arc<IconState>);

impl MenuAtIcon {
    /// Opens `entries` at the icon now; answers none at once once the icon is removed.
    pub(super) fn show(&self, entries: &[MenuEntry]) -> MenuChosenFuture {
        let shown = self
            .0
            .request(|icon| icon.menu_at_icon().map(|menu| menu.show(entries)));
        shown.flatten().unwrap_or_else(|| Box::pin(ready(None)))
    }
}

/// Where the shell reports what the user does to one tray icon, from any thread. Reports on an
/// icon the program has let go of are dropped.
#[derive(Clone)]
pub struct TrayIconReports(Weak<IconState>);

impl TrayIconReports {
    /// The user did `event` to the icon: every stream of its events hears it, the program unfrozen
    /// until it has handled it on each.
    pub fn event(&self, event: TrayIconEvent) {
        let Some(icon) = self.0.upgrade() else {
            return;
        };
        if let Some(heard) = locked(&icon.heard).as_ref() {
            heard.events.tell(event, &icon.lifecycle);
        }
    }

    /// The user chose the item `id` in the icon's attached menu: every stream of its choices
    /// hears it, the program unfrozen until it has handled it.
    pub fn attached_menu_chosen(&self, id: u32) {
        let Some(icon) = self.0.upgrade() else {
            return;
        };
        if let Some(heard) = locked(&icon.heard).as_ref() {
            heard.attached_chosen.tell(id, &icon.lifecycle);
        }
    }
}
