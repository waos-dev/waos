//! Waft's menus over `wasdk:platform/menus`, one to one: the menu bar is `get-menu-bar`'s, a menu
//! at a point of a view `get-context-menus`', and the tray icons `get-tray-icons`', each none
//! where the platform has none. A program has no Dock menu of its own (the menus proposal's
//! decision 5). A menu goes over as the WIT's flat list ([`entries`]), and a choice comes back by
//! the id Waft's app gave the item.
//!
//! What the user chooses in the menu bar and does to a tray icon reaches the loop on streams it
//! reads ([`events`](crate::events)): the menu bar's, opened as the loop starts, and each icon's,
//! handed to the loop as the icon is added. The loop reads each next item only once it has pushed
//! the one before into the client, so a frozen program the engine unfrozen for an item stays unfrozen
//! until the app has handled it.

mod entries;
mod tray_icons;

use std::rc::Rc;

use waft_embedder::{KeyData, MenuBar, MenuEntry, Offset, PopupMenuFuture, View};
use wasdk::menus;

use crate::views::WasdkView;
pub(crate) use entries::{flat_menu, platform_image};
pub(crate) use tray_icons::WasdkTrayIcons;

/// The program's menu bar.
pub(crate) struct WasdkMenuBar(menus::MenuBar);

impl WasdkMenuBar {
    /// The platform's menu bar, where it has one.
    pub(crate) fn get() -> Option<WasdkMenuBar> {
        menus::get_menu_bar().map(WasdkMenuBar)
    }
}

impl MenuBar for WasdkMenuBar {
    fn set_menus(&self, menus: &[MenuEntry]) {
        self.0.set(&flat_menu(menus));
    }

    /// Never asked here: the program answers each key as it comes, and the key's reply is where
    /// its host learns the app left it, then offering it to the menu bar itself.
    fn perform_key_equivalent(&self, _key: &KeyData) -> bool {
        false
    }
}

/// Menus opened at a point of one of the program's views.
pub(crate) struct WasdkContextMenus(Rc<menus::ContextMenus>);

impl WasdkContextMenus {
    /// The platform's context menus, where it has them.
    pub(crate) fn get() -> Option<WasdkContextMenus> {
        menus::get_context_menus().map(|menus| WasdkContextMenus(Rc::new(menus)))
    }

    /// Opens `menu` at `at`, in `view`'s logical pixels, which the WIT takes in its physical ones.
    pub(crate) fn show(
        &self,
        view: Rc<WasdkView>,
        at: Offset,
        menu: &[MenuEntry],
    ) -> PopupMenuFuture {
        let menus = Rc::clone(&self.0);
        let ratio = view.metrics().device_pixel_ratio;
        let entries = flat_menu(menu);
        Box::pin(async move {
            let (x, y) = (at.dx() * ratio, at.dy() * ratio);
            menus.show(view.platform_view(), x, y, entries).await
        })
    }
}
