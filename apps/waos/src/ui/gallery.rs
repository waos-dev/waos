//! The library as it looks, page by page, in the launcher and in a phone's window: for a person
//! changing how it looks to see every page after each change, without opening WaOS. Each scene
//! is the library's view captured (`capture_image`) and read back; with `WAOS_GALLERY_DIR` set,
//! it is written there as a PNG named for its layout, its place in the walk and what it shows:
//!
//! ```text
//! WAOS_GALLERY_DIR=target/gallery cargo test -p waos-app --lib gallery
//! ```
//!
//! Without it the scenes are still read back, so a page that stops painting fails here. Text is
//! in the fonts of the machine the test runs on; what is left unpainted reads back clear.

use std::path::Path;

use futures::executor::block_on;
use waft::ui::ViewId;
use waft_test::encode_png;
use waft_widgets::testing::{capture_image, find};
use waft_widgets::{View, downcast_widget};

use crate::logic::app_store::AppStorePlace;
use crate::logic::permissions::Permission;
use crate::logic::setting_changes::{DomainsKind, SettingChange, SettingsOf};
use crate::state::library::test_desktop::{DesktopKind, LibraryDesktop};
use crate::state::library::test_server::Server;
use crate::state::library::{AppDetail, LibraryPage, SettingsPage};
use waos_package::{Category, Index, ListedProgram};
use waos_store::{Answer, Question};

use crate::ui::library::test_steps::{
    adds_source, in_launcher_of, installs, installs_showing, on_phone, started_yesterday,
};

/// What a built-in app ships with: the category it is filed under, and its icon's PNG.
type BuiltIn = (Category, &'static [u8]);

/// The programs the library has installed, as a user's might be, the built-in apps with the
/// categories they are filed under and the icons they ship with, so the pictures show apps as
/// their authors make them.
const INSTALLED: [(&str, &str, Option<BuiltIn>); 5] = [
    (
        "dev.waos.notes",
        "Notes",
        Some((
            Category::Productivity,
            include_bytes!("../../../../builtin-apps/notes/icon.png"),
        )),
    ),
    (
        "dev.waos.camera",
        "Camera",
        Some((
            Category::Photography,
            include_bytes!("../../../../builtin-apps/camera/icon.png"),
        )),
    ),
    (
        "dev.waos.videoplayer",
        "Video Player",
        Some((
            Category::Video,
            include_bytes!("../../../../builtin-apps/video-player/icon.png"),
        )),
    ),
    (
        "dev.waos.inspector",
        "Inspector",
        Some((
            Category::DeveloperTools,
            include_bytes!("../../../../builtin-apps/inspector/icon.png"),
        )),
    ),
    ("dev.waos.zed", "Zed", None),
];

/// More programs, enough to fill the home past one row.
const MORE: [(&str, &str); 9] = [
    ("dev.example.books", "Books"),
    ("dev.example.calculator", "Calculator"),
    ("dev.example.calendar", "Calendar"),
    ("dev.example.maps", "Maps"),
    ("dev.example.music", "Music"),
    ("dev.example.photos", "Photos"),
    ("dev.example.reminders", "Reminders"),
    ("dev.example.terminal", "Terminal"),
    ("dev.example.weather", "Weather"),
];

/// The program whose package the gallery's source serves, not installed.
const LISTED: &str = "dev.example.triangle";

/// An app as the gallery's source lists it beside [`LISTED`]: its id, name, version, what it
/// is in a line, its category, and the file of its icon where the source serves one.
type ListedApp = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Category,
    Option<&'static str>,
);

