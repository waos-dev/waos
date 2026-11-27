//! The library's settings, in each layout's controls: the first page, which leads to WaOS's own
//! settings, to the permissions apps follow by default, and to each app's; and those pages. What they show is
//! the library's (`state::library::settings`), and what they keep of their own is only what is
//! being typed. What the layouts share is here: what the first page, the list of permissions
//! and a permission's page list.

pub(crate) mod cupertino;
pub(crate) mod cupertino_general;
pub(crate) mod cupertino_root;
pub(crate) mod fluent;
pub(crate) mod fluent_general;
pub(crate) mod fluent_page;
pub(crate) mod fluent_root;

use std::sync::Arc;

use waft::{App, Entity};
use waos_package::ProgramId;
use waos_store::Question;

use crate::logic::app_store::CatalogApp;
#[cfg(desktop)]
use crate::logic::developer_access::Listening;
#[cfg(desktop)]
use crate::logic::network_addresses::network_addresses;
use crate::logic::offers::Offer;
use crate::logic::permissions::Permission;
use crate::logic::setting_changes::{SettingChange, SettingsOf, ShownSettings};
use crate::state::SIMULATED_CAMERA;
use crate::state::library::{AppDetail, Library};
use crate::ui::library::app_store::source_name;
use crate::ui::library::words;
use waos::FileDialog;

/// An installed app as the settings' first page lists it.
#[derive(Clone, Debug)]
pub(crate) struct ListedApp {
    pub(crate) id: ProgramId,
    pub(crate) name: String,
    /// Its icon, a PNG; none to show the default one.
    pub(crate) icon: Option<Arc<[u8]>>,
}

/// The apps `library` has installed, as the settings' first page lists them: in the order of
/// their names, whatever their case, as the home shows them.
pub(crate) fn listed_apps(library: &Library) -> Vec<ListedApp> {
    let mut apps: Vec<ListedApp> = library
        .installed()
        .iter()
        .map(|program| {
            let id = program.manifest.id();
            ListedApp {
                id: id.clone(),
                name: program.name(),
                icon: library.icon(id).cloned(),
            }
        })
        .collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

/// An installed app as its settings page says it over its settings: what it is, whether it
/// runs, and what can be done with it now.
#[derive(Clone, Debug)]
pub(crate) struct AppInfo {
    pub(crate) name: String,
    /// Its icon, a PNG; none to show the default one.
    pub(crate) icon: Option<Arc<[u8]>>,
    /// Its version, and the source that lists it where one does.
    pub(crate) version: String,
    /// What it is doing now, where that is worth saying: being installed anew, or running.
    pub(crate) status: Option<&'static str>,
    pub(crate) actions: Vec<AppAction>,
}

/// What an app's settings page offers to do with the app.
#[derive(Clone, Debug)]
pub(crate) enum AppAction {
    Open,
    /// Installs the newer version a source lists, the app as that source lists it.
    Update(Box<CatalogApp>),
    /// Ends it, where the library closes the apps it opened.
    Close,
    /// Puts every permission it set otherwise back on its default.
    ResetPermissions,
    /// Asks whether to remove it.
    Remove,
}

impl AppAction {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            AppAction::Open => words::OPEN,
            AppAction::Update(_) => words::UPDATE,
            AppAction::Close => words::CLOSE,
            AppAction::ResetPermissions => words::RESET_PERMISSIONS,
            AppAction::Remove => words::ASK_TO_REMOVE,
        }
    }

    /// Whether the action is done to the app as a whole, rarely and not to be undone: it ends
    /// the app's page, where the others head it.
    pub(crate) fn ends_the_page(&self) -> bool {
        matches!(self, AppAction::ResetPermissions | AppAction::Remove)
    }

    /// Does the action to the app `id` through `library`.
    pub(crate) fn carry_out(&self, library: &Entity<Library>, id: &ProgramId, app: &mut App) {
        library.update(app, |library, cx| match self {
            AppAction::Open => library.open(id, cx),
            AppAction::Update(listed) => library.install_from(&listed.source, &listed.listed, cx),
            AppAction::Close => library.close(id, cx),
            AppAction::ResetPermissions => {
                let of = SettingsOf::Program(id.clone());
                for change in reset_permissions() {
                    library.change_settings(&of, change, cx);
                }
            }
            AppAction::Remove => library.ask_to_remove(id, cx),
        });
    }
}

/// The installed app `id` as its settings page says it; none once it is no longer installed.
pub(crate) fn app_info(library: &Library, id: &ProgramId) -> Option<AppInfo> {
    let program = library
        .installed()
        .iter()
        .find(|program| program.manifest.id() == id)?;
    let listed = library.listed_app(id);
    let source = listed.as_ref().and_then(|listed| {
        let read = library
            .sources()
            .iter()
            .find(|read| read.source == listed.source)?;
        Some(source_name(read))
    });
    let (operation, running) = (library.operation(id), library.is_running(id));
    let update = listed
        .filter(|listed| operation.is_none() && matches!(listed.offer, Offer::Update { .. }))
        .map(|listed| AppAction::Update(Box::new(listed)));
    let close = (running && library.closes_programs()).then_some(AppAction::Close);
    let reset = library
        .settings(&SettingsOf::Program(id.clone()))
        .is_ok_and(|shown| any_set_otherwise(&shown))
        .then_some(AppAction::ResetPermissions);
    let status = match operation {
        Some(operation) => Some(words::operation(operation)),
        None => running.then_some(words::RUNNING),
    };
    Some(AppInfo {
        name: program.name(),
        icon: library.icon(id).cloned(),
        version: words::version_from(&program.manifest.version().to_string(), source.as_deref()),
        status,
        actions: [
            update,
            Some(AppAction::Open),
            close,
            reset,
            Some(AppAction::Remove),
        ]
        .into_iter()
        .flatten()
        .collect(),
    })
}

