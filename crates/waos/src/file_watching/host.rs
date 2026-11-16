//! The code serving `wasdk:platform/file-watching` to a program: a folder it holds, by its
//! descriptor, watched, decided and held through the program's access, and its changes as a
//! stream of batches. The system is asked to watch it on
//! a blocking thread, not the program's: inotify watches each folder beneath a recursive watch
//! apart, which takes a while for a large one, while the program's other tasks go on.

use std::time::Duration;

use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable, StreamReader};
use wasmtime_wasi::runtime::spawn_blocking;

use super::watch;
use crate::access::{Asked, Holding, ProgramAccess};
use crate::bindings::wasdk::platform::file_watching::{self, Change, HostWithStore};
use crate::files::p3::bindings::wasi::filesystem::types::ErrorCode;
use crate::files::{Descriptor, HeldFolder};
use crate::stream::stream_of;

/// What the file-watching interface reads in a program's store: the descriptors it holds, and
/// where each watch is decided and held.
pub(crate) struct FileWatchingCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) access: &'a ProgramAccess,
}

pub(crate) trait FileWatchingCtxView: Send {
    fn file_watching_ctx(&mut self) -> FileWatchingCtx<'_>;
}

struct HasFileWatching;

impl HasData for HasFileWatching {
    type Data<'a> = FileWatchingCtx<'a>;
}

pub(crate) fn add_to_linker<T: FileWatchingCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    file_watching::add_to_linker::<_, HasFileWatching>(linker, T::file_watching_ctx)
}

impl file_watching::Host for FileWatchingCtx<'_> {}

impl<T: FileWatchingCtxView + 'static> HostWithStore<T> for HasFileWatching {
    /// A descriptor the program does not hold traps, as every file call's does.
    async fn watch(
        accessor: &Accessor<T, Self>,
        folder: Resource<Descriptor>,
        recursive: bool,
        latency_ms: u32,
    ) -> wasmtime::Result<Result<StreamReader<Vec<Change>>, ErrorCode>> {
        let latency = Duration::from_millis(u64::from(latency_ms));
        let decided = accessor.with(|mut access| -> wasmtime::Result<_> {
            let ctx = access.get();
            let descriptor = ctx.table.get(&folder)?;
            let held = ctx.access.decide(Asked::WatchFolder { recursive }, |_| {
                HeldFolder::of(descriptor)
            });
            Ok(held.map(|held| (held, ctx.access.clone())))
        })?;
        let (held, access) = match decided {
            Ok(decided) => decided,
            Err(code) => return Ok(Err(code.into())),
        };
        let watching = access.hold(Holding::Watch {
            folder: held.path().to_path_buf(),
            recursive,
        });
        let batches = match spawn_blocking(move || watch(&held, recursive, latency, watching)).await
        {
            Ok(batches) => batches,
            Err(code) => return Ok(Err(code.into())),
        };
        accessor.with(|access| {
            let stream = stream_of(access, batches, |_: &mut T, batch| Ok(batch))?;
            Ok(Ok(stream))
        })
    }
}
