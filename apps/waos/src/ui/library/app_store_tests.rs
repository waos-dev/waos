//! The app store in the desktop's launcher, over a source the test serves: its three places,
//! Discover with every app the source lists, what it puts forward and what a search or a
//! category narrows them to; Updates with Update all; and Sources, which shows one source's apps
//! alone.

use waos_package::{Category, Index, ListedProgram};

use super::test_steps::{adds_source, in_launcher};
use super::words;
use crate::state::library::test_desktop::LibraryDesktop;
use crate::state::library::test_server::Server;

const TRIANGLE: &str = "dev.example.triangle";

/// A listing of `id` named `name` at 1.0.0, whose package nobody serves: a row to show.
fn listed(id: &str, name: &str) -> ListedProgram {
    ListedProgram::new(
        id.parse().unwrap(),
        name,
        "1.0.0".parse().unwrap(),
        format!("{id}-1.0.0.wapk"),
        [0; 32],
    )
}

/// A source named Example Apps, which lists Triangle, whose package it serves, Maps and Notes,
/// and puts Maps forward.
fn says_what_a_store_shows(index: &mut Index) {
    index.name = Some("Example Apps".to_owned());
    index.featured = vec!["dev.example.maps".parse().unwrap()];
    index.programs[0].summary = Some("One triangle, drawn well".to_owned());
    index.programs[0].category = Some(Category::GraphicsDesign);
    let mut maps = listed("dev.example.maps", "Maps");
    maps.summary = Some("Find your way".to_owned());
    maps.category = Some(Category::Travel);
    index.programs.push(maps);
    index.programs.push(listed("dev.example.notes", "Notes"));
}

/// The launcher on its app store, with the source of [`says_what_a_store_shows`] read.
fn on_the_store() -> (LibraryDesktop, waft::ui::ViewId, Server) {
    let (mut desktop, view) = in_launcher();
    let server = Server::start();
    let triangle = desktop.package(TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    server.publish_saying(TRIANGLE, &triangle, "1.0.0", says_what_a_store_shows);
    adds_source(&mut desktop, &server.url("waos/index.json"));
    desktop.update(|library, cx| {
        library.show_page(crate::state::library::LibraryPage::AppStore, cx);
    });
    (desktop, view, server)
}

/// How many labels say `text`.
fn times_shown(desktop: &LibraryDesktop, text: &str) -> usize {
    desktop
        .labels()
        .iter()
        .filter(|label| *label == text)
        .count()
}

#[test]
fn discover_shows_every_app_with_its_line_and_what_the_source_puts_forward_first() {
    let (desktop, _view, _server) = on_the_store();
    for shown in [
        "Triangle",
        "One triangle, drawn well",
        "Notes",
        "Find your way",
    ] {
        assert!(desktop.shows(shown), "{shown}: {:?}", desktop.labels());
    }
    assert!(desktop.shows(words::FEATURED) && desktop.shows(words::ALL_APPS));
    assert_eq!(
        times_shown(&desktop, "Maps"),
        2,
        "put forward, and among every app"
    );
    assert_eq!(times_shown(&desktop, "Triangle"), 1);
    let card = desktop.find("Maps").expect("its card");
    let heading = desktop.find(words::ALL_APPS).expect("the heading");
    assert!(card.dy() < heading.dy(), "put forward over every app");
}

#[test]
fn a_search_or_a_category_shows_what_was_asked_for_alone() {
    let (mut desktop, view, _server) = on_the_store();
    desktop.type_text(view, "way");
    assert_eq!(times_shown(&desktop, "Maps"), 1, "{:?}", desktop.labels());
    assert!(!desktop.shows("Triangle") && !desktop.shows(words::FEATURED));
    desktop.type_text(view, "");

    desktop.tap(view, words::category(Category::GraphicsDesign));
    assert!(desktop.shows("Triangle"), "{:?}", desktop.labels());
    assert!(!desktop.shows("Maps") && !desktop.shows("Notes"));
    desktop.tap(view, words::category(Category::GraphicsDesign));
    assert!(desktop.shows("Notes"), "every app again");
}

#[test]
fn an_app_got_from_discover_is_installed_and_says_so() {
    let (mut desktop, view, _server) = on_the_store();
    assert_eq!(
        times_shown(&desktop, words::GET),
        4,
        "three apps, one twice"
    );
    let row = desktop.find("Triangle").expect("its row").dy();
    desktop.tap_beside_label(view, "Triangle", words::GET);
    desktop.settle_until_code_is_ready();
    assert_eq!(desktop.read(|library| library.installed().len()), 1);
    assert!(desktop.shows(words::INSTALLED), "{:?}", desktop.labels());
    let after = desktop.find("Triangle").expect("its row").dy();
    assert_eq!(after, row, "on its row still");
}

#[test]
fn updates_lists_what_has_a_newer_version_and_updates_them_all() {
    let (mut desktop, view, server) = on_the_store();
    desktop.tap(view, words::UPDATES);
    assert!(desktop.shows(words::UP_TO_DATE), "{:?}", desktop.labels());

    desktop.tap(view, words::DISCOVER);
    desktop.tap_beside_label(view, "Triangle", words::GET);
    desktop.settle_until_code_is_ready();
    let newer = desktop.package(TRIANGLE, "Triangle", "1.1.0", b"\0asm");
    server.publish_saying(TRIANGLE, &newer, "1.1.0", says_what_a_store_shows);
    desktop.update(|library, cx| library.read_sources(cx));
    desktop.settle_until("the newer listing", |desktop| {
        desktop.read(|library| library.updates().len()) == 1
    });

    desktop.tap(view, words::UPDATES);
    assert!(desktop.shows("Triangle") && !desktop.shows("Maps"));
    assert!(desktop.shows(&words::update_detail("1.1.0", "1.0.0")));
    desktop.tap(view, words::UPDATE_ALL);
    desktop.settle_until("the update", |desktop| {
        desktop.read(|library| library.updates().is_empty())
    });
    desktop.settle_until_code_is_ready();
    assert!(desktop.shows(words::UP_TO_DATE), "{:?}", desktop.labels());
}

#[test]
fn sources_names_each_source_and_shows_its_apps_alone() {
    let (mut desktop, view, _server) = on_the_store();
    desktop.tap(view, words::SOURCES);
    assert!(desktop.shows("Example Apps"), "{:?}", desktop.labels());
    assert!(desktop.shows(words::ADD) && !desktop.shows("Triangle"));
    let said = desktop
        .labels()
        .into_iter()
        .find(|label| label.ends_with(&words::apps_listed(3)));
    assert!(said.is_some(), "how many it lists: {:?}", desktop.labels());

    desktop.tap(view, words::SHOW_APPS);
    assert!(
        desktop.shows("Maps") && desktop.shows("Example Apps"),
        "its apps, under its name: {:?}",
        desktop.labels()
    );
    desktop.tap(view, words::EVERY_SOURCE);
    assert!(desktop.shows("Maps") && !desktop.shows("Example Apps"));

    desktop.tap(view, words::SOURCES);
    desktop.tap(view, words::REMOVE_SOURCE);
    assert!(desktop.shows(words::NO_SOURCES), "{:?}", desktop.labels());
}
