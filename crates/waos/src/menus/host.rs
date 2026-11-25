//! The code serving `wasdk:platform/menus` to a program: the shell's places, each a handle where
//! the shell has it; the program's menu bar, set and its choices read; a context menu opened at
//! one of its views; and its tray icons, with their events and menus.
//!
//! Every stream of choices or events holds the program unfrozen for the item it delivered last,
//! until the program reads again or drops it (`stream_holding`).

use wasmtime::component::{
    Access, Accessor, HasData, Linker, Resource, ResourceTable, StreamReader,
};

use super::{AttachedMenu, ContextMenus, MenuAtIcon, MenuBar, MenuPlaces, TrayIcon, TrayIcons};
use crate::bindings::wasdk::platform::menus::{
    self, HostAttachedMenu, HostAttachedMenuWithStore, HostContextMenus, HostContextMenusWithStore,
    HostMenuAtIcon, HostMenuAtIconWithStore, HostMenuBar, HostMenuBarWithStore, HostTrayIcon,
    HostTrayIconWithStore, HostTrayIcons, HostTrayIconsWithStore, Image, MenuEntry, TrayIconError,
    TrayIconEvent,
};
use crate::running::RunningProgram;
use crate::stream::stream_holding;
use crate::views::ProgramView;

/// What the menus interface reads and writes in a program's store.
pub(crate) struct MenusCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a RunningProgram,
    pub(crate) places: &'a MenuPlaces,
}

pub(crate) trait MenusCtxView: Send {
    fn menus_ctx(&mut self) -> MenusCtx<'_>;
}

struct HasMenus;

impl HasData for HasMenus {
    type Data<'a> = MenusCtx<'a>;
}

pub(crate) fn add_to_linker<T: MenusCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    menus::add_to_linker::<_, HasMenus>(linker, T::menus_ctx)
}

impl menus::Host for MenusCtx<'_> {
    fn get_menu_bar(&mut self) -> wasmtime::Result<Option<Resource<MenuBar>>> {
        let place = self.places.menu_bar.clone().map(MenuBar);
        Ok(place.map(|bar| self.table.push(bar)).transpose()?)
    }

    fn get_context_menus(&mut self) -> wasmtime::Result<Option<Resource<ContextMenus>>> {
        let place = self.places.context_menus.clone().map(ContextMenus);
        Ok(place.map(|menus| self.table.push(menus)).transpose()?)
    }

    fn get_tray_icons(&mut self) -> wasmtime::Result<Option<Resource<TrayIcons>>> {
        let place = self.places.tray_icons.clone().map(TrayIcons);
        Ok(place.map(|icons| self.table.push(icons)).transpose()?)
    }
}

impl HostMenuBar for MenusCtx<'_> {
    fn set(&mut self, bar: Resource<MenuBar>, entries: Vec<MenuEntry>) -> wasmtime::Result<()> {
        self.table.get(&bar)?.set(self.program, entries);
        Ok(())
    }

    fn drop(&mut self, bar: Resource<MenuBar>) -> wasmtime::Result<()> {
        self.table.delete(bar)?;
        Ok(())
    }
}

impl<T: MenusCtxView + 'static> HostMenuBarWithStore<T> for HasMenus {
    fn chosen(
        mut access: Access<T, Self>,
        _bar: Resource<MenuBar>,
    ) -> wasmtime::Result<StreamReader<u32>> {
        let choices = access.get().program.menus().choices();
        stream_holding(access, choices)
    }
}

impl HostContextMenus for MenusCtx<'_> {
    fn drop(&mut self, menus: Resource<ContextMenus>) -> wasmtime::Result<()> {
        self.table.delete(menus)?;
        Ok(())
    }
}

impl<T: MenusCtxView + 'static> HostContextMenusWithStore<T> for HasMenus {
    async fn show(
        accessor: &Accessor<T, Self>,
        menus: Resource<ContextMenus>,
        view: Resource<ProgramView>,
        x: f64,
        y: f64,
        entries: Vec<MenuEntry>,
    ) -> wasmtime::Result<Option<u32>> {
        let shown = accessor.with(|mut access| {
            let context = access.get();
            let view = context.table.get(&view)?.id();
            let menus = &context.table.get(&menus)?.0;
            Ok::<_, wasmtime::Error>(menus.show(context.program, view, x, y, &entries))
        })?;
        Ok(shown.await)
    }
}

