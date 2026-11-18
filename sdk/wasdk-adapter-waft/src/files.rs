//! Waft's file system over `wasdk::filesystem`, WASI 0.3's: a path read and written whole, opened
//! with a mode and read or written at an offset, its metadata, a folder listed, a folder made, and
//! paths renamed and deleted (the design's decisions 1, 2 and 6).
//!
//! The program's files are the ones `std::fs` reaches: `/data`, `/cache`, `/tmp`, the user's
//! mounts, and a file picked for it, each at its own path. WASI 0.3's file system is async, so
//! every call here awaits the engine's answer, and a read or a write streams as the engine
//! delivers it. WASI reads at an offset and keeps no cursor, so an opened file's cursor is this
//! adapter's.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use waft_embedder::{
    File, FileKind, FileMetadataFuture, FileOperationFuture, FileReadFuture, FileSizeFuture,
    FileWriteFuture, Files, ListFilesFuture, ListedEntry, Metadata, OpenFileFuture, OpenMode,
    ReadFileFuture, WriteFileFuture,
};
use wasdk::filesystem::{
    Descriptor, DescriptorFlags, DescriptorStat, DescriptorType, ErrorCode, Instant, OpenFlags,
    PathFlags,
};
use wasdk::preopens;
use wasdk::{StreamResult, wit_stream};

/// How much one whole read asks for at a time.
const READ_CHUNK: usize = 64 * 1024;

/// Waft's file system, over the root the engine gives the program.
pub(crate) struct WasdkFiles {
    /// Shared with each call's future, which outlives the borrow of this.
    root: Rc<Descriptor>,
}

impl WasdkFiles {
    /// The program's file system: its root, `/`, from the directories WASI preopens.
    pub(crate) fn get() -> Option<WasdkFiles> {
        let root = preopens::get_directories()
            .into_iter()
            .find_map(|(root, path)| (path == "/").then_some(root))?;
        Some(WasdkFiles {
            root: Rc::new(root),
        })
    }
}

/// `path` opened as `mode` says, its cursor at the start or, where the mode appends, the end.
async fn opened(root: &Descriptor, path: &str, mode: OpenMode) -> io::Result<WasdkFile> {
    let (open, flags) = mode_flags(mode);
    let descriptor = root
        .open_at(PathFlags::SYMLINK_FOLLOW, path.to_owned(), open, flags)
        .await
        .map_err(io_error)?;
    let cursor = if mode.appends() {
        descriptor.stat().await.map_err(io_error)?.size
    } else {
        0
    };
    Ok(WasdkFile {
        descriptor: Rc::new(descriptor),
        cursor,
    })
}

impl Files for WasdkFiles {
    fn read(&self, path: &std::path::Path) -> ReadFileFuture {
        let path = program_path(path);
        let root = Rc::clone(&self.root);
        Box::pin(async move {
            let mut file = opened(&root, &path?, OpenMode::Read).await?;
            read_whole(&mut file).await
        })
    }

    fn write(&self, path: &std::path::Path, bytes: &[u8]) -> WriteFileFuture {
        let path = program_path(path);
        let bytes = bytes.to_vec();
        let root = Rc::clone(&self.root);
        Box::pin(async move {
            let mut file = opened(&root, &path?, OpenMode::Write).await?;
            write_whole(&mut file, &bytes).await?;
            file.descriptor.sync_data().await.map_err(io_error)
        })
    }

    fn open(&self, path: &std::path::Path, mode: OpenMode) -> OpenFileFuture {
        let path = program_path(path);
        let root = Rc::clone(&self.root);
        Box::pin(async move {
            let file = opened(&root, &path?, mode).await?;
            Ok(Box::new(file) as Box<dyn File>)
        })
    }

    fn metadata(&self, path: &std::path::Path) -> FileMetadataFuture {
        let path = program_path(path);
        let root = Rc::clone(&self.root);
        Box::pin(async move {
            let status = root
                .stat_at(PathFlags::SYMLINK_FOLLOW, path?)
                .await
                .map_err(io_error)?;
            Ok(metadata_of(status))
        })
    }

    fn list(&self, path: &std::path::Path) -> ListFilesFuture {
        let listing = path.to_path_buf();
        let path = program_path(path);
        let root = Rc::clone(&self.root);
        Box::pin(async move {
            let folder = root
                .open_at(
                    PathFlags::SYMLINK_FOLLOW,
                    path?,
                    OpenFlags::DIRECTORY,
                    DescriptorFlags::READ,
                )
                .await
                .map_err(io_error)?;
            let (entries, ended) = folder.read_directory();
            let entries = entries.collect().await;
            ended.await.map_err(io_error)?;
            Ok(entries
                .into_iter()
                .map(|entry| ListedEntry {
                    path: listing.join(&entry.name),
                    name: entry.name,
                    kind: kind_of(entry.type_),
                })
                .collect())
        })
    }

