//! The `wasi:filesystem@0.2` calls, each the files' own call on the handles the program holds.

use std::io;

use wasmtime::component::Resource;
use wasmtime_wasi_io::streams::{DynInputStream, DynOutputStream};

use super::bindings::wasi::filesystem::preopens;
use super::bindings::wasi::filesystem::types::{self, HostDescriptor, HostDirectoryEntryStream};
use super::convert::{follows, open_request, time_to_set};
use super::streams::{FileInputStream, FileOutputStream};
use super::{FilesCtx, FsError, FsResult};
use crate::files::descriptor::DirectoryEntry;
use crate::files::errors::ErrorCode;
use crate::files::target::Target;
use crate::files::{Descriptor, ProgramFiles};

/// A folder's entries, as the program reads them one at a time. Public, as the bindings that
/// name it as `wasi:filesystem`'s `directory-entry-stream` are; the crate does not export it.
pub struct DirectoryEntries(std::vec::IntoIter<Result<DirectoryEntry, ErrorCode>>);

impl FilesCtx<'_> {
    /// Where `path`, given with the handle `descriptor`, leads.
    fn target(&mut self, descriptor: &Resource<Descriptor>, path: &str) -> FsResult<Target> {
        let descriptor = self.table.get(descriptor)?;
        Ok(self.files.target(descriptor, path)?)
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

    /// The code of a stream's failed read or write, which holds the system's error.
    fn filesystem_error_code(
        &mut self,
        error: Resource<wasmtime::Error>,
    ) -> wasmtime::Result<Option<types::ErrorCode>> {
        let error = self.table.get(&error)?;
        Ok(error
            .downcast_ref::<io::Error>()
            .map(|error| ErrorCode::from(error).into()))
    }
}

