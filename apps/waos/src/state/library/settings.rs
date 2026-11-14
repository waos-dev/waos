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
use waos::Switch;
use waos_package::ProgramId;

use super::{Library, WantsToBeSeen};
use crate::logic::prompts::SettingsOpened;
use crate::logic::setting_changes::{
    SettingChange, SettingsOf, ShownSettings, change_global, change_program, global_page,
    program_page,
};
use crate::os::notification_settings_link;

/// A page of the library.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LibraryPage {
    /// The home: the programs installed, as icons.
    Home,
    /// Getting programs: the sources added, with what each lists, and the field that adds one.
    GetPrograms,
    /// The global settings, or a program's.
    Settings(SettingsOf),
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

    /// The library goes to `page`; a program waiting for the user to leave the page it was on
    /// hears so.
    pub(crate) fn show_page(&mut self, page: LibraryPage, cx: &mut Context<Library>) {
        if page != self.page {
            self.left_settings();
            if let LibraryPage::Settings(SettingsOf::Program(_)) = &page {
                self.offered = self.runs.environments.offered();
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
        let global = self
            .store
            .global_settings()
            .map_err(|error| format!("{error}"))?;
        match of {
            SettingsOf::Global => Ok(global_page(&global)),
            SettingsOf::Program(id) => {
                let own = self
                    .store
                    .program_settings(id)
                    .map_err(|error| format!("{error}"))?;
                Ok(program_page(&global, &own, &self.offered))
            }
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
        let changed = match of {
            SettingsOf::Global => self.store.change_global_settings(|global| {
                refused = change_global(global, change).err();
            }),
            SettingsOf::Program(id) => match self.store.global_settings() {
                Ok(global) => self.store.change_program_settings(id, |own| {
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

    /// Whether the system refuses the program `id` its notifications, as its app last said,
    /// whatever its answer on its page.
    pub(crate) fn notifications_refused(&self, id: &ProgramId) -> bool {
        self.notifications_refused.contains(id)
    }

    /// The system said whether it `refuses` the program `id` its notifications, as the program's app
    /// told it: as WaOS welcomed the app, as the system's question was answered, as the app or WaOS
    /// came to the front, or as a post was answered; the program's page says so from now on. Only
    /// a program's app of its own, as on macOS, hears the system say it.
    #[cfg(target_os = "macos")]
    pub(crate) fn system_permission_said(
        &mut self,
        id: &ProgramId,
        refuses: bool,
        cx: &mut Context<Library>,
    ) {
        let changed = if refuses {
            self.notifications_refused.insert(id.clone())
        } else {
            self.notifications_refused.remove(id)
        };
        if changed {
            tracing::info!(%id, refuses, "the system said of the program's notifications");
            cx.notify();
        }
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

    /// A program asks its user to change `switch`, and waits on `left` for the user to leave
    /// the page: its own page opens, under its name once installed and its id before; the
    /// global one for a program the engine runs under no id.
    fn settings_asked(&mut self, asked: SettingsOpened, cx: &mut Context<Library>) {
        let (of, name) = match asked.program.name().parse::<ProgramId>() {
            Ok(id) => {
                let name = self.installed_program(&id).map_or_else(
                    || id.to_string(),
                    |program| program.manifest.name().to_owned(),
                );
                (SettingsOf::Program(id), name)
            }
            Err(_) => (SettingsOf::Global, asked.program.name().to_owned()),
        };
        tracing::info!(name, switch = ?asked.switch, "a program asks its user to change a switch");
        self.show_page(LibraryPage::Settings(of), cx);
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
    let (watched, store) = (library.downgrade(), library.read(app).store.clone());
    app.spawn(async move |cx| {
        loop {
            store.settings_changed().await;
            let Some(library) = watched.upgrade() else {
                break;
            };
            cx.update(|app| library.update(app, |_, cx| cx.notify()));
        }
    });
}

/// A program waiting for the user to leave a settings page: what tells it so.
pub(crate) type LeftSettings = oneshot::Sender<()>;
