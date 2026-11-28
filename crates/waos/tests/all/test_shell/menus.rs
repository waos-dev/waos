//! The shell's menus, as the platform's would hold them: the menu bar each program set; each
//! menu opened, at a view or at a tray icon, open until a test chooses in it as the user would;
//! and each tray icon a program added, which a test does to as the user would through its
//! reports, and which is removed once the engine drops it. Its icons offer both menus, as macOS's
//! do, until a test takes one away, as Windows' or Linux's systems have none of it, or has the
//! tray refuse them, counting each it refuses.
//!
//! The invariant: an icon reaches the test as the program asks for it, before the program hears
//! its reports, and the engine drops what is reported before then. A test reports once the
//! program shows it holds the icon: it set the icon's attached menu, took its menu at the icon, or
//! said so in its output.

use std::future::ready;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::menus::{Image, MenuEntry, TrayIconError, TrayIconEvent};
use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    MenuChosenFuture, RunningProgram, ShellAttachedMenu, ShellContextMenus, ShellMenuAtIcon,
    ShellMenuBar, ShellTrayIcon, ShellTrayIcons, TrayIconAddFuture, TrayIconReports,
};

use waos_headless::testing::Changes;

/// The menu bars set, the menus opened and the tray icons added, in order.
pub struct TestMenus {
    /// Each bar a program set, with the program's name.
    bars: Mutex<Vec<(String, Vec<MenuEntry>)>>,
    opened: Arc<OpenedMenus>,
    icons: Mutex<Vec<Arc<TestTrayIcon>>>,
    /// Whether the icons added from now on offer a menu the system opens by itself.
    attached_menus: AtomicBool,
    /// Whether the icons added from now on offer a menu opened at them.
    menus_at_icon: AtomicBool,
    /// Why the tray refuses every icon, from now on; none while it shows them.
    refusal: Mutex<Option<String>>,
    /// How many icons the tray has refused.
    refused: AtomicUsize,
    changes: Arc<Changes>,
}

impl Default for TestMenus {
    fn default() -> TestMenus {
        TestMenus::announcing(&Arc::default())
    }
}

/// Each menu opened, in order, and where its choice goes until a test makes it.
struct OpenedMenus {
    menus: Mutex<Vec<OpenedMenu>>,
    changes: Arc<Changes>,
}

struct OpenedMenu {
    at: OpenedAt,
    entries: Vec<MenuEntry>,
    choice: Option<oneshot::Sender<Option<u32>>>,
}

/// Where a menu was opened.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenedAt {
    /// At a point of a program's view, in its physical pixels.
    View {
        program: String,
        view: ViewId,
        x: f64,
        y: f64,
    },
    /// At a tray icon.
    TrayIcon,
}

impl TestMenus {
    /// No bar set, no menu opened and no icon added, each announced to `changes` as it comes.
    pub fn announcing(changes: &Arc<Changes>) -> TestMenus {
        TestMenus {
            bars: Mutex::default(),
            opened: Arc::new(OpenedMenus {
                menus: Mutex::default(),
                changes: Arc::clone(changes),
            }),
            icons: Mutex::default(),
            attached_menus: AtomicBool::new(true),
            menus_at_icon: AtomicBool::new(true),
            refusal: Mutex::default(),
            refused: AtomicUsize::new(0),
            changes: Arc::clone(changes),
        }
    }

    /// The icons added from now on offer no menu the system opens by itself, as Windows'.
    pub fn without_attached_menus(&self) {
        self.attached_menus.store(false, Ordering::SeqCst);
    }

    /// The icons added from now on offer no menu opened at them, as Linux's.
    pub fn without_menus_at_icon(&self) {
        self.menus_at_icon.store(false, Ordering::SeqCst);
    }

    /// The tray shows no icon from now on, refusing each for `reason`.
    pub fn refusing(&self, reason: &str) {
        *self.refusal.lock().unwrap() = Some(reason.to_owned());
    }

