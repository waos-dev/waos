//! The app store in the desktop's launcher, over a source the test serves: its three places,
//! Discover with every app the source lists, what it puts forward and what a search or a
//! category narrows them to; Updates with Update all; and Sources, which shows one source's apps
//! alone.

use waos_package::{Category, Index, ListedProgram};

use super::test_steps::{adds_source, in_launcher, on_phone};
use super::words;
use crate::state::library::test_desktop::LibraryDesktop;
use crate::state::library::test_server::Server;

const TRIANGLE: &str = "dev.example.triangle";

/// The picture the source serves beside its index as Triangle's screenshot.
const TRIANGLE_SCREENSHOT: &str = "triangle-1.png";

/// Back, as Fluent's title bars draw it.
const BACK: &str = "\u{f15b}";

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
    let triangle = &mut index.programs[0];
    triangle.summary = Some("One triangle, drawn well".to_owned());
    triangle.category = Some(Category::GraphicsDesign);
    triangle.publisher = Some("Example".to_owned());
    triangle.released = Some("2026-11-23".parse().unwrap());
    triangle.description = Some("A triangle of one colour.\n\nNothing else.".to_owned());
    triangle.screenshots = vec![TRIANGLE_SCREENSHOT.to_owned()];
    let mut maps = listed("dev.example.maps", "Maps");
    maps.summary = Some("Find your way".to_owned());
    maps.category = Some(Category::Travel);
    index.programs.push(maps);
    index.programs.push(listed("dev.example.notes", "Notes"));
}

/// The launcher on its app store, with the source of [`says_what_a_store_shows`] read.
fn on_the_store() -> (LibraryDesktop, waft::ui::ViewId, Server) {
    on_the_store_of(in_launcher())
}

/// The library of `desktop`, shown in `view`, on its app store, with the source of
/// [`says_what_a_store_shows`] read.
fn on_the_store_of(
    (mut desktop, view): (LibraryDesktop, waft::ui::ViewId),
) -> (LibraryDesktop, waft::ui::ViewId, Server) {
    let server = Server::start();
    let triangle = desktop.package(TRIANGLE, "Triangle", "1.0.0", b"\0asm");
    server.publish_saying(TRIANGLE, &triangle, "1.0.0", says_what_a_store_shows);
    server.serve_beside_index(
        TRIANGLE_SCREENSHOT,
        include_bytes!("../../../assets/program-default.png"),
    );
    adds_source(&mut desktop, &server.source());
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
    assert!(
        desktop.shows(words::category(Category::GraphicsDesign))
            && !desktop.shows(words::category(Category::Travel)),
        "the category chosen is a token in the field, the chips gone: {:?}",
        desktop.labels()
    );
    desktop.press_backspace();
    assert!(desktop.shows("Notes"), "every app again");
    assert!(
        desktop.shows(words::category(Category::Travel)),
        "and the chips"
    );
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
    let (mut desktop, view, server) = on_the_store();
    desktop.tap(view, words::SOURCES);
    assert!(desktop.shows("Example Apps"), "{:?}", desktop.labels());
    assert!(desktop.shows(words::ADD) && !desktop.shows("Triangle"));
    assert!(
        desktop.shows(&words::apps_listed(3)),
        "how many it lists: {:?}",
        desktop.labels()
    );
    assert!(
        !desktop.shows(&server.source()),
        "its address shows once its row is opened: {:?}",
        desktop.labels()
    );

    desktop.tap(view, words::SHOW_APPS);
    assert!(
        desktop.shows("Maps") && desktop.shows("Example Apps"),
        "its apps, under its name: {:?}",
        desktop.labels()
    );
    desktop.press_backspace();
    assert!(
        desktop.shows("Maps") && !desktop.shows("Example Apps"),
        "the source's token taken back: {:?}",
        desktop.labels()
    );

    desktop.tap(view, words::SOURCES);
    desktop.tap(view, "Example Apps");
    assert!(
        desktop.shows(&server.source()),
        "the row opened on its address: {:?}",
        desktop.labels()
    );
    desktop.tap(view, words::REMOVE_SOURCE);
    assert!(desktop.shows(words::NO_SOURCES), "{:?}", desktop.labels());
}

#[test]
fn an_apps_row_opens_its_page_which_says_what_its_source_does_and_gets_it() {
    let (mut desktop, view, _server) = on_the_store();
    desktop.tap(view, "Triangle");
    for said in [
        "Triangle",
        "Example",
        "One triangle, drawn well",
        "A triangle of one colour.",
        "Nothing else.",
        words::VERSION,
        "1.0.0",
        words::RELEASED,
        "23 November 2026",
        words::CATEGORY,
        words::category(Category::GraphicsDesign),
        words::SOURCE,
        "Example Apps",
    ] {
        assert!(desktop.shows(said), "{said}: {:?}", desktop.labels());
    }
    assert!(!desktop.shows("Maps"), "the page of one app");
    desktop.settle_until("its screenshot", |desktop| {
        desktop.read(|library| {
            let source = &library.sources()[0].source;
            library.picture(source, TRIANGLE_SCREENSHOT).is_some()
        })
    });

    desktop.tap(view, words::GET);
    desktop.settle_until_code_is_ready();
    assert_eq!(desktop.read(|library| library.installed().len()), 1);
    assert!(
        desktop.shows(words::OPEN) && !desktop.shows(words::GET),
        "{:?}",
        desktop.labels()
    );

    desktop.tap(view, BACK);
    assert!(
        desktop.shows(words::FEATURED) && desktop.shows("Maps"),
        "back on the store: {:?}",
        desktop.labels()
    );
    desktop.type_text(view, "way");
    assert_eq!(
        times_shown(&desktop, "Maps"),
        1,
        "the field has the keyboard"
    );
}

#[test]
fn on_a_phone_an_apps_row_opens_its_page_which_gets_it_and_goes_back_to_the_store() {
    let (mut phone, view, _server) = on_the_store_of(on_phone());
    phone.tap(view, "Triangle");
    for said in [
        "Triangle",
        "Example",
        "One triangle, drawn well",
        "A triangle of one colour.",
        words::VERSION,
        "1.0.0",
        words::SOURCE,
        "Example Apps",
    ] {
        assert!(phone.shows(said), "{said}: {:?}", phone.labels());
    }
    assert!(!phone.shows("Maps"), "the page of one app");

    phone.tap(view, words::GET);
    phone.settle_until_code_is_ready();
    assert!(
        phone.shows(words::OPEN) && !phone.shows(words::GET),
        "{:?}",
        phone.labels()
    );

    phone.tap(view, words::GET_PROGRAMS);
    assert!(
        phone.shows("Maps") && phone.shows(words::ADD_SOURCE),
        "back on the store: {:?}",
        phone.labels()
    );
}
