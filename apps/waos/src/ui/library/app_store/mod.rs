//! The app store, a page of its own behind the home's plus, with a way back. What the layouts
//! share is here, read from the library: Discover's apps, those the sources put forward and
//! those a search or a category narrows them to ([`discover`]); the apps with a newer version
//! ([`updates`]); an app's own page ([`app_page`]); each source as its row shows it
//! ([`source_rows`]); and what installs a package from a file the user picks, where the system
//! has a file picker ([`PackagePicker`]). The controls are each layout's.
//!
//! The invariant: an app's row offers exactly what the library offers for it (`logic::offers`),
//! and Installing… while the library installs it, whatever the offer, until its code is ready, as
//! the home says.

pub(crate) mod cupertino;
pub(crate) mod cupertino_app_page;
pub(crate) mod fluent;
pub(crate) mod fluent_app_page;

use std::sync::Arc;

use waft::ui::ViewId;
use waft::{App, BuildContext, Entity, View};
use waos_package::{Category, ProgramId};
use waos_store::Source;

use super::words;
use crate::logic::app_store::{CatalogApp, categories, looked_for};
use crate::logic::offers::Offer;
use crate::state::library::PackageOperation;
use crate::state::library::{Library, Listing, SourceListing, picks_packages};

/// What the page's Install from file shows the system's file picker for a package with: the
/// library, which takes what is picked, and the view the page is in, whose window the picker
/// shows on.
#[derive(Clone)]
pub(crate) struct PackagePicker {
    library: Entity<Library>,
    parent: Option<ViewId>,
}

impl PackagePicker {
    /// The picker for the page `context` builds, over `library`; none where the system has no
    /// file picker, and the page offers no Install from file.
    pub(crate) fn of(
        library: &Entity<Library>,
        app: &App,
        context: BuildContext,
    ) -> Option<PackagePicker> {
        picks_packages(app).then(|| PackagePicker {
            library: library.clone(),
            parent: View::maybe_id_of(app, context),
        })
    }

    /// Shows the picker; the package picked is asked about as one opened with WaOS.
    pub(crate) fn show(&self, app: &mut App) {
        let parent = self.parent;
        self.library
            .update(app, |library, cx| library.pick_package(parent, cx));
    }
}

/// What a listed program's row offers, or says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowAction {
    Get,
    Update,
    Installing,
    Installed,
}

/// One app as the store shows it, in a row or a card: its icon, its name, a line under it, and
/// what can be done with it.
#[derive(Clone, Debug)]
pub(crate) struct StoreApp {
    pub(crate) app: CatalogApp,
    /// The icon its source lists, once read; else its own where it is installed; none for one
    /// neither has, which shows the default icon.
    pub(crate) icon: Option<Arc<[u8]>>,
    /// What its source says it is, in a line; its version where the source says none, and the
    /// version installed where it updates one.
    pub(crate) line: String,
    pub(crate) action: RowAction,
}

/// What the store's first place shows: the apps the sources put forward, the categories to
/// narrow by, and the apps listed, those the user looks for where they search or chose a
/// category.
#[derive(Clone, Debug)]
pub(crate) struct Discover {
    /// None while the user searches or chose a category, where the place shows what was asked
    /// for alone.
    pub(crate) featured: Vec<StoreApp>,
    pub(crate) categories: Vec<Category>,
    pub(crate) apps: Vec<StoreApp>,
}

/// What `library`'s app store shows on its first place.
pub(crate) fn discover(library: &Library) -> Discover {
    let catalog = library.catalog();
    let (search, category) = (library.app_store_search(), library.app_store_category());
    let asked = !search.trim().is_empty() || category.is_some();
    let featured = if asked {
        Vec::new()
    } else {
        library.featured()
    };
    let shown = |apps: Vec<CatalogApp>| -> Vec<StoreApp> {
        apps.iter().map(|app| store_app(app, library)).collect()
    };
    Discover {
        featured: shown(featured),
        categories: categories(&catalog),
        apps: shown(looked_for(&catalog, search, category)),
    }
}

/// The apps `library`'s app store offers an update of.
pub(crate) fn updates(library: &Library) -> Vec<StoreApp> {
    library
        .updates()
        .iter()
        .map(|app| store_app(app, library))
        .collect()
}

