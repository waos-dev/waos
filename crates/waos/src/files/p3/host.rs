//! The `wasi:filesystem@0.3` calls, each the files' own call on the handles the program holds,
//! made in place: a call the program awaits has its answer when it returns.

use std::iter;

use futures::channel::oneshot;
use wasmtime::AsContextMut;
use wasmtime::component::{Access, Accessor, FutureReader, Lift, Lower, Resource, StreamReader};

use super::bindings::wasi::filesystem::preopens;
use super::bindings::wasi::filesystem::types::{
    self, Advice, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry, Filesize,
    MetadataHashValue, NewTimestamp, OpenFlags, PathFlags,
};
use super::convert::{follows, open_request, time_to_set};
use super::streams::{FileReader, FileWriter, WriteAt};
use super::{FsError, FsResult};
use crate::files::errors::ErrorCode;
use crate::files::target::Target;
use crate::files::{Descriptor, FilesCtx, HasFiles, ProgramFiles};

/// How a stream the program reads or writes ended, as its call's future answers it.
type StreamEnding = FutureReader<Result<(), types::ErrorCode>>;

impl FilesCtx<'_> {
    /// Where `path`, given with the handle `descriptor`, leads.
    fn target_of(&mut self, descriptor: &Resource<Descriptor>, path: &str) -> FsResult<Target> {
        let descriptor = self.table.get(descriptor)?;
        Ok(self.files.target(descriptor, path)?)
    }

    /// The writable file the handle `descriptor` is on.
    fn writable_file(
        &mut self,
        descriptor: &Resource<Descriptor>,
    ) -> FsResult<std::sync::Arc<std::fs::File>> {
        let file = self.table.get(descriptor)?.file()?;
        file.check_writable()?;
        Ok(file.handle().clone())
    }
}

impl preopens::Host for FilesCtx<'_> {
    /// The root, the one folder the program is given.
    fn get_directories(&mut self) -> wasmtime::Result<Vec<(Resource<Descriptor>, String)>> {
        let root = self.table.push(ProgramFiles::root())?;
        Ok(vec![(root, "/".to_owned())])
    }
}

impl types::Host for FilesCtx<'_> {
    fn convert_error_code(&mut self, error: FsError) -> wasmtime::Result<types::ErrorCode> {
        match error {
            FsError::Code(code) => Ok(code),
            FsError::Trap(trap) => Err(trap),
        }
    }
}

impl types::HostDescriptor for FilesCtx<'_> {
    fn drop(&mut self, descriptor: Resource<Descriptor>) -> wasmtime::Result<()> {
        self.table.delete(descriptor)?;
        Ok(())
    }
}

/// A stream of nothing and the error it ended with, for a call that fails before it streams.
fn failed_stream<T: Lift + Lower + Send + Sync + 'static>(
    mut store: impl AsContextMut,
    error: FsError,
) -> wasmtime::Result<(StreamReader<T>, StreamEnding)> {
    let code = match error {
        FsError::Code(code) => code,
        FsError::Trap(trap) => return Err(trap),
    };
    let stream = StreamReader::new(&mut store, iter::empty())?;
    let ending = FutureReader::new(
        &mut store,
        std::future::ready(Ok::<_, wasmtime::Error>(Err(code))),
    )?;
    Ok((stream, ending))
}

/// Writes what `data` brings to `file` at `at`; a file the program may not write is answered at
/// once, its stream closed.
fn write_stream<U: 'static>(
    mut store: Access<'_, U, HasFiles>,
    file: FsResult<std::sync::Arc<std::fs::File>>,
    mut data: StreamReader<u8>,
    at: WriteAt,
) -> wasmtime::Result<StreamEnding> {
    let (ending, ended) = oneshot::channel();
    match file {
        Ok(file) => data.pipe(&mut store, FileWriter::new(file, at, ending))?,
        Err(FsError::Code(code)) => {
            data.close(&mut store)?;
            let _ = ending.send(Err(code));
        }
        Err(FsError::Trap(trap)) => return Err(trap),
    }
    FutureReader::new(&mut store, ended)
}

