//! The code serving `wasdk:platform/system` to a program: the operating system, and what its
//! user set, with the changes.

use wasmtime::component::{Access, HasData, Linker, StreamReader};

use crate::bindings::wasdk::platform::system::{
    self, Brightness, HostWithStore, Locale, OperatingSystem, SystemChange,
};
use crate::stream::stream_of;
use wasmtime_wasi::p3::bindings::clocks::system_clock::Instant;

use super::{SystemSettings, operating_system};

/// What the system interface reads in a program's store.
pub(crate) struct SystemCtx<'a> {
    pub(crate) settings: &'a SystemSettings,
}

pub(crate) trait SystemCtxView: Send {
    fn system_ctx(&mut self) -> SystemCtx<'_>;
}

struct HasSystem;

impl HasData for HasSystem {
    type Data<'a> = SystemCtx<'a>;
}

pub(crate) fn add_to_linker<T: SystemCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    system::add_to_linker::<_, HasSystem>(linker, T::system_ctx)
}

impl system::Host for SystemCtx<'_> {
    fn os(&mut self) -> wasmtime::Result<OperatingSystem> {
        Ok(operating_system())
    }

    fn platform_brightness(&mut self) -> wasmtime::Result<Brightness> {
        Ok(self.settings.platform_brightness())
    }

    fn locales(&mut self) -> wasmtime::Result<Vec<Locale>> {
        Ok(self.settings.locales())
    }

    fn time_zone(&mut self) -> wasmtime::Result<Option<String>> {
        Ok(self.settings.time_zone())
    }

    fn utc_offset(&mut self, when: Instant) -> wasmtime::Result<Option<i64>> {
        Ok(self.settings.utc_offset(when))
    }

    fn time_zone_abbreviation(&mut self, when: Instant) -> wasmtime::Result<Option<String>> {
        Ok(self.settings.time_zone_abbreviation(when))
    }
}

impl<T: SystemCtxView + 'static> HostWithStore<T> for HasSystem {
    fn changes(mut access: Access<T, Self>) -> wasmtime::Result<StreamReader<SystemChange>> {
        let changes = access.get().settings.changes();
        stream_of(access, changes, |_: &mut T, change| Ok(change))
    }
}
