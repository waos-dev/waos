//! The code serving `wasdk:platform/screens` to a program: the list, its changes, and a screen's
//! getters.
//!
//! A handle on a screen holds the screen as it was when the handle was made, and answers the
//! system's current value while the system has the screen: a program told a screen changed
//! reads it again through the handle it has, and one told a screen was removed goes on reading
//! what it was.

use wasmtime::component::{Access, HasData, Linker, Resource, ResourceTable, StreamReader};

use crate::bindings::wasdk::platform::screens::{
    self, HostScreen, HostWithStore, PixelSize, Rect, ScreenId,
};
use crate::stream::stream_of;

use super::{Screen, ScreenChange, Screens};

/// What the screens interface reads in a program's store.
pub(crate) struct ScreensCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) screens: &'a Screens,
}

pub(crate) trait ScreensCtxView: Send {
    fn screens_ctx(&mut self) -> ScreensCtx<'_>;
}

struct HasScreens;

impl HasData for HasScreens {
    type Data<'a> = ScreensCtx<'a>;
}

pub(crate) fn add_to_linker<T: ScreensCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    screens::add_to_linker::<_, HasScreens>(linker, T::screens_ctx)
}

impl ScreensCtx<'_> {
    /// The screen behind `handle` as the system has it now, or as it was when the system no
    /// longer has it.
    fn current(&mut self, handle: &Resource<Screen>) -> wasmtime::Result<Screen> {
        let known = self.table.get_mut(handle)?;
        if let Some(now) = self.screens.find(known.id) {
            *known = now;
        }
        Ok(known.clone())
    }
}

impl screens::Host for ScreensCtx<'_> {
    fn screens(&mut self) -> wasmtime::Result<Vec<Resource<Screen>>> {
        self.screens
            .list()
            .into_iter()
            .map(|screen| Ok(self.table.push(screen)?))
            .collect()
    }
}

impl<T: ScreensCtxView + 'static> HostWithStore<T> for HasScreens {
    fn screen_changes(
        mut access: Access<T, Self>,
    ) -> wasmtime::Result<StreamReader<screens::ScreenChange>> {
        let changes = access.get().screens.changes();
        stream_of(access, changes, |data: &mut T, change| {
            Ok(match change {
                ScreenChange::Added(screen) => {
                    screens::ScreenChange::Added(data.screens_ctx().table.push(screen)?)
                }
                ScreenChange::Removed(id) => screens::ScreenChange::Removed(id),
                ScreenChange::Changed(id) => screens::ScreenChange::Changed(id),
            })
        })
    }
}

impl HostScreen for ScreensCtx<'_> {
    fn id(&mut self, screen: Resource<Screen>) -> wasmtime::Result<ScreenId> {
        Ok(self.current(&screen)?.id)
    }

    fn name(&mut self, screen: Resource<Screen>) -> wasmtime::Result<String> {
        Ok(self.current(&screen)?.name)
    }

    fn size(&mut self, screen: Resource<Screen>) -> wasmtime::Result<PixelSize> {
        Ok(self.current(&screen)?.size)
    }

    fn scale(&mut self, screen: Resource<Screen>) -> wasmtime::Result<f64> {
        Ok(self.current(&screen)?.scale)
    }

    fn refresh_rate(&mut self, screen: Resource<Screen>) -> wasmtime::Result<u32> {
        Ok(self.current(&screen)?.refresh_rate)
    }

    fn frame(&mut self, screen: Resource<Screen>) -> wasmtime::Result<Rect> {
        Ok(self.current(&screen)?.frame)
    }

    fn visible_frame(&mut self, screen: Resource<Screen>) -> wasmtime::Result<Rect> {
        Ok(self.current(&screen)?.visible_frame)
    }

    fn drop(&mut self, screen: Resource<Screen>) -> wasmtime::Result<()> {
        self.table.delete(screen)?;
        Ok(())
    }
}