/// What the gallery's source lists: the built-in apps, two of them newer than the ones
/// installed, and apps nobody has installed.
const SOURCE_LISTS: [ListedApp; 7] = [
    (
        "dev.waos.notes",
        "Notes",
        "1.2.0",
        "Notes, and a text editor for files anywhere",
        Category::Productivity,
        Some("notes.png"),
    ),
    (
        "dev.waos.camera",
        "Camera",
        "1.0.0",
        "A live camera with looks, and the photos you take",
        Category::Photography,
        Some("camera.png"),
    ),
    (
        "dev.waos.videoplayer",
        "Video Player",
        "1.1.0",
        "A video player with a library of your videos",
        Category::Video,
        Some("video-player.png"),
    ),
    (
        "dev.waos.inspector",
        "Inspector",
        "1.0.0",
        "See what each app has used, and what it is doing now",
        Category::DeveloperTools,
        Some("inspector.png"),
    ),
    (
        "dev.example.maps",
        "Maps",
        "2.4.0",
        "Find your way, and what is on it",
        Category::Travel,
        None,
    ),
    (
        "dev.example.music",
        "Music",
        "1.3.1",
        "Your songs, wherever they are kept",
        Category::Music,
        None,
    ),
    (
        "dev.example.weather",
        "Weather",
        "1.0.4",
        "Today's sky, and the week's",
        Category::Weather,
        None,
    ),
];

/// A point of the launcher's card with no icon under it: in its search row.
const OFF_EVERY_ICON: waft::Offset = waft::Offset::new(600.0, 30.0);

/// The store's places as the gallery draws them, each with its scene's name.
const STORE_SCENES: [(AppStorePlace, &str); 3] = [
    (AppStorePlace::Discover, "store-discover"),
    (AppStorePlace::Updates, "store-updates"),
    (AppStorePlace::Sources, "store-sources"),
];

/// The files the gallery's source serves as the screenshots of the first app it lists, and
/// what it says of that app at length.
const SCREENSHOTS: [&str; 2] = ["notes-1.png", "notes-2.png"];
const DESCRIPTION: &str = "Write a note, and find it again: every note is a file in a folder you \
    choose, so other apps read them too.\n\nNotes opens any text file, wherever it is, and keeps \
    your place in each.";

/// The file the gallery's source serves as its own icon.
const SOURCE_ICON: &str = "example-apps.png";

/// What the gallery's source puts forward.
const SOURCE_FEATURES: [&str; 2] = ["dev.waos.notes", "dev.example.maps"];

#[test]
fn the_library_in_the_launcher() {
    // With preferences of its own, as the app has, so WaOS's own settings have a page to draw.
    let as_the_app = DesktopKind {
        own_preferences: true,
        ..DesktopKind::default()
    };
    walks_the_library("launcher", in_launcher_of(as_the_app));
}

#[test]
fn the_library_in_a_phones_window() {
    walks_the_library("phone", on_phone());
}

