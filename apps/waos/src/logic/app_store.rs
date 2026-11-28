//! The app store's catalog, made of what the sources list: each app once, whichever sources list
//! it, as Cydia, F-Droid and AltStore browse every source at once; the apps the sources put
//! forward; those with an update; and those a search or a category narrows them to. What each
//! layout of the store shows is read from here, so the desktop's and the phone's agree.
//!
//! The invariant: an app is in the catalog once, at the newest version any source lists, from
//! the first source that lists that version, so what the store offers for it is one thing.

use std::cmp::Ordering;

use waos_package::{Category, ListedProgram, ProgramId};
use waos_services::sources::Source;

use super::offers::Offer;

/// One of the app store's places.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum AppStorePlace {
    /// Every app the sources list, to browse and search.
    #[default]
    Discover,
    /// The installed apps a source lists a newer version of.
    Updates,
    /// The sources the user added.
    Sources,
}

/// A source as far as it was read: the apps it lists, with what the library offers for each,
/// and those it puts forward.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ListedSource<'a> {
    pub(crate) source: &'a Source,
    /// The ids of the apps it puts forward, the first the most.
    pub(crate) featured: &'a [ProgramId],
    pub(crate) apps: &'a [(ListedProgram, Offer)],
}

/// One app as the store shows it: its listing, the source that lists it, and what the library
/// offers to do with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CatalogApp {
    pub(crate) listed: ListedProgram,
    pub(crate) source: Source,
    pub(crate) offer: Offer,
}

