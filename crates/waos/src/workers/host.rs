//! The code serving `wasdk:platform/workers` to an instance of a program: the program's workers,
//! where its code has any, starting one at an entry with the instance's inbox stream bridged to
//! the new instance, what it sends, once, and when it finished.
//!
//! A worker's handle is the [`Worker`] itself in the store's table: deleting it, as the program's
//! drop does, ends the worker, and so does the store's end, which drops its table.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::thread;

use wasmtime::component::{
    Access, Accessor, HasData, Linker, Resource, ResourceTable, StreamReader,
};

use super::{Worker, Workers, message_channel};
use crate::bindings::wasdk::platform::workers::{
    self, HostWorker, HostWorkerWithStore, HostWorkers, HostWorkersWithStore, WorkerError,
};
use crate::running::{ProgramInstances, start_worker};
use crate::stream::{Forwarded, stream_of};

/// What the workers interface reads and writes in an instance's store.
pub(crate) struct WorkersCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    /// What the program's instances are made from, a new worker's among them.
    pub(crate) instances: &'a Arc<ProgramInstances>,
}

pub(crate) trait WorkersCtxView: Send {
    fn workers_ctx(&mut self) -> WorkersCtx<'_>;
}

struct HasWorkers;

impl HasData for HasWorkers {
    type Data<'a> = WorkersCtx<'a>;
}

pub(crate) fn add_to_linker<T: WorkersCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    workers::add_to_linker::<_, HasWorkers>(linker, T::workers_ctx)
}

impl workers::Host for WorkersCtx<'_> {
    fn get_workers(&mut self) -> wasmtime::Result<Option<Resource<Workers>>> {
        if !self.instances.has_workers() {
            return Ok(None);
        }
        Ok(Some(self.table.push(Workers)?))
    }
}

impl HostWorkers for WorkersCtx<'_> {
    fn available_parallelism(&mut self, _workers: Resource<Workers>) -> wasmtime::Result<u32> {
        let threads = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        Ok(u32::try_from(threads).unwrap_or(u32::MAX))
    }

    fn drop(&mut self, workers: Resource<Workers>) -> wasmtime::Result<()> {
        self.table.delete(workers)?;
        Ok(())
    }
}

impl<T: WorkersCtxView + 'static> HostWorkersWithStore<T> for HasWorkers {
    /// What the program writes to `inbox` crosses to the new instance as it reads it; answers
    /// once the new instance's `run` has accepted the entry or refused it.
    async fn start(
        accessor: &Accessor<T, Self>,
        _workers: Resource<Workers>,
        entry: String,
        inbox: StreamReader<Vec<u8>>,
    ) -> wasmtime::Result<Result<Resource<Worker>, WorkerError>> {
        let starting = accessor.with(|mut access| -> wasmtime::Result<_> {
            let (to_worker, worker_inbox) = message_channel();
            inbox.pipe(&mut access, Forwarded::new(to_worker))?;
            let instances = Arc::clone(access.get().instances);
            Ok(start_worker(&instances, entry, worker_inbox))
        })?;
        let worker = match starting.await {
            Ok(worker) => worker,
            Err(refused) => return Ok(Err(refused)),
        };
        accessor.with(|mut access| Ok(Ok(access.get().table.push(worker)?)))
    }
}

impl HostWorker for WorkersCtx<'_> {
    /// Ends the worker where it stands.
    fn drop(&mut self, worker: Resource<Worker>) -> wasmtime::Result<()> {
        self.table.delete(worker)?;
        Ok(())
    }
}

impl<T: WorkersCtxView + 'static> HostWorkerWithStore<T> for HasWorkers {
    fn messages(
        mut access: Access<T, Self>,
        worker: Resource<Worker>,
    ) -> wasmtime::Result<Option<StreamReader<Vec<u8>>>> {
        let Some(messages) = access.get().table.get_mut(&worker)?.take_messages() else {
            return Ok(None);
        };
        Ok(Some(stream_of(access, messages, |_: &mut T, message| {
            Ok(message)
        })?))
    }

    async fn finished(
        accessor: &Accessor<T, Self>,
        worker: Resource<Worker>,
    ) -> wasmtime::Result<Result<(), String>> {
        let finished = accessor.with(|mut access| -> wasmtime::Result<_> {
            Ok(access.get().table.get(&worker)?.finished())
        })?;
        Ok(finished.await)
    }
}
