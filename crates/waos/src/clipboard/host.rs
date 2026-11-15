//! The code serving `wasdk:platform/clipboard` to a program: the platform's clipboard and its
//! items, a copy's options, a write with the source it puts on the clipboard, the requests of the
//! source's promised forms and files with their answers, a flush, and the user's copy request and
//! paste, which the program hears among a view's events.
//!
//! What a write's content holds of the program's store is taken out in the call, before anything
//! is awaited: each data stream piped into a collection, each file looked up in the program's
//! mounts. A write refused leaves no stream of the program's open.

use std::path::{Path, PathBuf};
use std::time::Duration;

use wasmtime::bail;
use wasmtime::component::{
    Access, Accessor, FutureReader, HasData, Linker, Resource, ResourceTable, StreamReader,
};

use super::answers::{Answering, Done};
use super::written::{self, WrittenForm, WrittenItem};
use super::{
    CopyOptions, CopyRequest, ListedItem, Paste, PlatformClipboard, ProgramClipboard,
    ProgramSources, Request, Source,
};
use crate::access::Cause;
use crate::bindings::wasdk::platform::clipboard::{
    self, ClipboardError, Content, HostClipboard, HostClipboardItem, HostClipboardItemWithStore,
    HostClipboardWithStore, HostCopyOptions, HostCopyRequest, HostCopyRequestWithStore, HostPaste,
    HostRequest, HostRequestWithStore, HostSource, HostSourceWithStore,
};
use crate::files::ProgramFiles;
use crate::lifecycle::Lifecycle;
use crate::stream::{Collected, stream_of, stream_of_bytes};

/// What the clipboard interface reads and writes in a program's store.
pub(crate) struct ClipboardCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) clipboard: &'a ProgramClipboard,
    /// The program's mounts, where a file it writes is looked up.
    pub(crate) files: &'a mut ProgramFiles,
    /// What thaws the program to answer a paste of its source.
    pub(crate) lifecycle: &'a Lifecycle,
}

pub(crate) trait ClipboardCtxView: Send {
    fn clipboard_ctx(&mut self) -> ClipboardCtx<'_>;
}

struct HasClipboard;

impl HasData for HasClipboard {
    type Data<'a> = ClipboardCtx<'a>;
}

pub(crate) fn add_to_linker<T: ClipboardCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    clipboard::add_to_linker::<_, HasClipboard>(linker, T::clipboard_ctx)
}

impl clipboard::Host for ClipboardCtx<'_> {
    fn get_clipboard(&mut self) -> wasmtime::Result<Option<Resource<PlatformClipboard>>> {
        let platform = self.clipboard.platform();
        if !platform.exists() {
            return Ok(None);
        }
        Ok(Some(self.table.push(platform.clone())?))
    }
}

impl ClipboardCtx<'_> {
    /// Each item of `items`, as a handle.
    fn handles(&mut self, items: Vec<ListedItem>) -> wasmtime::Result<Vec<Resource<ListedItem>>> {
        items
            .into_iter()
            .map(|item| Ok(self.table.push(item)?))
            .collect()
    }
}

impl HostClipboard for ClipboardCtx<'_> {
    fn items(
        &mut self,
        _clipboard: Resource<PlatformClipboard>,
    ) -> wasmtime::Result<Vec<Resource<ListedItem>>> {
        let items = self.clipboard.items();
        self.handles(items)
    }

    fn drop(&mut self, clipboard: Resource<PlatformClipboard>) -> wasmtime::Result<()> {
        self.table.delete(clipboard)?;
        Ok(())
    }
}

impl<T: ClipboardCtxView + 'static> HostClipboardWithStore<T> for HasClipboard {
    /// Each listing enters the table as the program reads it, so a change it never reads makes
    /// no handle.
    fn changes(
        mut access: Access<T, Self>,
        _clipboard: Resource<PlatformClipboard>,
    ) -> wasmtime::Result<StreamReader<Vec<Resource<ListedItem>>>> {
        let changes = access.get().clipboard.changes();
        stream_of(access, changes, |data: &mut T, items| {
            data.clipboard_ctx().handles(items)
        })
    }

    async fn write(
        accessor: &Accessor<T, Self>,
        _clipboard: Resource<PlatformClipboard>,
        items: Vec<Vec<(String, Content)>>,
        options: Resource<CopyOptions>,
    ) -> wasmtime::Result<Result<Resource<Source>, ClipboardError>> {
        let taken = accessor.with(|mut access| {
            let writer = Writer::Program;
            take_written(&mut access, items, options, writer)
        })?;
        write_taken(accessor, taken).await
    }

    /// Asks the program, through its source's requests, for the forms its copy still promises,
    /// and keeps what it gives in time.
    async fn flush(
        accessor: &Accessor<T, Self>,
        _clipboard: Resource<PlatformClipboard>,
    ) -> wasmtime::Result<()> {
        let kept = accessor.with(|mut access| access.get().clipboard.sources().keep_promised());
        kept.await;
        Ok(())
    }
}