    /// Each menu bar set so far, with the name of the program that set it, in order.
    pub fn bars(&self) -> Vec<(String, Vec<MenuEntry>)> {
        self.bars.lock().unwrap().clone()
    }

    /// Waits until the `count`th menu is opened, and answers where, and what it shows.
    pub fn wait_for_opened(&self, count: usize) -> (OpenedAt, Vec<MenuEntry>) {
        self.changes
            .wait_until(&format!("{count} menus opened"), || {
                self.opened.menus.lock().unwrap().len() >= count
            });
        let opened = &self.opened.menus.lock().unwrap()[count - 1];
        (opened.at.clone(), opened.entries.clone())
    }

    /// The user chooses `choice` in the `count`th menu opened; none closes it without a choice.
    pub fn choose(&self, count: usize, choice: Option<u32>) {
        let choose = self.opened.menus.lock().unwrap()[count - 1].choice.take();
        let _ = choose.expect("a menu open").send(choice);
    }

    /// How many tray icons the tray shows, or showed before they were removed.
    pub fn icons_added(&self) -> usize {
        self.icons.lock().unwrap().len()
    }

    /// Waits until the `count`th tray icon is added, and answers it.
    pub fn wait_for_icon(&self, count: usize) -> Arc<TestTrayIcon> {
        self.changes
            .wait_until(&format!("{count} tray icons added"), || {
                self.icons.lock().unwrap().len() >= count
            });
        Arc::clone(&self.icons.lock().unwrap()[count - 1])
    }

    /// Waits until the tray has refused the `count`th icon.
    pub fn wait_for_refused(&self, count: usize) {
        self.changes
            .wait_until(&format!("{count} tray icons refused"), || {
                self.refused.load(Ordering::SeqCst) >= count
            });
    }
}

impl OpenedMenus {
    /// Opens `entries` at `at`, until a test chooses in it.
    fn open(&self, at: OpenedAt, entries: &[MenuEntry]) -> MenuChosenFuture {
        let (choice, chosen) = oneshot::channel();
        self.menus.lock().unwrap().push(OpenedMenu {
            at,
            entries: entries.to_vec(),
            choice: Some(choice),
        });
        self.changes.announce();
        Box::pin(chosen.map(|chosen| chosen.unwrap_or(None)))
    }
}

impl ShellMenuBar for TestMenus {
    fn set(&self, program: &RunningProgram, entries: &[MenuEntry]) {
        let set = (program.name().to_owned(), entries.to_vec());
        self.bars.lock().unwrap().push(set);
        self.changes.announce();
    }
}

impl ShellContextMenus for TestMenus {
    fn show(
        &self,
        program: &RunningProgram,
        view: ViewId,
        x: f64,
        y: f64,
        entries: &[MenuEntry],
    ) -> MenuChosenFuture {
        let at = OpenedAt::View {
            program: program.name().to_owned(),
            view,
            x,
            y,
        };
        self.opened.open(at, entries)
    }
}

impl ShellTrayIcons for TestMenus {
    fn add(
        &self,
        program: &RunningProgram,
        image: &Image,
        tooltip: Option<&str>,
        reports: TrayIconReports,
    ) -> TrayIconAddFuture {
        if let Some(reason) = self.refusal.lock().unwrap().clone() {
            self.refused.fetch_add(1, Ordering::SeqCst);
            self.changes.announce();
            return Box::pin(ready(Err(TrayIconError::Failed(reason))));
        }
        let icon = Arc::new(TestTrayIcon {
            program: program.name().to_owned(),
            image: Mutex::new(image.clone()),
            tooltip: Mutex::new(tooltip.map(str::to_owned)),
            attached_menu: Mutex::default(),
            menu_at_icon_taken: AtomicBool::new(false),
            reports,
            removed: AtomicBool::new(false),
            opened: Arc::clone(&self.opened),
            offers_attached_menu: self.attached_menus.load(Ordering::SeqCst),
            offers_menu_at_icon: self.menus_at_icon.load(Ordering::SeqCst),
            changes: Arc::clone(&self.changes),
        });
        self.icons.lock().unwrap().push(Arc::clone(&icon));
        self.changes.announce();
        Box::pin(ready(Ok(
            Box::new(AddedIcon(icon)) as Box<dyn ShellTrayIcon>
        )))
    }
}