fn store_app(app: &CatalogApp, library: &Library) -> StoreApp {
    let listed = &app.listed;
    let version = listed.version.to_string();
    let (line, action) = match &app.offer {
        Offer::Install => (listed.summary.clone().unwrap_or(version), RowAction::Get),
        Offer::Update { installed } => {
            (words::update_detail(&version, installed), RowAction::Update)
        }
        Offer::Installed => (
            listed.summary.clone().unwrap_or(version),
            RowAction::Installed,
        ),
    };
    let installing = matches!(
        library.operation(&listed.id),
        Some(PackageOperation::Downloading { .. } | PackageOperation::Installing)
    );
    let action = if installing {
        RowAction::Installing
    } else {
        action
    };
    let listed_icon = listed
        .icon
        .as_deref()
        .and_then(|icon| library.picture(&app.source, icon));
    StoreApp {
        app: app.clone(),
        icon: listed_icon.or_else(|| library.icon(&listed.id)).cloned(),
        line,
        action,
    }
}

/// An app's page in the store, as each layout shows it: what its source says of it.
#[derive(Clone, Debug)]
pub(crate) struct AppPage {
    /// The app as a row shows it: its icon, and what can be done with it.
    pub(crate) app: StoreApp,
    /// What it is, in a line.
    pub(crate) summary: Option<String>,
    pub(crate) publisher: Option<String>,
    /// Its screenshots in the order its source lists them, each once it is read.
    pub(crate) screenshots: Vec<Option<Arc<[u8]>>>,
    /// What it is, at length, a paragraph each.
    pub(crate) description: Vec<String>,
    /// What else is known of it, each under its label: its version, when that was released,
    /// its category and the source it is from.
    pub(crate) details: Vec<(&'static str, String)>,
}

/// The page of the app `id` in `library`'s app store; none where no source read lists it.
pub(crate) fn app_page(library: &Library, id: &ProgramId) -> Option<AppPage> {
    let app = library.listed_app(id)?;
    let listed = &app.listed;
    let screenshots = listed
        .screenshots
        .iter()
        .map(|screenshot| library.picture(&app.source, screenshot).cloned())
        .collect();
    let description = listed
        .description
        .iter()
        .flat_map(|description| description.split("\n\n"))
        .map(|paragraph| paragraph.trim().to_owned())
        .filter(|paragraph| !paragraph.is_empty())
        .collect();
    let version = match &app.offer {
        Offer::Update { installed } => words::update_detail(&listed.version.to_string(), installed),
        Offer::Install | Offer::Installed => listed.version.to_string(),
    };
    let released = listed
        .released
        .map(|released| released.strftime("%-d %B %Y").to_string());
    let source = library
        .sources()
        .iter()
        .find(|read| read.source == app.source)
        .map(source_name);
    let details = [
        (words::VERSION, Some(version)),
        (words::RELEASED, released),
        (
            words::CATEGORY,
            listed
                .category
                .map(|category| words::category(category).to_owned()),
        ),
        (words::SOURCE, source),
    ];
    Some(AppPage {
        summary: listed.summary.clone(),
        publisher: listed.publisher.clone(),
        screenshots,
        description,
        details: details
            .into_iter()
            .filter_map(|(label, value)| Some((label, value?)))
            .collect(),
        app: store_app(&app, library),
    })
}

/// A source as the store's third place shows it: its logo, what it calls itself and how many
/// apps it lists, or what it says while it has none to show; and its address, for whoever asks.
#[derive(Clone, Debug)]
pub(crate) struct SourceRow {
    pub(crate) source: Source,
    /// The picture that stands for it, once read; none for one that has none, which shows a
    /// plain mark.
    pub(crate) logo: Option<Arc<[u8]>>,
    pub(crate) name: String,
    /// Its address, as its user types it.
    pub(crate) address: String,
    /// How many apps it lists; or that it is being read, or why it could not be.
    pub(crate) lists: Result<usize, String>,
}

/// The sources `library` holds, as the store's third place shows them.
pub(crate) fn source_rows(library: &Library) -> Vec<SourceRow> {
    let row = |read: &SourceListing| SourceRow {
        source: read.source.clone(),
        logo: library.source_logo(read).cloned(),
        name: source_name(read),
        address: read.source.to_string(),
        lists: match &read.listing {
            Listing::Reading => Err(words::READING.to_owned()),
            Listing::Failed(why) => Err(words::unreadable(why)),
            Listing::Read(apps) => Ok(apps.len()),
        },
    };
    library.sources().iter().map(row).collect()
}

/// What the store calls the source `read`: what it calls itself, or its address where it says
/// nothing, or was not read.
pub(crate) fn source_name(read: &SourceListing) -> String {
    read.name.clone().unwrap_or_else(|| read.source.to_string())
}
