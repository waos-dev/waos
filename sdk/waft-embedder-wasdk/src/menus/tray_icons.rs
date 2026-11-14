//! The program's tray icons, as Waft adds them: each the WIT's `tray-icon`, answered once the
//! platform has made it, removed when Waft's app drops it, with the menu the platform opens by
//! itself at it and what opens one there. What the user does to an icon, and chooses in its
//! attached menu, reaches the app as `EmbedderClient::tray_icon_event`, by the id given here.

use std::cell::Cell;
use std::rc::Rc;

use waft_embedder::{
    AttachedMenu, IconImage, MenuAtIcon, MenuEntry, Offset, PopupMenuFuture, TrayIcon,
    TrayIconError, TrayIconEvent, TrayIconFuture, TrayIconId, TrayIcons,
};
use wasdk::menus;

use super::entries::{flat_menu, platform_image};
use crate::events::{Event, StreamOpener};

/// The platform's tray.
pub(crate) struct WasdkTrayIcons {
    icons: Rc<menus::TrayIcons>,
    /// The id the next icon gets.
    next_id: Cell<TrayIconId>,
    /// Where each icon's streams go, for the loop to read.
    opener: StreamOpener,
}

impl WasdkTrayIcons {
    /// The platform's tray, where it has one, whose icons' streams go to `opener`.
    pub(crate) fn get(opener: StreamOpener) -> Option<WasdkTrayIcons> {
        Some(WasdkTrayIcons {
            icons: Rc::new(menus::get_tray_icons()?),
            next_id: Cell::new(1),
            opener,
        })
    }
}

impl TrayIcons for WasdkTrayIcons {
    /// The WIT's `add`, answered once the platform has made the icon; its events, and its attached
    /// menu's choices, are handed to the loop then.
    fn add(&self, image: &IconImage, tooltip: Option<&str>) -> TrayIconFuture {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let icons = Rc::clone(&self.icons);
        let opener = self.opener.clone();
        let image = platform_image(image);
        let tooltip = tooltip.map(str::to_owned);
        Box::pin(async move {
            match icons.add(image, tooltip).await {
                Ok(icon) => {
                    Ok(Box::new(WasdkTrayIcon::added(id, icon, &opener)) as Box<dyn TrayIcon>)
                }
                Err(menus::TrayIconError::Failed(reason)) => Err(TrayIconError::Failed(reason)),
            }
        })
    }
}

/// One of the program's tray icons: removed as it drops, its streams ending with it.
struct WasdkTrayIcon {
    id: TrayIconId,
    icon: menus::TrayIcon,
    attached_menu: Option<WasdkAttachedMenu>,
    menu_at_icon: Option<WasdkMenuAtIcon>,
}

impl WasdkTrayIcon {
    /// The platform's `icon`, which Waft's app knows as `id`, its events and its attached menu's
    /// choices handed to the loop through `opener`.
    fn added(id: TrayIconId, icon: menus::TrayIcon, opener: &StreamOpener) -> WasdkTrayIcon {
        opener.open(icon.events(), move |event| {
            Event::TrayIcon(id, framework_event(event))
        });
        let attached_menu = icon.attached_menu().map(|menu| {
            opener.open(menu.chosen(), move |choice| {
                Event::TrayIcon(id, TrayIconEvent::MenuSelected(choice))
            });
            WasdkAttachedMenu(menu)
        });
        let menu_at_icon = icon
            .menu_at_icon()
            .map(|menu| WasdkMenuAtIcon(Rc::new(menu)));
        WasdkTrayIcon {
            id,
            icon,
            attached_menu,
            menu_at_icon,
        }
    }
}

impl TrayIcon for WasdkTrayIcon {
    fn id(&self) -> TrayIconId {
        self.id
    }

    fn set_image(&self, image: &IconImage) {
        self.icon.set_image(&platform_image(image));
    }

    fn set_tooltip(&self, tooltip: Option<&str>) {
        self.icon.set_tooltip(tooltip);
    }

    fn attached_menu(&self) -> Option<&dyn AttachedMenu> {
        self.attached_menu
            .as_ref()
            .map(|menu| menu as &dyn AttachedMenu)
    }

    fn menu_at_icon(&self) -> Option<&dyn MenuAtIcon> {
        self.menu_at_icon
            .as_ref()
            .map(|menu| menu as &dyn MenuAtIcon)
    }
}

/// The menu the platform opens by itself at an icon.
struct WasdkAttachedMenu(menus::AttachedMenu);

impl AttachedMenu for WasdkAttachedMenu {
    fn set_menu(&self, menu: &[MenuEntry]) {
        self.0.set(&flat_menu(menu));
    }
}

/// What opens a menu at an icon.
struct WasdkMenuAtIcon(Rc<menus::MenuAtIcon>);

impl MenuAtIcon for WasdkMenuAtIcon {
    fn show(&self, menu: &[MenuEntry]) -> PopupMenuFuture {
        let at_icon = Rc::clone(&self.0);
        let entries = flat_menu(menu);
        Box::pin(async move { at_icon.show(entries).await })
    }
}

/// What the user did to an icon, as Waft names it; a position is in the screens' logical points
/// on both sides.
fn framework_event(event: menus::TrayIconEvent) -> TrayIconEvent {
    let at = |point: wasdk::windows::Point| Offset::new(point.x, point.y);
    match event {
        menus::TrayIconEvent::Activated(point) => TrayIconEvent::Activated {
            position: at(point),
        },
        menus::TrayIconEvent::ContextMenuRequested(point) => TrayIconEvent::ContextMenuRequested {
            position: at(point),
        },
        menus::TrayIconEvent::SecondaryActivated(point) => TrayIconEvent::SecondaryActivated {
            position: at(point),
        },
        menus::TrayIconEvent::DoubleClicked => TrayIconEvent::DoubleClicked,
        menus::TrayIconEvent::Scrolled(delta) => TrayIconEvent::Scrolled { delta },
    }
}