impl HostTrayIcons for MenusCtx<'_> {
    fn drop(&mut self, icons: Resource<TrayIcons>) -> wasmtime::Result<()> {
        self.table.delete(icons)?;
        Ok(())
    }
}

impl<T: MenusCtxView + 'static> HostTrayIconsWithStore<T> for HasMenus {
    /// The icon enters the table once the shell has made it.
    async fn add(
        accessor: &Accessor<T, Self>,
        icons: Resource<TrayIcons>,
        image: Image,
        tooltip: Option<String>,
    ) -> wasmtime::Result<Result<Resource<TrayIcon>, TrayIconError>> {
        let adding = accessor.with(|mut access| {
            let context = access.get();
            let icons = context.table.get(&icons)?;
            Ok::<_, wasmtime::Error>(icons.add(context.program, &image, tooltip.as_deref()))
        })?;
        match adding.await {
            Ok(icon) => accessor.with(|mut access| Ok(Ok(access.get().table.push(icon)?))),
            Err(error) => Ok(Err(error)),
        }
    }
}

impl HostTrayIcon for MenusCtx<'_> {
    fn set_image(&mut self, icon: Resource<TrayIcon>, image: Image) -> wasmtime::Result<()> {
        self.table.get(&icon)?.set_image(&image);
        Ok(())
    }

    fn set_tooltip(
        &mut self,
        icon: Resource<TrayIcon>,
        tooltip: Option<String>,
    ) -> wasmtime::Result<()> {
        self.table.get(&icon)?.set_tooltip(tooltip.as_deref());
        Ok(())
    }

    fn attached_menu(
        &mut self,
        icon: Resource<TrayIcon>,
    ) -> wasmtime::Result<Option<Resource<AttachedMenu>>> {
        let menu = self.table.get(&icon)?.attached_menu();
        Ok(menu.map(|menu| self.table.push(menu)).transpose()?)
    }

    fn menu_at_icon(
        &mut self,
        icon: Resource<TrayIcon>,
    ) -> wasmtime::Result<Option<Resource<MenuAtIcon>>> {
        let menu = self.table.get(&icon)?.menu_at_icon();
        Ok(menu.map(|menu| self.table.push(menu)).transpose()?)
    }

    /// The icon is removed as its handle goes.
    fn drop(&mut self, icon: Resource<TrayIcon>) -> wasmtime::Result<()> {
        self.table.delete(icon)?;
        Ok(())
    }
}

impl<T: MenusCtxView + 'static> HostTrayIconWithStore<T> for HasMenus {
    fn events(
        mut access: Access<T, Self>,
        icon: Resource<TrayIcon>,
    ) -> wasmtime::Result<StreamReader<TrayIconEvent>> {
        let events = access.get().table.get(&icon)?.events();
        stream_holding(access, events)
    }
}

impl HostAttachedMenu for MenusCtx<'_> {
    fn set(
        &mut self,
        menu: Resource<AttachedMenu>,
        entries: Vec<MenuEntry>,
    ) -> wasmtime::Result<()> {
        self.table.get(&menu)?.set(&entries);
        Ok(())
    }

    fn drop(&mut self, menu: Resource<AttachedMenu>) -> wasmtime::Result<()> {
        self.table.delete(menu)?;
        Ok(())
    }
}

impl<T: MenusCtxView + 'static> HostAttachedMenuWithStore<T> for HasMenus {
    fn chosen(
        mut access: Access<T, Self>,
        menu: Resource<AttachedMenu>,
    ) -> wasmtime::Result<StreamReader<u32>> {
        let choices = access.get().table.get(&menu)?.chosen();
        stream_holding(access, choices)
    }
}

impl HostMenuAtIcon for MenusCtx<'_> {
    fn drop(&mut self, menu: Resource<MenuAtIcon>) -> wasmtime::Result<()> {
        self.table.delete(menu)?;
        Ok(())
    }
}

impl<T: MenusCtxView + 'static> HostMenuAtIconWithStore<T> for HasMenus {
    async fn show(
        accessor: &Accessor<T, Self>,
        menu: Resource<MenuAtIcon>,
        entries: Vec<MenuEntry>,
    ) -> wasmtime::Result<Option<u32>> {
        let shown = accessor.with(|mut access| {
            Ok::<_, wasmtime::Error>(access.get().table.get(&menu)?.show(&entries))
        })?;
        Ok(shown.await)
    }
}
