//! The library's pages, which every layout shows the one it is on, and its settings pages: the
//! global settings, and each program's, read from the store and changed through it. A program
//! asking for one of its switches opens its own page, installed or run in development; the
//! program hears the user is done once the user leaves the page, or the library goes out of
//! sight. A program's page also says when the system refuses the program its notifications, as
//! macOS may refuse a program's app whatever the user allows in WaOS, and opens the system's
//! settings at them, since the system asks only once.
//!
//! The invariant: every program waiting for the user to leave a settings page hears it exactly
//! once, at the first moment the page is no longer in front of the user; and what the system
//! refuses is never written into the program's settings, which keep the user's own answer.

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::ui::OpenFuture;
use waft::{App, Context, Entity, WeakEntity};
use waos::{Switch, SystemPermissionState};

use crate::logic::activity::activity_of;
use crate::logic::permissions::Permission;
use waos_package::ProgramId;

use super::{Library, WantsToBeSeen};
#[cfg(target_os = "macos")]
use crate::logic::notification_permissions::NotificationPermissions;
use crate::logic::prompts::SettingsOpened;
use crate::logic::setting_changes::{
    SettingChange, SettingsOf, ShownSettings, change_global, change_program, global_page,
    program_page,
};
use crate::os::notification_settings_link;

/// What the system denies of what a settings page shows, whatever the page allows: the user's
/// answer in WaOS is one layer, the system's another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DeniedBySystem {
    /// The page's program's notifications.
    pub(crate) notifications: bool,
    /// One of the folders the page lists, on the phone's storage.
    pub(crate) folders: bool,
}

/// A page of the library.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LibraryPage {
    /// The home: the programs installed, as icons.
    Home,
    /// The app store: every app the sources list, the updates, and the sources.
    AppStore,
    /// An app's page in the app store: what its source says of it, before it is got.
    App(ProgramId),
    /// A page of the settings.
    Settings(SettingsPage),
}

/// A page of the library's settings, as iOS's Settings leads to its own: General, Privacy &
/// Security by kind, and each app.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SettingsPage {
    /// The first, which leads to each of the others.
    Root,
    /// WaOS's own settings: where apps open, where WaOS stays, the phone's files.
    General,
    /// The kinds of permission, each with its default.
    Permissions,
    /// One kind: its default, which every app follows until set otherwise, and the apps that
    /// are, as a browser's site settings show a permission's.
    Permission(Permission),
    /// One app's: each kind of permission with what it holds, and what is the app's alone.
    App(ProgramId),
    /// One kind of permission for one app: what it holds, or that it follows the default.
    AppPermission(ProgramId, Permission),
    /// What else is one app's alone, a page of each: the machines it acts on, its file pickers,
    /// its sound output, its links and its roles.
    AppDetail(ProgramId, AppDetail),
}

/// What is one app's alone, beside its permissions, each on a page of its own under the app's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AppDetail {
    /// The machines it acts on, and which one answers when it asks for a computer.
    Machines,
    /// Its file pickers, each asking its user or simulated: answered with a path set here.
    FilePickers,
    /// Which sound output it plays to: asked, or one set here.
    SoundOutput,
    /// What it was answered when it asked to open links of its own kinds.
    Links,
    /// What it was answered when it asked to be more than an app.
    Roles,
}

impl LibraryPage {
    /// The page this one was opened over, which going back shows: an app's page is over the
    /// app store, a page of the settings over their first, every other page over the home; none
    /// beneath the home.
    pub(crate) fn beneath(&self) -> Option<LibraryPage> {
        match self {
            LibraryPage::Home => None,
            LibraryPage::AppStore | LibraryPage::Settings(SettingsPage::Root) => {
                Some(LibraryPage::Home)
            }
            LibraryPage::App(_) => Some(LibraryPage::AppStore),
            LibraryPage::Settings(SettingsPage::Permission(_)) => {
                Some(LibraryPage::Settings(SettingsPage::Permissions))
            }
            LibraryPage::Settings(
                SettingsPage::AppPermission(id, _) | SettingsPage::AppDetail(id, _),
            ) => Some(LibraryPage::Settings(SettingsPage::App(id.clone()))),
            LibraryPage::Settings(_) => Some(LibraryPage::Settings(SettingsPage::Root)),
        }
    }

    /// The pages open up to this one, the home first: what a navigator stacks to show it.
    pub(crate) fn stacked(&self) -> Vec<LibraryPage> {
        let mut pages = vec![self.clone()];
        while let Some(beneath) = pages.last().and_then(LibraryPage::beneath) {
            pages.push(beneath);
        }
        pages.reverse();
        pages
    }
}

/// A program asked the user to change one of its switches, on the page open for it.
#[derive(Clone, Debug)]
pub(crate) struct SwitchAsked {
    /// The program, as its user knows it.
    pub(crate) name: String,
    pub(crate) switch: Switch,
}