/// A tray icon a program added: what it shows, and what reports to it what the user does.
pub struct TestTrayIcon {
    pub program: String,
    image: Mutex<Image>,
    tooltip: Mutex<Option<String>>,
    /// What its attached menu shows; none before the program sets it.
    attached_menu: Mutex<Option<Vec<MenuEntry>>>,
    /// Whether the program has taken what opens a menu at it.
    menu_at_icon_taken: AtomicBool,
    reports: TrayIconReports,
    removed: AtomicBool,
    /// Where a menu opened at it goes.
    opened: Arc<OpenedMenus>,
    offers_attached_menu: bool,
    offers_menu_at_icon: bool,
    changes: Arc<Changes>,
}

impl TestTrayIcon {
    pub fn image(&self) -> Image {
        self.image.lock().unwrap().clone()
    }

    pub fn tooltip(&self) -> Option<String> {
        self.tooltip.lock().unwrap().clone()
    }

    /// Waits until the program sets the icon's attached menu, and answers what it shows.
    pub fn wait_for_attached_menu(&self) -> Vec<MenuEntry> {
        self.changes.wait_until("an attached menu set", || {
            self.attached_menu.lock().unwrap().is_some()
        });
        self.attached_menu.lock().unwrap().clone().unwrap()
    }

    /// Waits until the program has taken what opens a menu at the icon.
    pub fn wait_for_menu_at_icon_taken(&self) {
        self.changes.wait_until("the menu at the icon taken", || {
            self.menu_at_icon_taken.load(Ordering::SeqCst)
        });
    }

    /// The user does `event` to the icon.
    pub fn report(&self, event: TrayIconEvent) {
        self.reports.event(event);
    }

    /// The user chooses the item `id` in the icon's attached menu.
    pub fn choose_in_attached_menu(&self, id: u32) {
        self.reports.attached_menu_chosen(id);
    }

    /// Whether the engine has dropped the icon, which removes it.
    pub fn is_removed(&self) -> bool {
        self.removed.load(Ordering::SeqCst)
    }
}

impl ShellAttachedMenu for TestTrayIcon {
    fn set(&self, entries: &[MenuEntry]) {
        *self.attached_menu.lock().unwrap() = Some(entries.to_vec());
        self.changes.announce();
    }
}

impl ShellMenuAtIcon for TestTrayIcon {
    fn show(&self, entries: &[MenuEntry]) -> MenuChosenFuture {
        self.opened.open(OpenedAt::TrayIcon, entries)
    }
}

/// The icon as the engine holds it: removed when dropped.
struct AddedIcon(Arc<TestTrayIcon>);

impl ShellTrayIcon for AddedIcon {
    fn set_image(&self, image: &Image) {
        *self.0.image.lock().unwrap() = image.clone();
        self.0.changes.announce();
    }

    fn set_tooltip(&self, tooltip: Option<&str>) {
        *self.0.tooltip.lock().unwrap() = tooltip.map(str::to_owned);
        self.0.changes.announce();
    }

    fn attached_menu(&self) -> Option<&dyn ShellAttachedMenu> {
        let icon = self.0.as_ref();
        icon.offers_attached_menu
            .then_some(icon as &dyn ShellAttachedMenu)
    }

    fn menu_at_icon(&self) -> Option<&dyn ShellMenuAtIcon> {
        let icon = self.0.as_ref();
        if !icon.offers_menu_at_icon {
            return None;
        }
        icon.menu_at_icon_taken.store(true, Ordering::SeqCst);
        icon.changes.announce();
        Some(icon as &dyn ShellMenuAtIcon)
    }
}

impl Drop for AddedIcon {
    fn drop(&mut self) {
        self.0.removed.store(true, Ordering::SeqCst);
        self.0.changes.announce();
    }
}
