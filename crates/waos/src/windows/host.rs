//! The code serving `wasdk:platform/windows` to a program: a window's methods, its frame, whether
//! it is maximized or full screen, its changes and what a double-click on a title bar does from
//! the engine's copies and the rest through the shell, its size limits through the shell's, and the setters of a
//! `window-config`, each into the record `request-view` reads.

use std::time::Duration;

use wasmtime::component::{Access, HasData, Linker, Resource, ResourceTable, StreamReader};

use crate::bindings::wasdk::platform::windows::{
    self, HostSizeLimits, HostWindow, HostWindowConfig, HostWindowWithStore, MacosTitleBar, Point,
    Rect, Size, TitleBarAction, WindowBackground, WindowChange, WindowLevel,
};
use crate::stream::stream_of;

use super::{ProgramWindow, SizeLimits, WindowConfig};

/// What the windows interface reads and writes in a program's store.
pub(crate) struct WindowsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
}

pub(crate) trait WindowsCtxView: Send {
    fn windows_ctx(&mut self) -> WindowsCtx<'_>;
}

struct HasWindows;

impl HasData for HasWindows {
    type Data<'a> = WindowsCtx<'a>;
}

pub(crate) fn add_to_linker<T: WindowsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    windows::add_to_linker::<_, HasWindows>(linker, T::windows_ctx)
}

impl windows::Host for WindowsCtx<'_> {}

impl WindowsCtx<'_> {
    fn window(&self, window: &Resource<ProgramWindow>) -> wasmtime::Result<&ProgramWindow> {
        Ok(self.table.get(window)?)
    }

    fn config(&mut self, config: &Resource<WindowConfig>) -> wasmtime::Result<&mut WindowConfig> {
        Ok(self.table.get_mut(config)?)
    }
}

impl<T: WindowsCtxView + 'static> HostWindowWithStore<T> for HasWindows {
    fn changes(
        mut access: Access<T, Self>,
        window: Resource<ProgramWindow>,
    ) -> wasmtime::Result<StreamReader<WindowChange>> {
        let changes = access.get().window(&window)?.changes();
        stream_of(access, changes, |_: &mut T, change| Ok(change))
    }
}