/// Draws the library of `desktop`, shown in `view`, from empty to installed programs, getting
/// programs and the settings.
fn walks_the_library(layout: &str, (mut desktop, view): (LibraryDesktop, ViewId)) {
    let mut scenes = Scenes::new(layout, view);
    scenes.draw(&mut desktop, "empty");

    let (first, first_name, first_built_in) = INSTALLED[0];
    let (first_category, first_icon) = first_built_in.expect("the first is a built-in app");
    let package = desktop.package_showing(first, first_name, Some(first_category), first_icon);
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    scenes.draw(&mut desktop, "install-question");
    desktop.update(|library, cx| library.answer(true, cx));
    desktop.settle_until_code_is_ready();
    for (id, name, built_in) in &INSTALLED[1..] {
        match built_in {
            Some((category, icon)) => {
                installs_showing(&mut desktop, id, name, Some(*category), icon);
            }
            None => installs(&mut desktop, id, name, b"\0asm"),
        }
    }
    let home_drawn = scenes.draw(&mut desktop, "programs");

    let server = Server::start();
    let listed = desktop.package(LISTED, "Triangle", "1.0.0", b"\0asm");
    server.publish_saying(LISTED, &listed, "1.0.0", says_what_a_store_shows);
    for (_, _, built_in) in INSTALLED {
        if let Some((category, icon)) = built_in {
            server.serve_beside_index(&icon_file_of(category), icon);
        }
    }
    server.serve_beside_index(SOURCE_ICON, include_bytes!("../../assets/icon.png"));
    // An app's screenshots are pictures of something on screen: the home, as drawn above.
    for screenshot in SCREENSHOTS {
        server.serve_beside_index(screenshot, &home_drawn);
    }
    adds_source(&mut desktop, &server.source());
    shows(&mut desktop, LibraryPage::AppStore);
    reads_the_icons_listed(&mut desktop);
    for (place, scene) in STORE_SCENES {
        desktop.update(|library, cx| library.show_app_store_place(place, cx));
        scenes.draw(&mut desktop, scene);
    }
    // A source's row opens on a press, which the desktop's layout alone has.
    if layout == "launcher" {
        desktop.tap(view, "Example Apps");
        scenes.draw(&mut desktop, "store-source-opened");
    }
    desktop.update(|library, cx| {
        let source = library.sources()[0].source.clone();
        library.show_app_store_source(Some(source), cx);
        library.show_app_store_category(Some(Category::Productivity), cx);
    });
    scenes.draw(&mut desktop, "store-one-source-one-category");
    desktop.update(|library, cx| library.reset_app_store(cx));

    let shown = SOURCE_LISTS[0].0.parse().expect("an app's id");
    shows(&mut desktop, LibraryPage::App(shown));
    desktop.settle_until("the app's screenshots", |desktop| {
        desktop.read(|library| {
            let source = &library.sources()[0].source;
            SCREENSHOTS
                .iter()
                .all(|screenshot| library.picture(source, screenshot).is_some())
        })
    });
    scenes.draw(&mut desktop, "store-app");

    shows(&mut desktop, LibraryPage::Settings(SettingsPage::Root));
    scenes.draw(&mut desktop, "settings");
    // WaOS's own settings have a page where this desktop has any.
    if desktop.read(|library| library.kept_in_dock().is_some()) {
        shows(&mut desktop, LibraryPage::Settings(SettingsPage::General));
        scenes.draw(&mut desktop, "settings-general");
    }
    shows(
        &mut desktop,
        LibraryPage::Settings(SettingsPage::Permissions),
    );
    scenes.draw(&mut desktop, "settings-permissions");
    // Zed has set its own network, so the kind's page has an app set otherwise to show.
    desktop.update(|library, cx| {
        let zed = SettingsOf::Program("dev.waos.zed".parse().expect("an app's id"));
        library.change_settings(&zed, SettingChange::Domains(DomainsKind::Only), cx);
    });
    shows(
        &mut desktop,
        LibraryPage::Settings(SettingsPage::Permission(Permission::Network)),
    );
    scenes.draw(&mut desktop, "settings-network");
    let program: waos_package::ProgramId = first.parse().expect("a program's id");
    // Its camera set otherwise, so its page says one kind is its own.
    desktop.update(|library, cx| {
        let of = SettingsOf::Program(program.clone());
        let denied = SettingChange::Answer(Question::Camera, Some(Answer::Denied));
        library.change_settings(&of, denied, cx);
    });
    shows(
        &mut desktop,
        LibraryPage::Settings(SettingsPage::App(program.clone())),
    );
    scenes.draw(&mut desktop, "program-settings");
    shows(
        &mut desktop,
        LibraryPage::Settings(SettingsPage::AppPermission(
            program.clone(),
            Permission::Camera,
        )),
    );
    scenes.draw(&mut desktop, "program-camera");
    shows(
        &mut desktop,
        LibraryPage::Settings(SettingsPage::AppDetail(program.clone(), AppDetail::Answers)),
    );
    scenes.draw(&mut desktop, "program-answers");

    shows(&mut desktop, LibraryPage::Home);
    for (id, name) in MORE {
        installs(&mut desktop, id, name, b"\0asm");
    }
    for (id, starts) in [
        ("dev.waos.zed", 9),
        ("dev.waos.notes", 4),
        ("dev.example.maps", 2),
    ] {
        started_yesterday(&mut desktop, id, starts);
    }
    scenes.draw(&mut desktop, "often-used");
    // An icon shows nothing for the pointer resting over it: the desktop's mouse on one, the
    // home is drawn as with the mouse off every icon.
    if layout == "launcher" {
        let on_an_icon = desktop.find("Maps").expect("an icon of the home");
        desktop.hover_at(view, on_an_icon);
        let under_the_pointer = scenes.picture(&mut desktop);
        desktop.hover_at(view, OFF_EVERY_ICON);
        assert!(
            scenes.picture(&mut desktop) == under_the_pointer,
            "an icon under the pointer is drawn as any other"
        );
    }

    desktop.update(|library, cx| library.show_category(Some(Category::Productivity), cx));
    scenes.draw(&mut desktop, "one-category");
}