impl<U: 'static> types::HostDescriptorWithStore<U> for HasFiles {
    fn read_via_stream(
        mut store: Access<U, Self>,
        descriptor: Resource<Descriptor>,
        offset: Filesize,
    ) -> wasmtime::Result<(StreamReader<u8>, StreamEnding)> {
        let file = match store.get().table.get(&descriptor)? {
            Descriptor::File(file) => file.handle().clone(),
            _ => return failed_stream(&mut store, ErrorCode::IsDirectory.into()),
        };
        let (ending, ended) = oneshot::channel();
        let stream = StreamReader::new(&mut store, FileReader::new(file, offset, ending))?;
        Ok((stream, FutureReader::new(&mut store, ended)?))
    }

    fn write_via_stream(
        mut store: Access<'_, U, Self>,
        descriptor: Resource<Descriptor>,
        data: StreamReader<u8>,
        offset: Filesize,
    ) -> wasmtime::Result<StreamEnding> {
        let file = store.get().writable_file(&descriptor);
        write_stream(store, file, data, WriteAt::Position(offset))
    }

    fn append_via_stream(
        mut store: Access<'_, U, Self>,
        descriptor: Resource<Descriptor>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<StreamEnding> {
        let file = store.get().writable_file(&descriptor);
        write_stream(store, file, data, WriteAt::End)
    }

    /// A folder's entries, read whole as the files read them, streamed up to the first that
    /// fails, whose error ends the stream.
    fn read_directory(
        mut store: Access<'_, U, Self>,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<(StreamReader<DirectoryEntry>, StreamEnding)> {
        let read = {
            let ctx = store.get();
            let descriptor = ctx.table.get(&descriptor)?;
            ctx.files.read_directory(descriptor)
        };
        let entries = match read {
            Ok(entries) => entries,
            Err(code) => return failed_stream(&mut store, code.into()),
        };
        let mut streamed = Vec::with_capacity(entries.len());
        let mut how = Ok(());
        for entry in entries {
            match entry {
                Ok(entry) => streamed.push(DirectoryEntry::from(entry)),
                Err(code) => {
                    how = Err(code.into());
                    break;
                }
            }
        }
        let stream = StreamReader::new(&mut store, streamed)?;
        let ending = FutureReader::new(
            &mut store,
            std::future::ready(Ok::<_, wasmtime::Error>(how)),
        )?;
        Ok((stream, ending))
    }

    async fn advise(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        offset: Filesize,
        length: Filesize,
        advice: Advice,
    ) -> FsResult<()> {
        store.with(|mut store| {
            let file = store.get().table.get(&descriptor)?.file()?;
            Ok(file.advise(offset, length, advice.into())?)
        })
    }

    async fn sync_data(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<()> {
        store.with(|mut store| Ok(store.get().table.get(&descriptor)?.sync_data()?))
    }

    async fn get_flags(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<DescriptorFlags> {
        store.with(|mut store| Ok(store.get().table.get(&descriptor)?.flags().into()))
    }

    async fn get_type(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<DescriptorType> {
        store.with(|mut store| Ok(store.get().table.get(&descriptor)?.type_()?.into()))
    }

    async fn set_size(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        size: Filesize,
    ) -> FsResult<()> {
        store.with(|mut store| Ok(store.get().table.get(&descriptor)?.file()?.set_size(size)?))
    }

    async fn set_times(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        accessed: NewTimestamp,
        modified: NewTimestamp,
    ) -> FsResult<()> {
        let (accessed, modified) = (time_to_set(accessed)?, time_to_set(modified)?);
        store.with(|mut store| {
            Ok(store
                .get()
                .table
                .get(&descriptor)?
                .set_times(accessed, modified)?)
        })
    }

    async fn sync(store: &Accessor<U, Self>, descriptor: Resource<Descriptor>) -> FsResult<()> {
        store.with(|mut store| Ok(store.get().table.get(&descriptor)?.sync()?))
    }

    async fn create_directory_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> FsResult<()> {
        store.with(|mut store| {
            Ok(store
                .get()
                .target_of(&descriptor, &path)?
                .create_directory()?)
        })
    }

    async fn stat(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<DescriptorStat> {
        store.with(|mut store| {
            let ctx = store.get();
            let descriptor = ctx.table.get(&descriptor)?;
            Ok(ctx.files.status(descriptor)?.into())
        })
    }

    async fn stat_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> FsResult<DescriptorStat> {
        store.with(|mut store| {
            let target = store.get().target_of(&descriptor, &path)?;
            Ok(target.status(follows(path_flags))?.into())
        })
    }

    async fn set_times_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        accessed: NewTimestamp,
        modified: NewTimestamp,
    ) -> FsResult<()> {
        let (accessed, modified) = (time_to_set(accessed)?, time_to_set(modified)?);
        store.with(|mut store| {
            let target = store.get().target_of(&descriptor, &path)?;
            Ok(target.set_times(follows(path_flags), accessed, modified)?)
        })
    }

    /// Makes `new_path` a hard link to `old_path`, which is never followed: a path flag that asks
    /// to follow it is refused.
    async fn link_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        old_path_flags: PathFlags,
        old_path: String,
        new_descriptor: Resource<Descriptor>,
        new_path: String,
    ) -> FsResult<()> {
        if follows(old_path_flags) {
            return Err(ErrorCode::Invalid.into());
        }
        store.with(|mut store| {
            let mut ctx = store.get();
            let from = ctx.target_of(&descriptor, &old_path)?;
            let to = ctx.target_of(&new_descriptor, &new_path)?;
            Ok(from.link_to(&to)?)
        })
    }

    async fn open_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        flags: DescriptorFlags,
    ) -> FsResult<Resource<Descriptor>> {
        let request = open_request(path_flags, open_flags, flags);
        store.with(|mut store| {
            let mut ctx = store.get();
            let opened = ctx.target_of(&descriptor, &path)?.open(&request)?;
            Ok(ctx.table.push(opened)?)
        })
    }

    async fn readlink_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> FsResult<String> {
        store.with(|mut store| Ok(store.get().target_of(&descriptor, &path)?.read_link()?))
    }

    async fn remove_directory_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> FsResult<()> {
        store.with(|mut store| {
            Ok(store
                .get()
                .target_of(&descriptor, &path)?
                .remove_directory()?)
        })
    }

    async fn rename_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        old_path: String,
        new_descriptor: Resource<Descriptor>,
        new_path: String,
    ) -> FsResult<()> {
        store.with(|mut store| {
            let mut ctx = store.get();
            let from = ctx.target_of(&descriptor, &old_path)?;
            let to = ctx.target_of(&new_descriptor, &new_path)?;
            Ok(from.rename_to(&to)?)
        })
    }

    /// Makes a symlink at `new_path` holding `old_path`.
    async fn symlink_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        old_path: String,
        new_path: String,
    ) -> FsResult<()> {
        store.with(|mut store| {
            Ok(store
                .get()
                .target_of(&descriptor, &new_path)?
                .symlink(&old_path)?)
        })
    }

    async fn unlink_file_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> FsResult<()> {
        store.with(|mut store| Ok(store.get().target_of(&descriptor, &path)?.unlink_file()?))
    }

    /// Whether the two handles are on one file or folder; a handle whose status the system does
    /// not give traps, as in `wasmtime-wasi`, since the call has no error to answer.
    async fn is_same_object(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        other: Resource<Descriptor>,
    ) -> wasmtime::Result<bool> {
        store.with(|mut store| {
            let ctx = store.get();
            let (descriptor, other) = (ctx.table.get(&descriptor)?, ctx.table.get(&other)?);
            descriptor.is_same_object(other).map_err(|code| {
                wasmtime::format_err!("the status of a handle is not given: {code:?}")
            })
        })
    }

    async fn metadata_hash(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<MetadataHashValue> {
        store.with(|mut store| {
            let ctx = store.get();
            let descriptor = ctx.table.get(&descriptor)?;
            Ok(ctx.files.metadata_hash(descriptor)?.into())
        })
    }

    async fn metadata_hash_at(
        store: &Accessor<U, Self>,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> FsResult<MetadataHashValue> {
        store.with(|mut store| {
            let target = store.get().target_of(&descriptor, &path)?;
            Ok(target.metadata_hash(follows(path_flags))?.into())
        })
    }
}
