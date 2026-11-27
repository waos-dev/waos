//! The program's menus in its app: its menu bar, which macOS shows while the app is in front, and
//! its context menus, each opened at a point of one of its views. WaOS forwards the bar whole, as
//! the program set it, and the window process shows it through its Waft host's, each choice of one
//! of the program's own items going back to WaOS by the id the program gave it; until WaOS forwards
//! one, the host shows its standard bar, as for a program that sets none. A context menu opens
//! through the host at the point WaOS names, in the physical pixels of the view in the window it
//! names, and the item chosen goes back once it closes.
//!
//! The items the platform provides are the host's to carry out: those that act on the app and its
//! windows (hide, minimize, zoom, full screen, bring all to front) act on the window process's own
//! windows, which are the program's, and Edit's reach the focused view, whose edit events already
//! cross. So do About, which shows macOS's standard panel from the program's bundle, and Quit,
//! which ends the app, its windows going with it while the program runs on in WaOS, until quitting
//! asks the program.
//!
//! The invariant a later editor will break: every context menu WaOS asks for is answered, with
//! none where no window shows its view or the host has no context menus, so the program's call
//! returns; and nothing waits on WaOS.

use std::rc::Rc;

use waft::ui::{Offset, PopupMenuFuture, ViewId};
use waft::{App, Entity};
use waos::bindings::wasdk::platform::menus::MenuEntry;
use waos::bindings::wasdk::platform::views::RequestId;
use waos_program_window::{ViewWindows, waft_menu};
use waos_window_messages::{ContextMenuId, FromWindow};

use crate::connection::Connection;
use crate::program_link::WaosProgram;

/// A context menu WaOS asks the window process to open.
pub(crate) struct AskedContextMenu {
    /// What its answer names.
    pub(crate) menu: ContextMenuId,
    /// The window of the view it opens at, by the request it was made for.
    pub(crate) window: RequestId,
    /// Where, in the view's physical pixels.
    pub(crate) at: (f64, f64),
    pub(crate) entries: Vec<MenuEntry>,
}

/// From now on, an item of the program's own chosen in the menu bar goes to WaOS through
/// `connection`, by its id.
pub(crate) fn send_menu_choices(app: &mut App, connection: Rc<Connection>) {
    app.platform_callbacks_mut().on_menu_selected = Some(Rc::new(move |_app, id| {
        connection.send(FromWindow::MenuBarChosen(id));
    }));
}

/// Shows `entries`, the program's menu bar whole, in place of the one before.
pub(crate) fn show_menu_bar(app: &App, entries: &[MenuEntry]) {
    let platform = app.platform();
    match platform.menu_bar() {
        Some(menu_bar) => menu_bar.set_menus(&waft_menu(entries)),
        None => tracing::warn!("Waft's platform has no menu bar to show the program's in"),
    }
}

/// Opens `asked` at its point of the view the window it names shows in `view_windows`, and answers
/// WaOS through `connection` once it closes; at once, with none, where no window shows that view.
pub(crate) fn open_context_menu(
    app: &mut App,
    view_windows: &Entity<ViewWindows<WaosProgram>>,
    connection: Rc<Connection>,
    asked: AskedContextMenu,
) {
    let showing = view_windows
        .read(app)
        .waft_view_of(app, |_program, view| view.window() == asked.window);
    let opened = showing.and_then(|(view, ratio)| {
        let (x, y) = asked.at;
        show_at(app, view, Offset::new(x / ratio, y / ratio), &asked.entries)
    });
    answer_when_closed(app, connection, asked.menu, opened);
}

/// `entries`, opened at `at`, in logical pixels from the top-left of Waft's view `view`; none
/// where the host has no context menus.
fn show_at(app: &App, view: ViewId, at: Offset, entries: &[MenuEntry]) -> Option<PopupMenuFuture> {
    let platform = app.platform();
    Some(platform.popup_menus()?.show(view, at, &waft_menu(entries)))
}

/// Answers WaOS, through `connection`, the item chosen in the context menu `menu` once `opened`
/// closes; at once with none when none opened.
fn answer_when_closed(
    app: &mut App,
    connection: Rc<Connection>,
    menu: ContextMenuId,
    opened: Option<PopupMenuFuture>,
) {
    let Some(opened) = opened else {
        connection.send(FromWindow::ContextMenuClosed { menu, chosen: None });
        return;
    };
    app.spawn(async move |_cx| {
        let chosen = opened.await;
        connection.send(FromWindow::ContextMenuClosed { menu, chosen });
    });
}

#[cfg(test)]
mod tests;
