//! Steps the library's tests take in the launcher and in a phone's window alike: showing the
//! library in either, installing a program, and adding a source.

use waft::IntoWidget;
use waft::ui::{View as _, ViewId};

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
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    desktop.update(|library, cx| library.answer(true, cx));
    desktop.settle_until_code_is_ready();
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
