//! The clipboard as Waft sees it: `wasdk:platform/clipboard`'s, one to one. Waft's
//! `Clipboard::items` is the WIT's `items`, a pasted item's `read` is the item's `read`, its
//! stream read to the end, and a write is the WIT's `write`, each form's content as the WIT
//! carries it: bytes given as a stream, a file by its path inside the program, and a promised form
//! or file answered from the source's requests, each time someone pastes it, by the producer
//! Waft gave ([`promises`]), until another copy replaces it. Before the program ends, its embedder
//! flushes the clipboard ([`WasdkClipboard::flush`]), so the forms it promises outlive it.
//!
//! The user's copy, cut and paste, which a view hears, are Waft's `ClipboardEvent`s: a copy or a
//! cut writes through the WIT's `copy-request` as a write of the clipboard does
//! ([`copy_request`]), and a paste's items are listed as the clipboard's are ([`pasted_items`]).

mod promises;

use std::future::{Future, ready};
use std::rc::Rc;
use std::time::Duration;

use waft_embedder::{
    Clipboard, ClipboardError, ClipboardItem, ClipboardItemsFuture, ClipboardReadFuture,
    ClipboardWriteFuture, Content, CopyOptions, CopyRequest, DataTransferItem,
};
use wasdk::clipboard as platform;
use wasdk::{StreamReader, wit_stream};

use promises::{Producer, Promises, answer_pastes};

/// The platform's clipboard, as Waft reaches it.
pub(crate) struct WasdkClipboard(Rc<platform::Clipboard>);

impl WasdkClipboard {
    /// The platform's clipboard, where it has one.
    pub(crate) fn get() -> Option<WasdkClipboard> {
        platform::get_clipboard().map(|clipboard| WasdkClipboard(Rc::new(clipboard)))
    }

    /// Asks for every form the program's copy still promises, files aside, while its answers
    /// still run; resolves once each is answered or its time is up.
    pub(crate) fn flush(&self) -> impl Future<Output = ()> + use<> {
        let clipboard = Rc::clone(&self.0);
        async move { clipboard.flush().await }
    }
}

impl Clipboard for WasdkClipboard {
    /// The WIT answers at once: empty while none of the program's views has the focus.
    fn items(&self) -> ClipboardItemsFuture {
        let items = self
            .0
            .items()
            .into_iter()
            .map(WasdkDataTransferItem::listed);
        Box::pin(ready(items.collect()))
    }

    fn write(&self, items: Vec<ClipboardItem>, options: CopyOptions) -> ClipboardWriteFuture {
        let clipboard = Rc::clone(&self.0);
        written_through(items, options, async move |items, options| {
            clipboard.write(items, options).await
        })
    }
}

/// The user's copy or cut, as Waft's: its writes go through `request`, which takes one.
pub(crate) fn copy_request(request: platform::CopyRequest) -> CopyRequest {
    let request = Rc::new(request);
    CopyRequest::new(move |items, options| {
        let request = Rc::clone(&request);
        written_through(items, options, async move |items, options| {
            request.write(items, options).await
        })
    })
}

/// The items of the user's paste, as Waft lists the clipboard's.
pub(crate) fn pasted_items(paste: platform::Paste) -> Vec<Box<dyn DataTransferItem>> {
    paste
        .items()
        .into_iter()
        .map(WasdkDataTransferItem::listed)
        .collect()
}

/// Writes `items` as `options` say through `write`, the WIT's write of the clipboard or of a
/// copy request, and answers the pastes of its promised forms from the source it puts there.
fn written_through<W>(
    items: Vec<ClipboardItem>,
    options: CopyOptions,
    write: W,
) -> ClipboardWriteFuture
where
    W: AsyncFnOnce(
            Vec<Vec<(String, platform::Content)>>,
            platform::CopyOptions,
        ) -> Result<platform::Source, platform::ClipboardError>
        + 'static,
{
    let written = written(items);
    let options = copy_options(options);
    Box::pin(async move {
        let source = write(written.items, options)
            .await
            .map_err(framework_error)?;
        wasdk::spawn(answer_pastes(source, written.promises));
        Ok(())
    })
}

/// One item on the clipboard, as the program listed it.
struct WasdkDataTransferItem {
    item: Rc<platform::ClipboardItem>,
    types: Vec<String>,
}

impl WasdkDataTransferItem {
    fn listed(item: platform::ClipboardItem) -> Box<dyn DataTransferItem> {
        Box::new(WasdkDataTransferItem {
            types: item.types(),
            item: Rc::new(item),
        })
    }
}

