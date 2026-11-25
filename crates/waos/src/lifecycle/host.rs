//! The code serving `wasdk:platform/lifecycle` to a program: its state and the changes, the
//! requests to end it, each a resource the program answers, and what the user opened with it.
//!
//! An exit request enters the store's table as the program reads it, so one the program never
//! reads stays the lifecycle's and answers exit when the stream is dropped; answering takes it
//! out, and so does dropping it, which answers exit. An open request's files are mounted as the
//! program reads it, and the request holds the program unfrozen until it reads again
//! (`stream_holding`).

use futures::StreamExt;
use futures::channel::mpsc;
use wasmtime::component::{Access, HasData, Linker, Resource, ResourceTable, StreamReader};

use crate::bindings::wasdk::platform::lifecycle::{
    self, AppExitResponse, HostExitRequest, HostWithStore, LifecycleState, OpenRequest,
};
use crate::files::Picks;
use crate::stream::{stream_holding, stream_of};

use super::{ExitRequest, Lifecycle};

/// What the lifecycle interface reads and writes in a program's store.
pub(crate) struct LifecycleCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) lifecycle: &'a Lifecycle,
    /// Where the files of what the user opens with the program are mounted.
    pub(crate) picks: &'a Picks,
    /// Whether this instance hears the platform's requests to end the program and what the user
    /// opens with it: the main instance does, and in a worker their streams end at once.
    pub(crate) hears_requests: bool,
}

pub(crate) trait LifecycleCtxView: Send {
    fn lifecycle_ctx(&mut self) -> LifecycleCtx<'_>;
}

struct HasLifecycle;

impl HasData for HasLifecycle {
    type Data<'a> = LifecycleCtx<'a>;
}

pub(crate) fn add_to_linker<T: LifecycleCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    lifecycle::add_to_linker::<_, HasLifecycle>(linker, T::lifecycle_ctx)
}

impl lifecycle::Host for LifecycleCtx<'_> {
    fn state(&mut self) -> wasmtime::Result<LifecycleState> {
        Ok(self.lifecycle.state())
    }
}

impl<T: LifecycleCtxView + 'static> HostWithStore<T> for HasLifecycle {
    fn state_changes(
        mut access: Access<T, Self>,
    ) -> wasmtime::Result<StreamReader<LifecycleState>> {
        let states = access.get().lifecycle.state_changes();
        stream_of(access, states, |_: &mut T, state| Ok(state))
    }

    fn exit_requests(
        mut access: Access<T, Self>,
    ) -> wasmtime::Result<StreamReader<Resource<ExitRequest>>> {
        let context = access.get();
        let requests = if context.hears_requests {
            context.lifecycle.exit_requests()
        } else {
            mpsc::unbounded().1
        };
        stream_of(access, requests, |data: &mut T, request| {
            Ok(data.lifecycle_ctx().table.push(request)?)
        })
    }

    fn open_requests(mut access: Access<T, Self>) -> wasmtime::Result<StreamReader<OpenRequest>> {
        let context = access.get();
        let picks = context.picks.clone();
        let requests = if context.hears_requests {
            context.lifecycle.open_requests()
        } else {
            mpsc::unbounded().1
        };
        let requests = requests.map(move |(request, unfreeze)| (request.mounted(&picks), unfreeze));
        stream_holding(access, requests)
    }
}

impl HostExitRequest for LifecycleCtx<'_> {
    fn respond(
        &mut self,
        request: Resource<ExitRequest>,
        response: AppExitResponse,
    ) -> wasmtime::Result<()> {
        self.table.delete(request)?.respond(response);
        Ok(())
    }

    fn drop(&mut self, request: Resource<ExitRequest>) -> wasmtime::Result<()> {
        self.table.delete(request)?;
        Ok(())
    }
}