impl HostDescriptor for FilesCtx<'_> {
    fn read_via_stream(
        &mut self,
        descriptor: Resource<Descriptor>,
        offset: types::Filesize,
    ) -> FsResult<Resource<DynInputStream>> {
        let file = match self.table.get(&descriptor)? {
            Descriptor::File(file) => file,
            Descriptor::Dir(_) | Descriptor::Above(_) => return Err(ErrorCode::IsDirectory.into()),
            #[cfg(windows)]
            Descriptor::Drives => return Err(ErrorCode::IsDirectory.into()),
        };
        let stream: DynInputStream = Box::new(FileInputStream::new(file.handle().clone(), offset));
        Ok(self.table.push(stream)?)
    }

    fn write_via_stream(
        &mut self,
        descriptor: Resource<Descriptor>,
        offset: types::Filesize,
    ) -> FsResult<Resource<DynOutputStream>> {
        let file = self.table.get(&descriptor)?.file()?;
        file.check_writable()?;
        let stream: DynOutputStream = Box::new(FileOutputStream::at(file.handle().clone(), offset));
        Ok(self.table.push(stream)?)
    }

    fn append_via_stream(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<Resource<DynOutputStream>> {
        let file = self.table.get(&descriptor)?.file()?;
        file.check_writable()?;
        let stream: DynOutputStream = Box::new(FileOutputStream::appending(file.handle().clone()));
        Ok(self.table.push(stream)?)
    }

    fn advise(
        &mut self,
        descriptor: Resource<Descriptor>,
        offset: types::Filesize,
        length: types::Filesize,
        advice: types::Advice,
    ) -> FsResult<()> {
        let file = self.table.get(&descriptor)?.file()?;
        Ok(file.advise(offset, length, advice.into())?)
    }

    fn sync_data(&mut self, descriptor: Resource<Descriptor>) -> FsResult<()> {
        Ok(self.table.get(&descriptor)?.sync_data()?)
    }

    fn get_flags(&mut self, descriptor: Resource<Descriptor>) -> FsResult<types::DescriptorFlags> {
        Ok(self.table.get(&descriptor)?.flags().into())
    }

    fn get_type(&mut self, descriptor: Resource<Descriptor>) -> FsResult<types::DescriptorType> {
        Ok(self.table.get(&descriptor)?.type_()?.into())
    }

    fn set_size(
        &mut self,
        descriptor: Resource<Descriptor>,
        size: types::Filesize,
    ) -> FsResult<()> {
        Ok(self.table.get(&descriptor)?.file()?.set_size(size)?)
    }

    fn set_times(
        &mut self,
        descriptor: Resource<Descriptor>,
        accessed: types::NewTimestamp,
        modified: types::NewTimestamp,
    ) -> FsResult<()> {
        let (accessed, modified) = (time_to_set(accessed)?, time_to_set(modified)?);
        Ok(self.table.get(&descriptor)?.set_times(accessed, modified)?)
    }

    fn read(
        &mut self,
        descriptor: Resource<Descriptor>,
        length: types::Filesize,
        offset: types::Filesize,
    ) -> FsResult<(Vec<u8>, bool)> {
        Ok(self.table.get(&descriptor)?.file()?.read(length, offset)?)
    }

    fn write(
        &mut self,
        descriptor: Resource<Descriptor>,
        buffer: Vec<u8>,
        offset: types::Filesize,
    ) -> FsResult<types::Filesize> {
        Ok(self
            .table
            .get(&descriptor)?
            .file()?
            .write(&buffer, offset)?)
    }

    fn read_directory(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<Resource<DirectoryEntries>> {
        let descriptor = self.table.get(&descriptor)?;
        let entries = self.files.read_directory(descriptor)?;
        Ok(self.table.push(DirectoryEntries(entries.into_iter()))?)
    }

    fn sync(&mut self, descriptor: Resource<Descriptor>) -> FsResult<()> {
        Ok(self.table.get(&descriptor)?.sync()?)
    }

    fn create_directory_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> FsResult<()> {
        Ok(self.target(&descriptor, &path)?.create_directory()?)
    }

    fn stat(&mut self, descriptor: Resource<Descriptor>) -> FsResult<types::DescriptorStat> {
        let descriptor = self.table.get(&descriptor)?;
        Ok(self.files.status(descriptor)?.into())
    }

    fn stat_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: types::PathFlags,
        path: String,
    ) -> FsResult<types::DescriptorStat> {
        let target = self.target(&descriptor, &path)?;
        Ok(target.status(follows(path_flags))?.into())
    }

    fn set_times_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: types::PathFlags,
        path: String,
        accessed: types::NewTimestamp,
        modified: types::NewTimestamp,
    ) -> FsResult<()> {
        let (accessed, modified) = (time_to_set(accessed)?, time_to_set(modified)?);
        let target = self.target(&descriptor, &path)?;
        Ok(target.set_times(follows(path_flags), accessed, modified)?)
    }

    /// Makes `new_path` a hard link to `old_path`, which is never followed: a path flag that asks
    /// to follow it is refused.
    fn link_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        old_path_flags: types::PathFlags,
        old_path: String,
        new_descriptor: Resource<Descriptor>,
        new_path: String,
    ) -> FsResult<()> {
        if follows(old_path_flags) {
            return Err(ErrorCode::Invalid.into());
        }
        let from = self.target(&descriptor, &old_path)?;
        let to = self.target(&new_descriptor, &new_path)?;
        Ok(from.link_to(&to)?)
    }

    fn open_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: types::PathFlags,
        path: String,
        open_flags: types::OpenFlags,
        flags: types::DescriptorFlags,
    ) -> FsResult<Resource<Descriptor>> {
        let request = open_request(path_flags, open_flags, flags);
        let opened = self.target(&descriptor, &path)?.open(&request)?;
        Ok(self.table.push(opened)?)
    }

    fn drop(&mut self, descriptor: Resource<Descriptor>) -> wasmtime::Result<()> {
        self.table.delete(descriptor)?;
        Ok(())
    }

    fn readlink_at(&mut self, descriptor: Resource<Descriptor>, path: String) -> FsResult<String> {
        Ok(self.target(&descriptor, &path)?.read_link()?)
    }

    fn remove_directory_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> FsResult<()> {
        Ok(self.target(&descriptor, &path)?.remove_directory()?)
    }

    fn rename_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        old_path: String,
        new_descriptor: Resource<Descriptor>,
        new_path: String,
    ) -> FsResult<()> {
        let from = self.target(&descriptor, &old_path)?;
        let to = self.target(&new_descriptor, &new_path)?;
        Ok(from.rename_to(&to)?)
    }

    /// Makes a symlink at `new_path` holding `old_path`.
    fn symlink_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        old_path: String,
        new_path: String,
    ) -> FsResult<()> {
        Ok(self.target(&descriptor, &new_path)?.symlink(&old_path)?)
    }

    fn unlink_file_at(&mut self, descriptor: Resource<Descriptor>, path: String) -> FsResult<()> {
        Ok(self.target(&descriptor, &path)?.unlink_file()?)
    }

    /// Whether the two handles are on one file or folder; a handle whose status the system does
    /// not give traps, as in `wasmtime-wasi`, since the call has no error to answer.
    fn is_same_object(
        &mut self,
        descriptor: Resource<Descriptor>,
        other: Resource<Descriptor>,
    ) -> wasmtime::Result<bool> {
        let (descriptor, other) = (self.table.get(&descriptor)?, self.table.get(&other)?);
        descriptor
            .is_same_object(other)
            .map_err(|code| wasmtime::format_err!("the status of a handle is not given: {code:?}"))
    }

    fn metadata_hash(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> FsResult<types::MetadataHashValue> {
        let descriptor = self.table.get(&descriptor)?;
        Ok(self.files.metadata_hash(descriptor)?.into())
    }

    fn metadata_hash_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: types::PathFlags,
        path: String,
    ) -> FsResult<types::MetadataHashValue> {
        let target = self.target(&descriptor, &path)?;
        Ok(target.metadata_hash(follows(path_flags))?.into())
    }
}

impl HostDirectoryEntryStream for FilesCtx<'_> {
    fn read_directory_entry(
        &mut self,
        entries: Resource<DirectoryEntries>,
    ) -> FsResult<Option<types::DirectoryEntry>> {
        let entries = self.table.get_mut(&entries)?;
        let entry = entries.0.next().transpose()?;
        Ok(entry.map(Into::into))
    }

    fn drop(&mut self, entries: Resource<DirectoryEntries>) -> wasmtime::Result<()> {
        self.table.delete(entries)?;
        Ok(())
    }
}