impl HostWindow for WindowsCtx<'_> {
    fn frame(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<Rect> {
        Ok(self.window(&window)?.frame())
    }

    fn set_frame(
        &mut self,
        window: Resource<ProgramWindow>,
        frame: Rect,
        animate: Option<u64>,
    ) -> wasmtime::Result<()> {
        let animate = animate.map(Duration::from_nanos);
        self.window(&window)?.shell().set_frame(frame, animate);
        Ok(())
    }

    fn set_title(
        &mut self,
        window: Resource<ProgramWindow>,
        title: String,
    ) -> wasmtime::Result<()> {
        self.window(&window)?.set_title(title);
        Ok(())
    }

    fn show(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<()> {
        self.window(&window)?.shell().show();
        Ok(())
    }

    fn hide(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<()> {
        self.window(&window)?.shell().hide();
        Ok(())
    }

    fn close(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<()> {
        self.window(&window)?.shell().close();
        Ok(())
    }

    fn size_limits(
        &mut self,
        window: Resource<ProgramWindow>,
    ) -> wasmtime::Result<Option<Resource<SizeLimits>>> {
        let limits = self.window(&window)?.size_limits();
        Ok(limits.map(|limits| self.table.push(limits)).transpose()?)
    }

    fn drag(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<()> {
        self.window(&window)?.shell().drag();
        Ok(())
    }

    fn is_maximized(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<bool> {
        Ok(self.window(&window)?.is_maximized())
    }

    fn set_maximized(
        &mut self,
        window: Resource<ProgramWindow>,
        maximized: bool,
    ) -> wasmtime::Result<()> {
        self.window(&window)?.shell().set_maximized(maximized);
        Ok(())
    }

    fn set_minimized(
        &mut self,
        window: Resource<ProgramWindow>,
        minimized: bool,
    ) -> wasmtime::Result<()> {
        self.window(&window)?.shell().set_minimized(minimized);
        Ok(())
    }

    fn title_bar_double_click_action(
        &mut self,
        window: Resource<ProgramWindow>,
    ) -> wasmtime::Result<Option<TitleBarAction>> {
        Ok(self.window(&window)?.title_bar_double_click_action())
    }

    fn is_fullscreen(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<bool> {
        Ok(self.window(&window)?.is_fullscreen())
    }

    fn set_fullscreen(
        &mut self,
        window: Resource<ProgramWindow>,
        fullscreen: bool,
        screen: Option<u64>,
    ) -> wasmtime::Result<()> {
        self.window(&window)?
            .shell()
            .set_fullscreen(fullscreen, screen);
        Ok(())
    }

    fn drop(&mut self, window: Resource<ProgramWindow>) -> wasmtime::Result<()> {
        self.table.delete(window)?;
        Ok(())
    }
}

impl HostSizeLimits for WindowsCtx<'_> {
    fn set_min_size(
        &mut self,
        limits: Resource<SizeLimits>,
        size: Option<Size>,
    ) -> wasmtime::Result<()> {
        self.table.get(&limits)?.set_min_size(size);
        Ok(())
    }

    fn set_max_size(
        &mut self,
        limits: Resource<SizeLimits>,
        size: Option<Size>,
    ) -> wasmtime::Result<()> {
        self.table.get(&limits)?.set_max_size(size);
        Ok(())
    }

    fn drop(&mut self, limits: Resource<SizeLimits>) -> wasmtime::Result<()> {
        self.table.delete(limits)?;
        Ok(())
    }
}

impl HostWindowConfig for WindowsCtx<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<WindowConfig>> {
        Ok(self.table.push(WindowConfig::default())?)
    }

    fn set_title(&mut self, config: Resource<WindowConfig>, title: String) -> wasmtime::Result<()> {
        self.config(&config)?.title = title;
        Ok(())
    }

    fn set_size(&mut self, config: Resource<WindowConfig>, size: Size) -> wasmtime::Result<()> {
        self.config(&config)?.size = Some(size);
        Ok(())
    }

    fn set_position(
        &mut self,
        config: Resource<WindowConfig>,
        position: Point,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.position = Some(position);
        Ok(())
    }

    fn set_decorations(
        &mut self,
        config: Resource<WindowConfig>,
        decorations: bool,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.decorations = decorations;
        Ok(())
    }

    fn set_resizable(
        &mut self,
        config: Resource<WindowConfig>,
        resizable: bool,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.resizable = resizable;
        Ok(())
    }

    fn set_level(
        &mut self,
        config: Resource<WindowConfig>,
        level: WindowLevel,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.level = level;
        Ok(())
    }

    fn set_activating(
        &mut self,
        config: Resource<WindowConfig>,
        activating: bool,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.activating = activating;
        Ok(())
    }

    fn set_all_desktops(
        &mut self,
        config: Resource<WindowConfig>,
        all_desktops: bool,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.all_desktops = all_desktops;
        Ok(())
    }

    fn set_background(
        &mut self,
        config: Resource<WindowConfig>,
        background: WindowBackground,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.background = background;
        Ok(())
    }

    fn set_shadow(&mut self, config: Resource<WindowConfig>, shadow: bool) -> wasmtime::Result<()> {
        self.config(&config)?.shadow = shadow;
        Ok(())
    }

    fn set_visible(
        &mut self,
        config: Resource<WindowConfig>,
        visible: bool,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.visible = visible;
        Ok(())
    }

    fn set_macos_title_bar(
        &mut self,
        config: Resource<WindowConfig>,
        title_bar: MacosTitleBar,
    ) -> wasmtime::Result<()> {
        self.config(&config)?.macos_title_bar = title_bar;
        Ok(())
    }

    fn drop(&mut self, config: Resource<WindowConfig>) -> wasmtime::Result<()> {
        self.table.delete(config)?;
        Ok(())
    }
}