impl Library {
    /// The page the library is on.
    pub(crate) fn page(&self) -> &LibraryPage {
        &self.page
    }

    /// The switch a program asked the user to change, on the settings page the library is on.
    pub(crate) fn switch_asked(&self) -> Option<&SwitchAsked> {
        self.switch_asked.as_ref()
    }

    /// Where the library is on a settings page of an app no longer installed, it goes to the
    /// settings' first page: that app has nothing left to show.
    pub(super) fn leave_a_removed_apps_page(&mut self, cx: &mut Context<Library>) {
        let LibraryPage::Settings(
            SettingsPage::App(id)
            | SettingsPage::AppPermission(id, _)
            | SettingsPage::AppDetail(id, _),
        ) = &self.page
        else {
            return;
        };
        if self
            .installed()
            .iter()
            .all(|program| program.manifest.id() != id)
        {
            self.show_page(LibraryPage::Settings(SettingsPage::Root), cx);
        }
    }

    /// The library goes back to the page the one it is on was opened over; nowhere from the
    /// home.
    pub(crate) fn go_back(&mut self, cx: &mut Context<Library>) {
        if let Some(beneath) = self.page.beneath() {
            self.show_page(beneath, cx);
        }
    }

    /// The library goes to `page`; a program waiting for the user to leave the page it was on
    /// hears so, and an app's page has its screenshots read.
    pub(crate) fn show_page(&mut self, page: LibraryPage, cx: &mut Context<Library>) {
        if page != self.page {
            self.left_settings();
            match &page {
                LibraryPage::Settings(
                    SettingsPage::App(_)
                    | SettingsPage::AppPermission(..)
                    | SettingsPage::AppDetail(..),
                ) => {
                    self.offered = self.environments.offered();
                }
                LibraryPage::App(id) => self.read_screenshots_of(&id.clone(), cx),
                LibraryPage::Home | LibraryPage::AppStore | LibraryPage::Settings(_) => {}
            }
            self.page = page;
            cx.notify();
        }
    }

    /// The library went out of sight: a program waiting for the user to leave its settings page
    /// hears so.
    pub(crate) fn hidden(&mut self, cx: &mut Context<Library>) {
        if self.left_settings() {
            cx.notify();
        }
    }

    /// The settings `of` shows; why not, when they cannot be read.
    pub(crate) fn settings(&self, of: &SettingsOf) -> Result<ShownSettings, String> {
        let permissions = &self.permissions;
        let global = permissions
            .global_settings()
            .map_err(|error| format!("{error}"))?;
        match of {
            SettingsOf::Global => Ok(global_page(&global)),
            SettingsOf::Program(id) => {
                let own = permissions
                    .app_settings(id)
                    .map_err(|error| format!("{error}"))?;
                Ok(program_page(&global, &own, &self.offered))
            }
        }
    }

    /// What the program `of` shows did over the days kept of it, today's as counted now; none on
    /// the global page.
    pub(crate) fn activity(&self, of: &SettingsOf) -> Option<waos_activity::Activity> {
        match of {
            SettingsOf::Global => None,
            SettingsOf::Program(id) => Some(activity_of(&self.engine, id)),
        }
    }

    /// Makes `change` to the settings `of` shows; one that cannot be made is the library's
    /// problem.
    pub(crate) fn change_settings(
        &mut self,
        of: &SettingsOf,
        change: SettingChange,
        cx: &mut Context<Library>,
    ) {
        let mut refused = None;
        let permissions = &self.permissions;
        let changed = match of {
            SettingsOf::Global => permissions.change_global_settings(|global| {
                refused = change_global(global, change).err();
            }),
            SettingsOf::Program(id) => match permissions.global_settings() {
                Ok(global) => permissions.change_app_settings(id, |own| {
                    refused = change_program(own, &global, change).err();
                }),
                Err(error) => Err(error),
            },
        };
        match (changed, refused) {
            (Err(error), _) => self.failed(format!("{error}"), cx),
            (Ok(()), Some(why)) => self.failed(why, cx),
            (Ok(()), None) => self.succeeded(cx),
        }
    }

    /// What the system denies of what the page of `of` shows as `shown`, whatever the page
    /// allows: the program's notifications, as its app last said, and a folder on the phone's
    /// storage while WaOS may not reach it.
    pub(crate) fn denied_by_system(
        &self,
        of: &SettingsOf,
        shown: &ShownSettings,
        app: &App,
    ) -> DeniedBySystem {
        let notifications = match of {
            SettingsOf::Program(id) => self.notifications_denied(id),
            SettingsOf::Global => false,
        };
        let mounts = &shown.file_system.value.mounts;
        DeniedBySystem {
            notifications,
            folders: (mounts.iter()).any(|mount| self.system_keeps_from(&mount.host, app)),
        }
    }

