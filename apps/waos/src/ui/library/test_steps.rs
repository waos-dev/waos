//! Steps the library's tests take in the launcher and in a phone's window alike: showing the
//! library in either, installing a program, adding a source, and an app having been started.

use std::time::{Duration, SystemTime};

use waft::IntoWidget;
use waft::ui::{View as _, ViewId};
use waos::{AuditDay, AuditLog as _};
use waos_package::Category;

use super::LaidOutLibrary;
use crate::state::library::Listing;
use crate::state::library::test_desktop::{DesktopKind, LibraryDesktop};
use crate::state::phone_window::in_phone_window;
use crate::ui::launcher::tests::{Start, launcher_desktop, launcher_view};

/// The desktop with the library in its launcher, shown.
pub(crate) fn in_launcher() -> (LibraryDesktop, ViewId) {
    in_launcher_of(DesktopKind::default())
}

/// The desktop of `kind` with the library in its launcher, shown.
pub(crate) fn in_launcher_of(kind: DesktopKind) -> (LibraryDesktop, ViewId) {
    let (desktop, _launcher) = launcher_desktop(kind, Start::Shown);
    let view = launcher_view(&desktop);
    (desktop, view)
}

/// The desktop with the library in a window of a phone's size.
pub(crate) fn on_phone() -> (LibraryDesktop, ViewId) {
    on_phone_of(DesktopKind::default())
}

/// The desktop of `kind` with the library in a window of a phone's size.
pub(crate) fn on_phone_of(kind: DesktopKind) -> (LibraryDesktop, ViewId) {
    let show = |app: &mut waft::App, library: &_, recordings: &_, _view_windows: &_| {
        let page = LaidOutLibrary {
            library: Clone::clone(library),
            recordings: Clone::clone(recordings),
            questions_here: true,
        };
        (Some(in_phone_window(app, page.into_widget())), ())
    };
    let (desktop, ()) = LibraryDesktop::of(kind, Box::new(show));
    let view = desktop.windows()[0].test_view().id();
    (desktop, view)
}

/// The program `id`, named `name`, at 1.0.0 and made of `component`, installed once the user says
/// so, its code ready.
pub(crate) fn installs(desktop: &mut LibraryDesktop, id: &str, name: &str, component: &[u8]) {
    let package = desktop.package(id, name, "1.0.0", component);
    installs_package(desktop, package);
}

/// Installs `id` named `name`, filed under `category`, with the PNG `icon` as its icon.
pub(crate) fn installs_showing(
    desktop: &mut LibraryDesktop,
    id: &str,
    name: &str,
    category: Option<Category>,
    icon: &[u8],
) {
    let package = desktop.package_showing(id, name, category, icon);
    installs_package(desktop, package);
}

/// Installs `id` named `name`, filed under `category`.
pub(crate) fn installs_filed(
    desktop: &mut LibraryDesktop,
    id: &str,
    name: &str,
    category: Category,
) {
    let package = desktop.package_filed(id, name, category);
    installs_package(desktop, package);
}

fn installs_package(desktop: &mut LibraryDesktop, package: std::path::PathBuf) {
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    desktop.update(|library, cx| library.answer(true, cx));
    desktop.settle_until_operations_end();
}

/// The source at `index` added, once its listing is read.
pub(crate) fn adds_source(desktop: &mut LibraryDesktop, index: &str) {
    desktop.update(|library, cx| library.add_source(index, cx));
    desktop.settle_until("the source's listing", |desktop| {
        desktop.read(|library| {
            library
                .sources()
                .iter()
                .all(|read| !matches!(read.listing, Listing::Reading))
        })
    });
}

/// The app `id` was started `starts` times yesterday, as the engine's record of it says.
pub(crate) fn started_yesterday(desktop: &mut LibraryDesktop, id: &str, starts: u32) {
    const DAY: u64 = 24 * 60 * 60;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("after 1970")
        .as_secs();
    let yesterday = SystemTime::UNIX_EPOCH + Duration::from_secs((now / DAY - 1) * DAY);
    let mut day = AuditDay::starting(yesterday);
    day.starts = starts;
    desktop.activity.keep(id, &day);
    desktop.update(|library, cx| library.read_often_used(cx));
}