/// Writes what `taken` took out of the program's store, unless the write was refused, and answers
/// the source it puts on the clipboard.
async fn write_taken<T: ClipboardCtxView + 'static>(
    accessor: &Accessor<T, HasClipboard>,
    taken: Result<Taken, ClipboardError>,
) -> wasmtime::Result<Result<Resource<Source>, ClipboardError>> {
    let taken = match taken {
        Ok(taken) => taken,
        Err(error) => return Ok(Err(error)),
    };
    let copied = written::copied(taken.items, &taken.source).await;
    let change = match taken.platform.write(copied, taken.options).await {
        Ok(change) => change,
        Err(error) => return Ok(Err(error)),
    };
    taken.source.written_as(change);
    taken.sources.wrote(taken.source.clone());
    accessor.with(|mut access| Ok(Ok(access.get().table.push(taken.source)?)))
}

/// Who asks for a write: the program, under its write switch, or the user, through a copy
/// request, which a switch does not hold back.
#[derive(Clone, Copy)]
enum Writer {
    Program,
    User,
}

/// What a write took out of the program's store, and what it goes on with.
struct Taken {
    items: Vec<WrittenItem>,
    options: CopyOptions,
    platform: PlatformClipboard,
    /// What the copy will be on the clipboard, answering its promised forms.
    source: Source,
    sources: ProgramSources,
}

/// `items` and `options` taken out of the program's store, if `writer` may write and each file
/// is in the program's mounts. Every data stream is piped into a collection, or closed when the
/// write is refused.
fn take_written<T: ClipboardCtxView + 'static>(
    access: &mut Access<T, HasClipboard>,
    items: Vec<Vec<(String, Content)>>,
    options: Resource<CopyOptions>,
    writer: Writer,
) -> wasmtime::Result<Result<Taken, ClipboardError>> {
    let context = access.get();
    let options = context.table.delete(options)?;
    let may_write = match writer {
        Writer::Program => context.clipboard.may_write(&forms_of(&items)),
        Writer::User => true,
    };
    let machine_paths = if may_write {
        machine_paths(context.files, &items).ok_or(ClipboardError::NotFound)
    } else {
        Err(ClipboardError::Denied)
    };
    let mut machine_paths = match machine_paths {
        Ok(machine_paths) => machine_paths.into_iter(),
        Err(error) => {
            close_data_streams(access, items)?;
            return Ok(Err(error));
        }
    };
    let mut written = Vec::with_capacity(items.len());
    for item in items {
        let mut forms = Vec::with_capacity(item.len());
        for (mime, content) in item {
            forms.push((mime, written_form(access, content, &mut machine_paths)?));
        }
        written.push(WrittenItem(forms));
    }
    let context = access.get();
    let platform = context.clipboard.platform().clone();
    let source = Source::new(
        platform.clone(),
        written::promised_forms(&written),
        context.lifecycle.clone(),
    );
    Ok(Ok(Taken {
        items: written,
        options,
        platform,
        source,
        sources: context.clipboard.sources().clone(),
    }))
}

/// The forms of `items`, item after item, each in its order.
fn forms_of(items: &[Vec<(String, Content)>]) -> Vec<&str> {
    items
        .iter()
        .flatten()
        .map(|(mime, _content)| mime.as_str())
        .collect()
}

/// Where on the machine each file of `items` is, in their order; none when one is outside the
/// program's mounts.
fn machine_paths(
    files: &mut ProgramFiles,
    items: &[Vec<(String, Content)>],
) -> Option<Vec<PathBuf>> {
    items
        .iter()
        .flatten()
        .filter_map(|(_, content)| match content {
            Content::File(path) => Some(files.machine_path(Path::new(path), Cause::Clipboard)),
            Content::Data(_) | Content::Promise | Content::FilePromise(_) => None,
        })
        .collect()
}

/// One form's content, out of the program's store: a data stream piped into a collection, a
/// file by its path on the machine, the next of `machine_paths`.
fn written_form<T: ClipboardCtxView + 'static>(
    access: &mut Access<T, HasClipboard>,
    content: Content,
    machine_paths: &mut impl Iterator<Item = PathBuf>,
) -> wasmtime::Result<WrittenForm> {
    Ok(match content {
        Content::Data(stream) => {
            let (collection, collected) = Collected::new();
            stream.pipe(&mut *access, collection)?;
            WrittenForm::Data(collected)
        }
        Content::Promise => WrittenForm::Promise,
        Content::File(_) => match machine_paths.next() {
            Some(machine_path) => WrittenForm::File(machine_path),
            None => bail!("a file written was not looked up"),
        },
        Content::FilePromise(name) => WrittenForm::FilePromise(name),
    })
}

/// Closes every data stream of `items`, which a refused write reads nothing of.
fn close_data_streams<T: ClipboardCtxView + 'static>(
    access: &mut Access<T, HasClipboard>,
    items: Vec<Vec<(String, Content)>>,
) -> wasmtime::Result<()> {
    for (_, content) in items.into_iter().flatten() {
        if let Content::Data(mut stream) = content {
            stream.close(&mut *access)?;
        }
    }
    Ok(())
}