    fn create_folder(&self, path: &std::path::Path) -> FileOperationFuture {
        let path = program_path(path);
        let root = Rc::clone(&self.root);
        Box::pin(async move {
            let mut made = String::new();
            for component in path?.split('/').filter(|name| !name.is_empty()) {
                if !made.is_empty() {
                    made.push('/');
                }
                made.push_str(component);
                match root.create_directory_at(made.clone()).await {
                    Ok(()) | Err(ErrorCode::Exist) => {}
                    Err(error) => return Err(io_error(error)),
                }
            }
            Ok(())
        })
    }

    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> FileOperationFuture {
        let (from, to) = (program_path(from), program_path(to));
        let root = Rc::clone(&self.root);
        Box::pin(async move { root.rename_at(from?, &root, to?).await.map_err(io_error) })
    }

    fn delete(&self, path: &std::path::Path) -> FileOperationFuture {
        let path = program_path(path);
        let root = Rc::clone(&self.root);
        Box::pin(async move { delete_tree(&root, &path?).await })
    }
}

/// A file the program opened, with the cursor WASI does not keep.
struct WasdkFile {
    /// Shared with `size`'s future, which outlives the borrow of this.
    descriptor: Rc<Descriptor>,
    cursor: u64,
}

impl WasdkFile {
    /// Reads at `offset` into `buf`, leaving the cursor alone.
    async fn read_from(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (mut stream, ended) = self.descriptor.read_via_stream(offset);
        let (result, data) = stream.read(Vec::with_capacity(buf.len())).await;
        let read = match result {
            StreamResult::Complete(read) => read,
            StreamResult::Dropped | StreamResult::Cancelled => 0,
        };
        let read = read.min(buf.len());
        buf[..read].copy_from_slice(&data[..read]);
        drop(stream);
        if read == 0 {
            ended.await.map_err(io_error)?;
        }
        Ok(read)
    }

    /// Writes `bytes` at `offset`, leaving the cursor alone.
    async fn write_at_offset(&self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        let (mut writer, reader) = wit_stream::new::<u8>();
        let bytes = bytes.to_vec();
        wasdk::spawn(async move {
            let _ = writer.write_all(bytes).await;
        });
        self.descriptor
            .write_via_stream(reader, offset)
            .await
            .map_err(io_error)
    }
}

impl File for WasdkFile {
    fn size(&self) -> FileSizeFuture {
        let descriptor = Rc::clone(&self.descriptor);
        Box::pin(async move { Ok(descriptor.stat().await.map_err(io_error)?.size) })
    }

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> FileReadFuture<'a> {
        Box::pin(async move {
            let read = self.read_from(self.cursor, buf).await?;
            self.cursor += read as u64;
            Ok(read)
        })
    }

    fn read_at<'a>(&'a self, offset: u64, buf: &'a mut [u8]) -> FileReadFuture<'a> {
        Box::pin(async move { self.read_from(offset, buf).await })
    }

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> FileWriteFuture<'a> {
        Box::pin(async move {
            self.write_at_offset(self.cursor, bytes).await?;
            self.cursor += bytes.len() as u64;
            Ok(())
        })
    }

    fn write_at<'a>(&'a mut self, offset: u64, bytes: &'a [u8]) -> FileWriteFuture<'a> {
        Box::pin(async move { self.write_at_offset(offset, bytes).await })
    }

    fn set_len(&mut self, len: u64) -> FileWriteFuture<'_> {
        Box::pin(async move { self.descriptor.set_size(len).await.map_err(io_error) })
    }

    fn flush(&mut self) -> FileWriteFuture<'_> {
        Box::pin(async move { self.descriptor.sync_data().await.map_err(io_error) })
    }
}

/// Reads `file` whole, a chunk at a time.
async fn read_whole(file: &mut WasdkFile) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0; READ_CHUNK];
    loop {
        let read = file.read(&mut chunk).await?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

/// Writes `bytes` to `file` at its cursor, all of them.
async fn write_whole(file: &mut WasdkFile, bytes: &[u8]) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        file.write(&bytes[written..]).await?;
        written = file.cursor as usize;
    }
    Ok(())
}

