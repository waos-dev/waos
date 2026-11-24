//! The library's settings, in each layout's controls: the first page, which leads to WaOS's own
//! settings, to those every app follows, and to each app's; and those pages. What they show is
//! the library's (`state::library::settings`), and what they keep of their own is only what is
//! being typed. What the layouts share is here: what the first page lists.

pub(crate) mod cupertino;
pub(crate) mod cupertino_root;
pub(crate) mod cupertino_waos;
pub(crate) mod fluent;
pub(crate) mod fluent_root;
pub(crate) mod fluent_waos;

use std::sync::Arc;

use waft::App;
use waos_package::ProgramId;

use crate::state::library::Library;

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

/// Whether WaOS has settings of its own to show here: where apps open, where WaOS stays, or
/// the phone's files, each only where this system offers the choice.
pub(crate) fn waos_has_settings(library: &Library, app: &App) -> bool {
    library.programs_inside_waos().is_some()
        || library.kept_in_dock().is_some()
        || library.phone_files_reachable(app).is_some()
}
