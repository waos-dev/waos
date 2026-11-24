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
pub(crate) mod fluent_root;

use std::sync::Arc;

use waft::App;
use waos_package::ProgramId;

use crate::logic::permissions::Permission;
use crate::logic::setting_changes::SettingsOf;
use crate::state::library::Library;
use crate::ui::library::words;

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

/// Whether WaOS has settings of its own to show here: where apps open, where WaOS stays, or
/// the phone's files, each only where this system offers the choice.
pub(crate) fn waos_has_settings(library: &Library, app: &App) -> bool {
    library.programs_inside_waos().is_some()
        || library.kept_in_dock().is_some()
        || library.phone_files_reachable(app).is_some()
}
