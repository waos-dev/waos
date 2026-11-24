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

use crate::logic::setting_changes::SettingsOf;
use crate::state::library::LibraryPage;
use crate::state::library::test_desktop::LibraryDesktop;
use crate::state::library::test_server::Server;
use waos_package::Category;

use crate::ui::library::test_steps::{
    adds_source, in_launcher, installs, installs_showing, on_phone, started_yesterday,
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

/// The program a source lists, not installed.
const LISTED: &str = "dev.example.triangle";

#[test]
fn the_library_in_the_launcher() {
    walks_the_library("launcher", in_launcher());
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
    scenes.draw(&mut desktop, "programs");

    let server = Server::start();
    let listed = desktop.package(LISTED, "Triangle", "1.0.0", b"\0asm");
    server.publish(LISTED, &listed, "1.0.0");
    adds_source(&mut desktop, &server.url("waos/index.json"));
    shows(&mut desktop, LibraryPage::AppStore);
    scenes.draw(&mut desktop, "get-programs");

    shows(&mut desktop, LibraryPage::Settings(SettingsOf::Global));
    scenes.draw(&mut desktop, "settings");
    let program = first.parse().expect("a program's id");
    shows(
        &mut desktop,
        LibraryPage::Settings(SettingsOf::Program(program)),
    );
    scenes.draw(&mut desktop, "program-settings");

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

    desktop.update(|library, cx| library.show_category(Some(Category::Productivity), cx));
    scenes.draw(&mut desktop, "one-category");
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
    /// `WAOS_GALLERY_DIR` says.
    fn draw(&mut self, desktop: &mut LibraryDesktop, what: &str) {
        desktop.settle();
        self.drawn += 1;
        let shown = self.view;
        let mut app = desktop.shell.app();
        let view = find::by_widget_predicate("the library's view", move |_, widget| {
            downcast_widget::<View>(widget).is_some_and(|view| view.view.id() == shown)
        })
        .evaluate_in(&mut app)[0];
        let image = capture_image(&mut app, view).expect("the test platform draws images");
        let pixels = block_on(app.platform().read_image_pixels(&image))
            .unwrap_or_else(|error| panic!("{what} reads back: {error}"));
        if let Some(folder) = std::env::var_os("WAOS_GALLERY_DIR") {
            std::fs::create_dir_all(&folder).expect("the gallery's folder is made");
            let name = format!("{}-{:02}-{what}.png", self.layout, self.drawn);
            std::fs::write(Path::new(&folder).join(name), encode_png(&pixels))
                .expect("the scene is written");
        }
    }
}