/// Every app `sources` list, each once, at the newest version listed, from the first source
/// that lists that version; in the order of their names, whatever their case.
pub(crate) fn catalog(sources: &[ListedSource<'_>]) -> Vec<CatalogApp> {
    let mut apps: Vec<CatalogApp> = Vec::new();
    for source in sources {
        for (listed, offer) in source.apps {
            let app = CatalogApp {
                listed: listed.clone(),
                source: source.source.clone(),
                offer: offer.clone(),
            };
            match apps.iter_mut().find(|app| app.listed.id == listed.id) {
                Some(known) if is_newer(listed, &known.listed) => *known = app,
                Some(_) => {}
                None => apps.push(app),
            }
        }
    }
    apps.sort_by_cached_key(|app| (app.listed.name.to_lowercase(), app.listed.id.clone()));
    apps
}

/// Whether `listed` is a newer version than `known`, by semver's precedence.
fn is_newer(listed: &ListedProgram, known: &ListedProgram) -> bool {
    listed.version.cmp_precedence(&known.version) == Ordering::Greater
}

/// The apps of `catalog` that `sources` put forward: each source's in its own order, the
/// sources in the order given, an app two sources put forward once.
pub(crate) fn featured(sources: &[ListedSource<'_>], catalog: &[CatalogApp]) -> Vec<CatalogApp> {
    let mut featured: Vec<CatalogApp> = Vec::new();
    let put_forward = sources.iter().flat_map(|source| source.featured);
    for id in put_forward {
        let already = featured.iter().any(|app| &app.listed.id == id);
        if let Some(app) = catalog.iter().find(|app| &app.listed.id == id)
            && !already
        {
            featured.push(app.clone());
        }
    }
    featured
}

/// The apps of `catalog` installed at an older version than the one listed.
pub(crate) fn updates(catalog: &[CatalogApp]) -> Vec<CatalogApp> {
    catalog
        .iter()
        .filter(|app| matches!(app.offer, Offer::Update { .. }))
        .cloned()
        .collect()
}

/// The apps of `catalog` a user who typed `search` and chose `category` looks for: those with
/// what was typed in their name or their line, whatever the case of either, and filed under the
/// category; every app, where nothing is typed and none is chosen.
pub(crate) fn looked_for(
    catalog: &[CatalogApp],
    search: &str,
    category: Option<Category>,
) -> Vec<CatalogApp> {
    let typed = search.trim().to_lowercase();
    let says = |text: &str| text.to_lowercase().contains(&typed);
    catalog
        .iter()
        .filter(|app| category.is_none_or(|chosen| app.listed.category == Some(chosen)))
        .filter(|app| says(&app.listed.name) || app.listed.summary.as_deref().is_some_and(says))
        .cloned()
        .collect()
}

/// The categories the apps of `catalog` are filed under, each once, in the order of their
/// spellings: the ones the store offers to show alone.
pub(crate) fn categories(catalog: &[CatalogApp]) -> Vec<Category> {
    let mut filed: Vec<Category> = catalog
        .iter()
        .filter_map(|app| app.listed.category)
        .collect();
    filed.sort();
    filed.dedup();
    filed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(repository: &str) -> Source {
        Source::GitHub {
            owner: "example".to_owned(),
            repository: repository.to_owned(),
        }
    }

    fn listed(id: &str, name: &str, version: &str) -> ListedProgram {
        ListedProgram::new(
            id.parse().unwrap(),
            name,
            version.parse().unwrap(),
            format!("{id}-{version}.wapk"),
            [0; 32],
        )
    }

    fn get(id: &str, name: &str, version: &str) -> (ListedProgram, Offer) {
        (listed(id, name, version), Offer::Install)
    }

    fn names(apps: &[CatalogApp]) -> Vec<&str> {
        apps.iter().map(|app| app.listed.name.as_str()).collect()
    }

    #[test]
    fn an_app_two_sources_list_shows_once_at_the_newest_version_and_apps_go_by_name() {
        let (first, second) = (source("first"), source("second"));
        let first_apps = [
            get("dev.example.zed", "Zed", "1.0.0"),
            get("dev.example.notes", "notes", "1.0.0"),
        ];
        let second_apps = [
            get("dev.example.zed", "Zed", "1.2.0"),
            get("dev.example.maps", "Maps", "1.0.0"),
        ];
        let sources = [
            ListedSource {
                source: &first,
                featured: &[],
                apps: &first_apps,
            },
            ListedSource {
                source: &second,
                featured: &[],
                apps: &second_apps,
            },
        ];
        let catalog = catalog(&sources);
        assert_eq!(names(&catalog), ["Maps", "notes", "Zed"]);
        let zed = &catalog[2];
        assert_eq!(
            (zed.listed.version.to_string().as_str(), &zed.source),
            ("1.2.0", &second)
        );
    }

    #[test]
    fn two_sources_listing_one_version_show_the_first_sources() {
        let (first, second) = (source("first"), source("second"));
        let apps = [get("dev.example.zed", "Zed", "1.0.0")];
        let sources = [
            ListedSource {
                source: &first,
                featured: &[],
                apps: &apps,
            },
            ListedSource {
                source: &second,
                featured: &[],
                apps: &apps,
            },
        ];
        assert_eq!(catalog(&sources)[0].source, first);
    }

    #[test]
    fn what_the_sources_put_forward_comes_in_their_order_each_app_once() {
        let (first, second) = (source("first"), source("second"));
        let id = |id: &str| -> ProgramId { id.parse().unwrap() };
        let apps = [
            get("dev.example.zed", "Zed", "1.0.0"),
            get("dev.example.notes", "Notes", "1.0.0"),
            get("dev.example.maps", "Maps", "1.0.0"),
        ];
        let first_puts_forward = [id("dev.example.zed"), id("dev.example.gone")];
        let second_puts_forward = [id("dev.example.maps"), id("dev.example.zed")];
        let sources = [
            ListedSource {
                source: &first,
                featured: &first_puts_forward,
                apps: &apps,
            },
            ListedSource {
                source: &second,
                featured: &second_puts_forward,
                apps: &[],
            },
        ];
        let catalog = catalog(&sources);
        assert_eq!(names(&featured(&sources, &catalog)), ["Zed", "Maps"]);
    }

    #[test]
    fn the_updates_are_the_apps_installed_at_an_older_version() {
        let only = source("only");
        let apps = [
            get("dev.example.maps", "Maps", "1.0.0"),
            (
                listed("dev.example.zed", "Zed", "1.2.0"),
                Offer::Update {
                    installed: "1.0.0".to_owned(),
                },
            ),
            (
                listed("dev.example.notes", "Notes", "1.0.0"),
                Offer::Installed,
            ),
        ];
        let sources = [ListedSource {
            source: &only,
            featured: &[],
            apps: &apps,
        }];
        assert_eq!(names(&updates(&catalog(&sources))), ["Zed"]);
    }

    #[test]
    fn a_search_reads_names_and_lines_and_a_category_narrows_to_its_apps() {
        let only = source("only");
        let mut notes = listed("dev.example.notes", "Notes", "1.0.0");
        notes.summary = Some("A text editor for files anywhere".to_owned());
        notes.category = Some(Category::Productivity);
        let mut camera = listed("dev.example.camera", "Camera", "1.0.0");
        camera.category = Some(Category::Photography);
        let apps = [(notes, Offer::Install), (camera, Offer::Install)];
        let sources = [ListedSource {
            source: &only,
            featured: &[],
            apps: &apps,
        }];
        let catalog = catalog(&sources);
        assert_eq!(names(&looked_for(&catalog, " EDITOR ", None)), ["Notes"]);
        assert_eq!(names(&looked_for(&catalog, "cam", None)), ["Camera"]);
        assert_eq!(
            names(&looked_for(&catalog, "", Some(Category::Photography))),
            ["Camera"]
        );
        assert_eq!(names(&looked_for(&catalog, "", None)), ["Camera", "Notes"]);
        assert_eq!(
            categories(&catalog),
            [Category::Photography, Category::Productivity]
        );
    }
}