impl HostClipboardItem for ClipboardCtx<'_> {
    fn types(&mut self, item: Resource<ListedItem>) -> wasmtime::Result<Vec<String>> {
        Ok(self.table.get(&item)?.types().to_vec())
    }

    fn drop(&mut self, item: Resource<ListedItem>) -> wasmtime::Result<()> {
        self.table.delete(item)?;
        Ok(())
    }
}

impl<T: ClipboardCtxView + 'static> HostClipboardItemWithStore<T> for HasClipboard {
    async fn read(
        accessor: &Accessor<T, Self>,
        item: Resource<ListedItem>,
        r#type: String,
    ) -> wasmtime::Result<Result<StreamReader<u8>, ClipboardError>> {
        let read = accessor.with(|mut access| {
            let context = access.get();
            let item = context.table.get(&item)?;
            Ok::<_, wasmtime::Error>(context.clipboard.read(item, &r#type))
        })?;
        match read.await {
            Ok(bytes) => accessor.with(|access| Ok(Ok(stream_of_bytes(access, bytes)?))),
            Err(error) => Ok(Err(error)),
        }
    }
}

impl HostCopyOptions for ClipboardCtx<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<CopyOptions>> {
        Ok(self.table.push(CopyOptions::default())?)
    }

    fn sensitive(&mut self, options: Resource<CopyOptions>) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.sensitive = true;
        Ok(())
    }

    fn this_device_only(&mut self, options: Resource<CopyOptions>) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.this_device_only = true;
        Ok(())
    }

    /// `duration` in nanoseconds, as the program's clock counts.
    fn expires_after(
        &mut self,
        options: Resource<CopyOptions>,
        duration: u64,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.expires_after = Some(Duration::from_nanos(duration));
        Ok(())
    }

    fn drop(&mut self, options: Resource<CopyOptions>) -> wasmtime::Result<()> {
        self.table.delete(options)?;
        Ok(())
    }
}

impl HostSource for ClipboardCtx<'_> {
    fn drop(&mut self, source: Resource<Source>) -> wasmtime::Result<()> {
        self.table.delete(source)?;
        Ok(())
    }
}

impl<T: ClipboardCtxView + 'static> HostSourceWithStore<T> for HasClipboard {
    /// Each request enters the table as the program reads it.
    fn requests(
        mut access: Access<T, Self>,
        source: Resource<Source>,
    ) -> wasmtime::Result<StreamReader<Resource<Request>>> {
        let requests = access.get().table.get(&source)?.requests();
        stream_of(access, requests, |data: &mut T, request| {
            Ok(data.clipboard_ctx().table.push(request)?)
        })
    }
}

impl HostRequest for ClipboardCtx<'_> {
    fn item(&mut self, request: Resource<Request>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&request)?.form().item)
    }

    fn type_(&mut self, request: Resource<Request>) -> wasmtime::Result<String> {
        Ok(self.table.get(&request)?.form().mime.clone())
    }

    fn drop(&mut self, request: Resource<Request>) -> wasmtime::Result<()> {
        self.table.delete(request)?;
        Ok(())
    }
}

impl<T: ClipboardCtxView + 'static> HostRequestWithStore<T> for HasClipboard {
    /// The content goes to the paste as it comes, and `done` once it settles; a second answer is
    /// closed unread.
    fn provide(
        mut access: Access<T, Self>,
        request: Resource<Request>,
        mut content: StreamReader<u8>,
        mut done: FutureReader<Done>,
    ) -> wasmtime::Result<()> {
        match access.get().table.get(&request)?.answer() {
            Some(answering) => {
                let (chunks, settled) = Answering::begin(answering);
                content.pipe(&mut access, chunks)?;
                done.pipe(&mut access, settled)
            }
            None => {
                content.close(&mut access)?;
                done.close(&mut access)
            }
        }
    }
}

impl HostCopyRequest for ClipboardCtx<'_> {
    fn drop(&mut self, request: Resource<CopyRequest>) -> wasmtime::Result<()> {
        self.table.delete(request)?;
        Ok(())
    }
}

impl<T: ClipboardCtxView + 'static> HostCopyRequestWithStore<T> for HasClipboard {
    /// The user's write, once: a later one is refused, its streams closed unread.
    async fn write(
        accessor: &Accessor<T, Self>,
        request: Resource<CopyRequest>,
        items: Vec<Vec<(String, Content)>>,
        options: Resource<CopyOptions>,
    ) -> wasmtime::Result<Result<Resource<Source>, ClipboardError>> {
        let taken = accessor.with(|mut access| {
            if !access.get().table.get_mut(&request)?.take_write() {
                access.get().table.delete(options)?;
                close_data_streams(&mut access, items)?;
                return Ok(Err(ClipboardError::Denied));
            }
            take_written(&mut access, items, options, Writer::User)
        })?;
        write_taken(accessor, taken).await
    }
}

impl HostPaste for ClipboardCtx<'_> {
    fn items(&mut self, paste: Resource<Paste>) -> wasmtime::Result<Vec<Resource<ListedItem>>> {
        let items = self.table.get(&paste)?.items();
        self.handles(items)
    }

    fn drop(&mut self, paste: Resource<Paste>) -> wasmtime::Result<()> {
        self.table.delete(paste)?;
        Ok(())
    }
}
