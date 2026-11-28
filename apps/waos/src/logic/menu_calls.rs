//! Where the programs' context menus in WaOS's own windows and their tray icons go: each call to
//! the main thread, where Waft's menus are (`state::menus`). The engine calls from a program's
//! thread and is answered later, through the future it was given: a menu opened with the item
//! chosen, and a tray icon added once Waft has made it, with the menus it offers, or with why
//! Waft's tray did not show it. A program's menu bar is never WaOS's: on macOS it is its app's, as
//! its context menus are (`logic::macos_shell`).
//!
//! The invariant: every call taken reaches the main thread or is answered at once, so no program
//! waits for an answer that cannot come; and an icon's calls reach it in the order made, its
//! removal last.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::menus::{Image, MenuEntry, TrayIconError};
use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    MenuChosenFuture, RunningProgram, ShellAttachedMenu, ShellContextMenus, ShellMenuAtIcon,
    ShellTrayIcon, ShellTrayIcons, TrayIconAddFuture, TrayIconReports,
};

/// The id the app gives each tray icon a program adds, which its calls carry.
pub(crate) type IconKey = u64;

/// One program's menu call, on its way to the main thread.
pub(crate) enum MenuCall {
    /// A menu opened at a point of one of the program's views, in its physical pixels.
    ContextMenu {
        program: RunningProgram,
        view: ViewId,
        at: (f64, f64),
        entries: Vec<MenuEntry>,
        chosen: oneshot::Sender<Option<u32>>,
    },
    /// The program adds a tray icon, answered once Waft has made it.
    AddIcon {
        icon: IconKey,
        image: Image,
        tooltip: Option<String>,
        reports: TrayIconReports,
        added: oneshot::Sender<Result<IconMenus, TrayIconError>>,
    },
    /// Something asked of one of the program's tray icons.
    Icon(IconKey, IconCall),
}

/// What a program asks of one of its tray icons.
pub(crate) enum IconCall {
    SetImage(Image),
    SetTooltip(Option<String>),
    SetAttachedMenu(Vec<MenuEntry>),
    ShowMenuAtIcon {
        entries: Vec<MenuEntry>,
        chosen: oneshot::Sender<Option<u32>>,
    },
    /// The program let the icon go.
    Remove,
}

/// The app's menus, as the engine reaches them: every call goes to the main thread. Every clone
/// sends to the same.
#[derive(Clone)]
pub(crate) struct MenusOnRequest {
    calls: UnboundedSender<MenuCall>,
    last_icon: Arc<AtomicU64>,
}

impl MenusOnRequest {
    /// The menus, and where their calls arrive on the main thread.
    pub(crate) fn new() -> (MenusOnRequest, UnboundedReceiver<MenuCall>) {
        let (calls, arriving) = mpsc::unbounded();
        let menus = MenusOnRequest {
            calls,
            last_icon: Arc::default(),
        };
        (menus, arriving)
    }

    fn send(&self, call: MenuCall) {
        if self.calls.unbounded_send(call).is_err() {
            tracing::info!("WaOS is quitting: a menu call goes nowhere");
        }
    }
}

impl ShellContextMenus for MenusOnRequest {
    fn show(
        &self,
        program: &RunningProgram,
        view: ViewId,
        x: f64,
        y: f64,
        entries: &[MenuEntry],
    ) -> MenuChosenFuture {
        let (chosen, answered) = oneshot::channel();
        self.send(MenuCall::ContextMenu {
            program: program.clone(),
            view,
            at: (x, y),
            entries: entries.to_vec(),
            chosen,
        });
        Box::pin(answered.map(|chosen| chosen.unwrap_or(None)))
    }
}

impl ShellTrayIcons for MenusOnRequest {
    fn add(
        &self,
        _program: &RunningProgram,
        image: &Image,
        tooltip: Option<&str>,
        reports: TrayIconReports,
    ) -> TrayIconAddFuture {
        let icon = self.last_icon.fetch_add(1, Ordering::Relaxed) + 1;
        let (added, answered) = oneshot::channel();
        self.send(MenuCall::AddIcon {
            icon,
            image: image.clone(),
            tooltip: tooltip.map(str::to_owned),
            reports,
            added,
        });
        let calls = self.calls.clone();
        Box::pin(answered.map(move |added| {
            let menus = added.unwrap_or_else(|_| Err(quitting()))?;
            let icon = IconOnRequest { icon, calls, menus };
            Ok(Box::new(icon) as Box<dyn ShellTrayIcon>)
        }))
    }
}

/// The menus a tray icon Waft made offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IconMenus {
    /// One the system opens by itself: macOS's on a click, Linux's when the desktop decides.
    pub(crate) attached: bool,
    /// One the app opens at the icon: macOS's and Windows'.
    pub(crate) at_icon: bool,
}

/// Why an icon asked for while WaOS quits is not there.
fn quitting() -> TrayIconError {
    TrayIconError::Failed("WaOS is quitting".to_owned())
}

/// One tray icon, as the engine holds it: its calls go to the main thread, and its removal as it
/// drops.
struct IconOnRequest {
    icon: IconKey,
    calls: UnboundedSender<MenuCall>,
    /// The menus it offers, as Waft's icon said.
    menus: IconMenus,
}

impl IconOnRequest {
    fn send(&self, call: IconCall) {
        if self
            .calls
            .unbounded_send(MenuCall::Icon(self.icon, call))
            .is_err()
        {
            tracing::info!("WaOS is quitting: a tray icon's call goes nowhere");
        }
    }
}

impl ShellTrayIcon for IconOnRequest {
    fn set_image(&self, image: &Image) {
        self.send(IconCall::SetImage(image.clone()));
    }

    fn set_tooltip(&self, tooltip: Option<&str>) {
        self.send(IconCall::SetTooltip(tooltip.map(str::to_owned)));
    }

    fn attached_menu(&self) -> Option<&dyn ShellAttachedMenu> {
        self.menus
            .attached
            .then_some(self as &dyn ShellAttachedMenu)
    }

    fn menu_at_icon(&self) -> Option<&dyn ShellMenuAtIcon> {
        self.menus.at_icon.then_some(self as &dyn ShellMenuAtIcon)
    }
}

impl ShellAttachedMenu for IconOnRequest {
    fn set(&self, entries: &[MenuEntry]) {
        self.send(IconCall::SetAttachedMenu(entries.to_vec()));
    }
}

impl ShellMenuAtIcon for IconOnRequest {
    fn show(&self, entries: &[MenuEntry]) -> MenuChosenFuture {
        let (chosen, answered) = oneshot::channel();
        self.send(IconCall::ShowMenuAtIcon {
            entries: entries.to_vec(),
            chosen,
        });
        Box::pin(answered.map(|chosen| chosen.unwrap_or(None)))
    }
}

impl Drop for IconOnRequest {
    fn drop(&mut self) {
        self.send(IconCall::Remove);
    }
}