/// The gallery's source says of itself what a store shows: its name, what it puts forward, and
/// of each app its line, its category and its icon.
fn says_what_a_store_shows(index: &mut Index) {
    index.name = Some("Example Apps".to_owned());
    index.icon = Some(SOURCE_ICON.to_owned());
    index.featured = SOURCE_FEATURES
        .map(|id| id.parse().expect("an app's id"))
        .to_vec();
    let triangle = &mut index.programs[0];
    triangle.summary = Some("One triangle, drawn well".to_owned());
    triangle.category = Some(Category::GraphicsDesign);
    for (id, name, version, summary, category, icon) in SOURCE_LISTS {
        let mut listed = ListedProgram::new(
            id.parse().expect("an app's id"),
            name,
            version.parse().expect("a version"),
            format!("{id}-{version}.wapk"),
            [0; 32],
        );
        listed.summary = Some(summary.to_owned());
        listed.category = Some(category);
        listed.publisher = Some("Example".to_owned());
        listed.icon = icon.map(str::to_owned);
        index.programs.push(listed);
    }
    let described = &mut index.programs[1];
    described.description = Some(DESCRIPTION.to_owned());
    described.released = Some("2026-11-23".parse().expect("a date"));
    described.screenshots = SCREENSHOTS.map(str::to_owned).to_vec();
}

/// The file the gallery's source serves the icon of the built-in app filed under `category` as.
fn icon_file_of(category: Category) -> String {
    let listed = SOURCE_LISTS
        .iter()
        .find(|(.., listed_under, icon)| *listed_under == category && icon.is_some());
    let (.., icon) = listed.expect("the source lists every built-in app with an icon");
    icon.expect("an icon's file").to_owned()
}

/// Waits until the library has read every icon its only source lists, its own among them, as
/// it does once the source is read.
fn reads_the_icons_listed(desktop: &mut LibraryDesktop) {
    desktop.settle_until("the icons the source lists", |desktop| {
        desktop.read(|library| {
            let source = &library.sources()[0].source;
            let mut icons = SOURCE_LISTS.iter().filter_map(|(.., icon)| *icon);
            icons.all(|icon| library.picture(source, icon).is_some())
                && library.source_logo(&library.sources()[0]).is_some()
        })
    });
}

/// The library goes to `page`.
fn shows(desktop: &mut LibraryDesktop, page: LibraryPage) {
    desktop.update(|library, cx| library.show_page(page, cx));
}

/// The scenes of one walk: drawn in its layout's view, numbered in the order they are drawn.
struct Scenes<'a> {
    layout: &'a str,
    view: ViewId,
    drawn: u32,
}

impl<'a> Scenes<'a> {
    fn new(layout: &'a str, view: ViewId) -> Scenes<'a> {
        Scenes {
            layout,
            view,
            drawn: 0,
        }
    }

    /// Reads back what the view shows once the desktop settles, written as `what` where
    /// `WAOS_GALLERY_DIR` says; the picture drawn, as a PNG.
    fn draw(&mut self, desktop: &mut LibraryDesktop, what: &str) -> Vec<u8> {
        let png = self.picture(desktop);
        self.drawn += 1;
        if let Some(folder) = std::env::var_os("WAOS_GALLERY_DIR") {
            std::fs::create_dir_all(&folder).expect("the gallery's folder is made");
            let name = format!("{}-{:02}-{what}.png", self.layout, self.drawn);
            std::fs::write(Path::new(&folder).join(name), &png).expect("the scene is written");
        }
        png
    }

    /// What the view shows once the desktop settles, read back, as a PNG.
    fn picture(&self, desktop: &mut LibraryDesktop) -> Vec<u8> {
        desktop.settle();
        let shown = self.view;
        let mut app = desktop.shell.app();
        let view = find::by_widget_predicate("the library's view", move |_, widget| {
            downcast_widget::<View>(widget).is_some_and(|view| view.view.id() == shown)
        })
        .evaluate_in(&mut app)[0];
        let image = capture_image(&mut app, view).expect("the test platform draws images");
        let pixels = block_on(app.platform().read_image_pixels(&image))
            .unwrap_or_else(|error| panic!("the view reads back: {error}"));
        encode_png(&pixels)
    }
}