impl DataTransferItem for WasdkDataTransferItem {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let item = Rc::clone(&self.item);
        let mime = mime.to_owned();
        Box::pin(async move {
            let content = item.read(mime).await.map_err(framework_error)?;
            Ok(content.collect().await)
        })
    }
}

/// A write as the WIT takes it: each item in its forms, and what produces its promised ones.
#[derive(Default)]
struct Written {
    items: Vec<Vec<(String, platform::Content)>>,
    promises: Promises,
}

/// `items` as the WIT writes them.
fn written(items: Vec<ClipboardItem>) -> Written {
    let mut written = Written::default();
    for (item, ClipboardItem { representations }) in (0u32..).zip(items) {
        let mut forms = Vec::with_capacity(representations.len());
        for (mime, content) in representations {
            let content = match content {
                Content::Data(bytes) => platform::Content::Data(stream_of(bytes)),
                Content::Promise(produce) => {
                    let producer = Producer::Form(produce);
                    written.promises.promise(item, mime.clone(), producer);
                    platform::Content::Promise
                }
                Content::File(path) => platform::Content::File(path.to_string_lossy().into_owned()),
                Content::FilePromise { name, produce } => {
                    let producer = Producer::File(produce);
                    written.promises.promise(item, mime.clone(), producer);
                    platform::Content::FilePromise(name)
                }
            };
            forms.push((mime, content));
        }
        written.items.push(forms);
    }
    written
}

/// `bytes`, as a stream the program gives, written by a task of its own.
pub(super) fn stream_of(bytes: Vec<u8>) -> StreamReader<u8> {
    let (mut writer, reader) = wit_stream::new::<u8>();
    wasdk::spawn(async move {
        writer.write_all(bytes).await;
    });
    reader
}

/// `options`, as the WIT's `copy-options` set them.
fn copy_options(options: CopyOptions) -> platform::CopyOptions {
    let copy_options = platform::CopyOptions::new();
    if options.sensitive {
        copy_options.sensitive();
    }
    if options.this_device_only {
        copy_options.this_device_only();
    }
    if let Some(expiry) = options.expires_after {
        copy_options.expires_after(nanoseconds(expiry));
    }
    copy_options
}

/// `duration` in nanoseconds, as the WIT's `duration` counts, the longest it holds for longer.
fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// The WIT's `error`, as Waft names it.
fn framework_error(error: platform::ClipboardError) -> ClipboardError {
    match error {
        platform::ClipboardError::Changed => ClipboardError::Changed,
        platform::ClipboardError::NotFound => ClipboardError::NotFound,
        platform::ClipboardError::Denied => ClipboardError::Denied,
        platform::ClipboardError::Unsupported => ClipboardError::Unsupported,
        platform::ClipboardError::Failed(reason) => ClipboardError::Failed(reason),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn item(representations: Vec<(&str, Content)>) -> ClipboardItem {
        ClipboardItem {
            representations: representations
                .into_iter()
                .map(|(mime, content)| (mime.to_owned(), content))
                .collect(),
        }
    }

    #[test]
    fn a_file_is_written_by_its_path_and_a_promised_file_by_its_name() {
        let note = item(vec![(
            "text/plain",
            Content::File(PathBuf::from("/data/a")),
        )]);
        let picture = item(vec![(
            "image/png",
            Content::FilePromise {
                name: "picture.png".to_owned(),
                produce: Rc::new(|_| Box::pin(ready(Ok(())))),
            },
        )]);
        let copied = written(vec![note, picture]);
        assert!(matches!(
            &copied.items[0][..],
            [(mime, platform::Content::File(path))] if mime == "text/plain" && path == "/data/a"
        ));
        assert!(matches!(
            &copied.items[1][..],
            [(mime, platform::Content::FilePromise(name))] if mime == "image/png" && name == "picture.png"
        ));
        assert!(matches!(
            copied.promises.producer(1, "image/png".to_owned()),
            Some(Producer::File(_))
        ));
    }

    #[test]
    fn an_expiry_is_counted_in_nanoseconds_up_to_the_longest_the_wit_holds() {
        assert_eq!(nanoseconds(Duration::from_secs(60)), 60_000_000_000);
        assert_eq!(nanoseconds(Duration::MAX), u64::MAX);
    }

    #[test]
    fn the_wits_refusals_keep_their_meaning() {
        assert_eq!(
            framework_error(platform::ClipboardError::Changed),
            ClipboardError::Changed
        );
        assert_eq!(
            framework_error(platform::ClipboardError::Failed("gone".to_owned())),
            ClipboardError::Failed("gone".to_owned())
        );
    }
}