/// Deletes the file, link or folder at `path`, a folder with what is in it, as Dart's
/// `Directory.delete(recursive: true)` deletes one.
fn delete_tree<'a>(
    root: &'a Descriptor,
    path: &'a str,
) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'a>> {
    Box::pin(async move {
        let status = root
            .stat_at(PathFlags::empty(), path.to_owned())
            .await
            .map_err(io_error)?;
        if !matches!(status.type_, DescriptorType::Directory) {
            return root.unlink_file_at(path.to_owned()).await.map_err(io_error);
        }
        let folder = root
            .open_at(
                PathFlags::SYMLINK_FOLLOW,
                path.to_owned(),
                OpenFlags::DIRECTORY,
                DescriptorFlags::READ | DescriptorFlags::MUTATE_DIRECTORY,
            )
            .await
            .map_err(io_error)?;
        let (entries, ended) = folder.read_directory();
        let entries = entries.collect().await;
        ended.await.map_err(io_error)?;
        for entry in entries {
            let child = format!("{path}/{}", entry.name);
            delete_tree(root, &child).await?;
        }
        root.remove_directory_at(path.to_owned())
            .await
            .map_err(io_error)
    })
}

/// `path` as the root descriptor names it: a program path with its leading `/` taken off.
fn program_path(path: &std::path::Path) -> io::Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a path that is not text"))?;
    Ok(text.strip_prefix('/').unwrap_or(text).to_owned())
}

/// The flags that open a file in `mode`, and what the opened file may do.
fn mode_flags(mode: OpenMode) -> (OpenFlags, DescriptorFlags) {
    let read = DescriptorFlags::READ;
    let write = DescriptorFlags::WRITE;
    match mode {
        OpenMode::Read => (OpenFlags::empty(), read),
        OpenMode::Write => (OpenFlags::CREATE | OpenFlags::TRUNCATE, read | write),
        OpenMode::Append => (OpenFlags::CREATE, read | write),
        OpenMode::WriteOnly => (OpenFlags::CREATE | OpenFlags::TRUNCATE, write),
        OpenMode::WriteOnlyAppend => (OpenFlags::CREATE, write),
    }
}

/// What a descriptor type is, in Waft's words.
fn kind_of(type_: DescriptorType) -> FileKind {
    match type_ {
        DescriptorType::RegularFile => FileKind::File,
        DescriptorType::Directory => FileKind::Folder,
        DescriptorType::SymbolicLink => FileKind::Link,
        _ => FileKind::Other,
    }
}

/// What a descriptor's status says, as Waft's metadata.
fn metadata_of(status: DescriptorStat) -> Metadata {
    Metadata {
        kind: kind_of(status.type_),
        size: status.size,
        modified: status.data_modification_timestamp.map(system_time),
    }
}

/// `at`, a WASI instant, as the system's time.
fn system_time(at: Instant) -> SystemTime {
    UNIX_EPOCH + Duration::new(at.seconds as u64, at.nanoseconds)
}

/// Why WASI refused a call, as the I/O error Waft's `Files` answers.
fn io_error(code: ErrorCode) -> io::Error {
    let kind = match code {
        ErrorCode::NoEntry => io::ErrorKind::NotFound,
        ErrorCode::Access | ErrorCode::NotPermitted => io::ErrorKind::PermissionDenied,
        ErrorCode::Exist => io::ErrorKind::AlreadyExists,
        ErrorCode::Invalid => io::ErrorKind::InvalidInput,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("{code:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_path_is_the_roots_name_for_it() {
        assert_eq!(
            program_path(std::path::Path::new("/data/a.txt")).unwrap(),
            "data/a.txt"
        );
        assert_eq!(
            program_path(std::path::Path::new("data/a.txt")).unwrap(),
            "data/a.txt"
        );
        assert_eq!(program_path(std::path::Path::new("/")).unwrap(), "");
    }

    #[test]
    fn a_mode_opens_and_permits_what_it_says() {
        let (open, flags) = mode_flags(OpenMode::Read);
        assert_eq!(open, OpenFlags::empty());
        assert_eq!(flags, DescriptorFlags::READ);
        let (open, flags) = mode_flags(OpenMode::Write);
        assert!(open.contains(OpenFlags::CREATE | OpenFlags::TRUNCATE));
        assert_eq!(flags, DescriptorFlags::READ | DescriptorFlags::WRITE);
        let (open, _) = mode_flags(OpenMode::Append);
        assert_eq!(open, OpenFlags::CREATE);
    }
}