    /// Whether the system denies the program `id` its notifications, as its app last said,
    /// whatever its answer on its page.
    fn notifications_denied(&self, id: &ProgramId) -> bool {
        (self.notification_permissions.as_ref())
            .is_some_and(|said| said.state(id) == SystemPermissionState::Denied)
    }

    /// From now on the pages read what the system said of each program's notifications from
    /// `permissions`, where each program's app says it.
    #[cfg(target_os = "macos")]
    pub(crate) fn read_notification_permissions(&mut self, permissions: NotificationPermissions) {
        self.notification_permissions = Some(permissions);
    }

    /// A program's app read the system's permission for its notifications again: as WaOS welcomed
    /// the app, as the system's question was answered, as the app or WaOS came to the front, or as
    /// a post was answered; the program's page shows what was read.
    #[cfg(target_os = "macos")]
    pub(crate) fn notification_permission_read(&mut self, cx: &mut Context<Library>) {
        cx.notify();
    }

    /// Opens the system's settings at the notifications of the program `id`'s app, where the user
    /// lets it notify once the system refused it, the system asking only once; where the system
    /// has no such page, nothing. One that does not open is the library's problem.
    pub(crate) fn open_notification_settings(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        let Some(link) = notification_settings_link(id) else {
            return;
        };
        tracing::info!(%id, link, "opening the system's settings at the program's notifications");
        let opening = cx.platform().opener().map(|opener| opener.open_link(&link));
        let Some(opening) = opening else {
            self.failed("the system has no opener for its settings".to_owned(), cx);
            return;
        };
        self.fail_if_not_opened(opening, cx);
    }

    /// Makes `opening` the library's problem if it does not open.
    fn fail_if_not_opened(&self, opening: OpenFuture, cx: &mut Context<Library>) {
        let library = cx.weak_entity();
        // The task runs to its end whether or not its handle is kept.
        let _opened = cx.spawn(async move |cx| {
            let Err(error) = opening.await else {
                return;
            };
            cx.update(|app: &mut App| {
                if let Some(library) = library.upgrade() {
                    library.update(app, |library, cx| {
                        library.failed(format!("System Settings did not open: {error}"), cx);
                    });
                }
            });
        });
    }

    /// An app asks its user to change `switch`, and waits on `left` for the user to leave the
    /// page: its own page of the switch's kind opens, under its name once installed and its id
    /// before.
    fn settings_asked(&mut self, asked: SettingsOpened, cx: &mut Context<Library>) {
        let name = self
            .installed_program(&asked.app)
            .map_or_else(|| asked.app.to_string(), |program| program.name());
        let page = SettingsPage::AppPermission(asked.app, Permission::of_switch(asked.switch));
        tracing::info!(name, switch = ?asked.switch, "an app asks its user to change a switch");
        self.show_page(LibraryPage::Settings(page), cx);
        self.switch_asked = Some(SwitchAsked {
            name,
            switch: asked.switch,
        });
        self.waiting_to_leave.push(asked.left);
        cx.emit(WantsToBeSeen);
        cx.notify();
    }

    /// Tells the programs waiting for the user to leave a settings page that the user has;
    /// whether any was.
    fn left_settings(&mut self) -> bool {
        self.switch_asked = None;
        let waiting = std::mem::take(&mut self.waiting_to_leave);
        let any = !waiting.is_empty();
        for left in waiting {
            // A program that ended meanwhile hears nothing.
            let _ = left.send(());
        }
        any
    }
}

/// From now on, a program asking its user to change a switch reaches `library`, from `opened`;
/// and the library hears each change of the settings, which may come from a program's answer.
pub(crate) fn open_settings_when_asked(
    app: &mut App,
    library: &Entity<Library>,
    opened: UnboundedReceiver<SettingsOpened>,
) {
    take_settings_requests(app, library.downgrade(), opened);
    notify_settings_changes(app, library);
}

fn take_settings_requests(
    app: &mut App,
    library: WeakEntity<Library>,
    mut opened: UnboundedReceiver<SettingsOpened>,
) {
    app.spawn(async move |cx| {
        while let Some(asked) = opened.next().await {
            let Some(library) = library.upgrade() else {
                break;
            };
            cx.update(|app| library.update(app, |library, cx| library.settings_asked(asked, cx)));
        }
    });
}

/// The library notifies at each change of the settings, so a page shows the change.
fn notify_settings_changes(app: &mut App, library: &Entity<Library>) {
    let watched = library.downgrade();
    let permissions = library.read(app).permissions.clone();
    app.spawn(async move |cx| {
        loop {
            permissions.changed().await;
            let Some(library) = watched.upgrade() else {
                break;
            };
            cx.update(|app| library.update(app, |_, cx| cx.notify()));
        }
    });
}

/// A program waiting for the user to leave a settings page: what tells it so.
pub(crate) type LeftSettings = oneshot::Sender<()>;