/// Each kind of permission with what its default holds, as the list of kinds shows them; none
/// where the settings cannot be read.
pub(crate) fn permission_defaults(library: &Library) -> Vec<(Permission, String)> {
    let Ok(shown) = library.settings(&SettingsOf::Global) else {
        return Vec::new();
    };
    Permission::ALL
        .iter()
        .map(|permission| (*permission, words::permission_value(*permission, &shown)))
        .collect()
}

/// The apps that set `permission` otherwise than its default, each with what it set, as the
/// kind's page lists them.
pub(crate) fn set_otherwise(library: &Library, permission: Permission) -> Vec<(ListedApp, String)> {
    listed_apps(library)
        .into_iter()
        .filter_map(|listed| {
            let shown = library
                .settings(&SettingsOf::Program(listed.id.clone()))
                .ok()?;
            permission.set_its_own(&shown).then(|| {
                let value = words::permission_value(permission, &shown);
                (listed, value)
            })
        })
        .collect()
}

/// Each kind of permission as an app's page lists it: what the app holds, said with
/// "(default)" while it follows the default, as a browser says a site's permission.
pub(crate) fn app_permissions(shown: &ShownSettings) -> Vec<(Permission, String)> {
    Permission::ALL
        .iter()
        .map(|permission| {
            let value = words::permission_value(*permission, shown);
            let said = if permission.set_its_own(shown) {
                value
            } else {
                words::defaulted(&value)
            };
            (*permission, said)
        })
        .collect()
}

/// What is the app's alone, as its page lists it under its permissions, each with what it
/// holds in a word: its machines, its file pickers and its sound output always, its links and
/// roles where it was given any.
pub(crate) fn app_details(shown: &ShownSettings) -> Vec<(AppDetail, String)> {
    let set = &shown.answer_with;
    let simulated = FileDialog::ALL
        .iter()
        .any(|dialog| set.paths.file_dialog(*dialog).is_some());
    let pickers = if simulated {
        words::SIMULATED
    } else {
        words::ASK
    };
    let output = set
        .audio_output
        .as_ref()
        .map_or(words::ASK.to_owned(), |output| output.label.clone());
    let mut details = vec![
        (
            AppDetail::Machines,
            words::how_many(shown.machines.len(), "machine", "machines"),
        ),
        (AppDetail::FilePickers, pickers.to_owned()),
        (AppDetail::SoundOutput, output),
    ];
    if !shown.link_schemes.is_empty() {
        let links = words::how_many(shown.link_schemes.len(), "kind of link", "kinds of link");
        details.push((AppDetail::Links, links));
    }
    if !shown.roles.is_empty() {
        let roles = words::how_many(shown.roles.len(), "role", "roles");
        details.push((AppDetail::Roles, roles));
    }
    details
}

/// Whether any kind of permission of the app whose settings are `shown` is set otherwise than
/// its default, so the app's page offers to reset them.
pub(crate) fn any_set_otherwise(shown: &ShownSettings) -> bool {
    Permission::ALL
        .iter()
        .any(|permission| permission.set_its_own(shown))
}

/// The changes that put every kind of permission of an app back on its default.
pub(crate) fn reset_permissions() -> Vec<SettingChange> {
    Permission::ALL
        .iter()
        .flat_map(|permission| permission.covers())
        .map(|which| SettingChange::FollowGlobal(*which))
        .collect()
}

/// Whether WaOS has settings of its own to show here: where apps open, where WaOS stays, the
/// phone's files, or developer access, each only where this system offers the choice.
pub(crate) fn waos_has_settings(library: &Library, app: &App) -> bool {
    library.programs_inside_waos().is_some()
        || library.kept_in_dock().is_some()
        || library.phone_files_reachable(app).is_some()
        || developer_access_offered(library)
}

#[cfg(desktop)]
fn developer_access_offered(library: &Library) -> bool {
    library.developer_access_on().is_some()
}

#[cfg(mobile)]
fn developer_access_offered(_library: &Library) -> bool {
    false
}

/// What developer access says: what it does, then whether WaOS listens, as `listening` says.
#[cfg(desktop)]
pub(crate) fn developer_access_said(listening: Option<Listening>) -> String {
    match listening {
        Some(Listening::On { port }) => {
            words::developer_access_listening(port, &network_addresses())
        }
        Some(Listening::Failed(why)) => words::developer_access_failed(&why),
        Some(Listening::Not) | None => words::DEVELOPER_ACCESS_DETAIL.to_owned(),
    }
}

/// What a setting says of itself: `detail`, then what it says of the `default`, whichever are
/// said.
pub(crate) fn said_with(detail: Option<&str>, default: Option<&str>) -> Option<String> {
    match (detail, default) {
        (Some(detail), Some(default)) => Some(format!("{detail} {default}")),
        (Some(said), None) | (None, Some(said)) => Some(said.to_owned()),
        (None, None) => None,
    }
}

/// While the answer to `question` is simulated, what the device of WaOS's own is; for the
/// camera, where this system has none, that it does not.
pub(crate) fn simulated_detail(question: Question) -> Option<&'static str> {
    match question {
        Question::Camera if !SIMULATED_CAMERA => Some(words::NO_SIMULATED_CAMERA),
        Question::Camera => Some(words::SIMULATED_CAMERA_DETAIL),
        Question::Microphone => Some(words::SIMULATED_MICROPHONE_DETAIL),
        Question::Notifications => None,
    }
}
