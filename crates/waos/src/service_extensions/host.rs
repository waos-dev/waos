//! The code serving `wasdk:platform/service-extensions` to a program: its streams of calls, each
//! call a resource the program reads the name and parameters of and answers.
//!
//! A call enters the store's table as the program reads it, so one the program never reads stays
//! the extensions' and answers `failed` when its stream is dropped; answering takes it out, and so
//! does dropping it, which answers `failed`.

use futures::channel::mpsc;
use wasmtime::component::{Access, HasData, Linker, Resource, ResourceTable, StreamReader};

use crate::bindings::wasdk::platform::service_extensions::{
    self, ExtensionError, HostExtensionCall, HostWithStore,
};
use crate::stream::stream_of;

use super::{ExtensionCall, ProgramServiceExtensions};

/// What the service extensions interface reads and writes in a program's store.
pub(crate) struct ServiceExtensionsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) extensions: &'a ProgramServiceExtensions,
    /// Whether this instance hears the calls: the main instance does, and in a worker the stream
    /// ends at once.
    pub(crate) hears_calls: bool,
}

pub(crate) trait ServiceExtensionsCtxView: Send {
    fn service_extensions_ctx(&mut self) -> ServiceExtensionsCtx<'_>;
}

struct HasServiceExtensions;

impl HasData for HasServiceExtensions {
    type Data<'a> = ServiceExtensionsCtx<'a>;
}

pub(crate) fn add_to_linker<T: ServiceExtensionsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    service_extensions::add_to_linker::<_, HasServiceExtensions>(linker, T::service_extensions_ctx)
}

impl service_extensions::Host for ServiceExtensionsCtx<'_> {}

impl<T: ServiceExtensionsCtxView + 'static> HostWithStore<T> for HasServiceExtensions {
    fn calls(
        mut access: Access<T, Self>,
    ) -> wasmtime::Result<StreamReader<Resource<ExtensionCall>>> {
        let context = access.get();
        let calls = if context.hears_calls {
            context.extensions.calls()
        } else {
            mpsc::unbounded().1
        };
        stream_of(access, calls, |data: &mut T, call| {
            Ok(data.service_extensions_ctx().table.push(call)?)
        })
    }
}

impl HostExtensionCall for ServiceExtensionsCtx<'_> {
    fn name(&mut self, call: Resource<ExtensionCall>) -> wasmtime::Result<String> {
        Ok(self.table.get(&call)?.name().to_owned())
    }

    fn params(&mut self, call: Resource<ExtensionCall>) -> wasmtime::Result<Vec<(String, String)>> {
        Ok(self.table.get(&call)?.params().to_vec())
    }

    fn respond(
        &mut self,
        call: Resource<ExtensionCall>,
        answer: Result<String, ExtensionError>,
    ) -> wasmtime::Result<()> {
        self.table.delete(call)?.respond(answer);
        Ok(())
    }

    fn drop(&mut self, call: Resource<ExtensionCall>) -> wasmtime::Result<()> {
        self.table.delete(call)?;
        Ok(())
    }
}
