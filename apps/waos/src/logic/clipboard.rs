//! What goes between the program's clipboard and Waft's: a program's copy in Waft's words, each
//! promise still produced by the program when someone pastes it, a promised file's bytes carried
//! from the program's thread into Waft's sink on the thread Waft asks on, and Waft's refusals
//! in the program's.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use waft::ui::{
    ClipboardError as WaftError, ClipboardItem, Content as WaftContent, ContentFuture,
    CopyOptions as WaftOptions, FileSink as WaftSink, WriteFuture,
};
use waos::bindings::wasdk::platform::clipboard::ClipboardError;
use waos::{Content, CopiedItem, CopyOptions, FileSink, FileWriteFuture};

/// `items`, as Waft writes them.
pub(crate) fn waft_items(items: Vec<CopiedItem>) -> Vec<ClipboardItem> {
    items
        .into_iter()
        .map(|item| ClipboardItem {
            representations: item
                .representations
                .into_iter()
                .map(|(mime, content)| (mime, waft_content(content)))
                .collect(),
        })
        .collect()
}

/// One form's content, as Waft writes it: a promise, given or a file's, still the program's to
/// produce.
fn waft_content(content: Content) -> WaftContent {
    match content {
        Content::Data(bytes) => WaftContent::Data(bytes),
        Content::Promise(produce) => {
            WaftContent::Promise(Rc::new(move || -> ContentFuture { produce() }))
        }
        Content::File(path) => WaftContent::File(path),
        Content::FilePromise { name, produce } => WaftContent::FilePromise {
            name,
            produce: Rc::new(move |sink| into_waft_sink(&produce, sink)),
        },
    }
}

/// What makes a promised file's bytes: the program's producer, on its own thread.
type FileProducer = Arc<dyn Fn(Box<dyn FileSink>) -> FileWriteFuture + Send + Sync>;

/// One paste of a promised file: the program's producer writes into a sink of its own, whose
/// bytes are written into Waft's `sink` as they come, each write answered once Waft has taken
/// it, so the program goes no faster than the receiver's file.
fn into_waft_sink(produce: &FileProducer, mut sink: Box<dyn WaftSink>) -> WriteFuture {
    let (carried, mut chunks) = CarriedSink::new();
    let produced = produce(Box::new(carried));
    Box::pin(async move {
        let written = async {
            while let Some((bytes, taken)) = chunks.next().await {
                let written = sink.write(&bytes).await;
                let failed = written.is_err();
                let _ = taken.send(written);
                if failed {
                    break;
                }
            }
        };
        let (produced, ()) = futures::join!(produced, written);
        produced
    })
}

/// One chunk of a promised file, and where Waft's word on writing it goes.
type Chunk = (Vec<u8>, oneshot::Sender<io::Result<()>>);

/// The program's sink for one paste of a promised file: each write carried to the thread Waft
/// asked on, and answered once Waft's sink has taken it.
struct CarriedSink(mpsc::UnboundedSender<Chunk>);

impl CarriedSink {
    fn new() -> (CarriedSink, mpsc::UnboundedReceiver<Chunk>) {
        let (sender, chunks) = mpsc::unbounded();
        (CarriedSink(sender), chunks)
    }
}

impl FileSink for CarriedSink {
    fn write<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>> {
        let (taken, answer) = oneshot::channel();
        let sent = self.0.unbounded_send((bytes.to_vec(), taken));
        Box::pin(async move {
            sent.map_err(|_| receiver_gone())?;
            answer.await.unwrap_or_else(|_| Err(receiver_gone()))
        })
    }
}

/// Why a write the receiver no longer takes failed.
fn receiver_gone() -> io::Error {
    io::Error::other("the receiver no longer takes the file")
}

/// `options`, as Waft takes them.
pub(crate) fn waft_options(options: CopyOptions) -> WaftOptions {
    WaftOptions {
        sensitive: options.sensitive,
        this_device_only: options.this_device_only,
        expires_after: options.expires_after,
    }
}

/// Waft's `error`, as the program is told it.
pub(crate) fn program_error(error: WaftError) -> ClipboardError {
    match error {
        WaftError::Changed => ClipboardError::Changed,
        WaftError::NotFound => ClipboardError::NotFound,
        WaftError::Denied => ClipboardError::Denied,
        WaftError::Unsupported => ClipboardError::Unsupported,
        WaftError::Failed(reason) => ClipboardError::Failed(reason),
    }
}

#[cfg(test)]
mod tests {
    use std::future::ready;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use futures::executor::block_on;

    use super::*;

    #[test]
    fn a_copy_keeps_its_forms_in_order_and_its_promises_the_programs() {
        let produced = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&produced);
        let item = CopiedItem {
            representations: vec![
                ("text/plain".to_owned(), Content::Data(b"text".to_vec())),
                (
                    "text/html".to_owned(),
                    Content::Promise(Arc::new(move || {
                        counted.fetch_add(1, Ordering::SeqCst);
                        Box::pin(ready(Some(b"<b>text</b>".to_vec())))
                    })),
                ),
                (
                    "text/plain".to_owned(),
                    Content::File(PathBuf::from("/machine/note.txt")),
                ),
            ],
        };
        let [ClipboardItem { representations }] = &waft_items(vec![item])[..] else {
            panic!("one item");
        };
        let mimes: Vec<&str> = representations
            .iter()
            .map(|(mime, _)| mime.as_str())
            .collect();
        assert_eq!(mimes, ["text/plain", "text/html", "text/plain"]);
        assert!(matches!(&representations[0].1, WaftContent::Data(bytes) if bytes == b"text"));
        let WaftContent::Promise(produce) = &representations[1].1 else {
            panic!("the HTML is promised");
        };
        assert_eq!(produced.load(Ordering::SeqCst), 0);
        assert_eq!(block_on(produce()).as_deref(), Some(&b"<b>text</b>"[..]));
        assert_eq!(produced.load(Ordering::SeqCst), 1);
        assert!(matches!(
            &representations[2].1,
            WaftContent::File(path) if path == Path::new("/machine/note.txt")
        ));
    }

    /// A promised file whose producer writes `chunks` in order, then fails if `fails`.
    fn promised_picture(chunks: &'static [&'static [u8]], fails: bool) -> CopiedItem {
        let produce: FileProducer = Arc::new(move |mut sink: Box<dyn FileSink>| {
            Box::pin(async move {
                for chunk in chunks {
                    sink.write(chunk).await?;
                }
                if fails {
                    return Err(io::Error::other("the program broke"));
                }
                Ok(())
            })
        });
        CopiedItem {
            representations: vec![(
                "image/png".to_owned(),
                Content::FilePromise {
                    name: "picture.png".to_owned(),
                    produce,
                },
            )],
        }
    }

    /// Waft's sink for the receiver's file, keeping what is written into it.
    #[derive(Clone, Default)]
    struct ReceiversFile(Rc<std::cell::RefCell<Vec<u8>>>);

    impl WaftSink for ReceiversFile {
        fn write<'a>(
            &'a mut self,
            bytes: &'a [u8],
        ) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'a>> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Box::pin(ready(Ok(())))
        }
    }

    /// Pastes the promised file of `item`, as Waft's host does: what the receiver's file holds
    /// once the producer has finished, and how it finished.
    fn pasted(item: CopiedItem) -> (io::Result<()>, Vec<u8>) {
        let items = waft_items(vec![item]);
        let WaftContent::FilePromise { name, produce } = &items[0].representations[0].1 else {
            panic!("the picture is a promised file");
        };
        assert_eq!(name, "picture.png");
        let file = ReceiversFile::default();
        let produced = block_on(produce(Box::new(file.clone())));
        let written = file.0.borrow().clone();
        (produced, written)
    }

    #[test]
    fn a_promised_files_bytes_reach_wafts_sink_in_order() {
        let (produced, written) = pasted(promised_picture(&[b"\x89P", b"NG"], false));
        produced.expect("the file is whole");
        assert_eq!(written, b"\x89PNG");
    }

    #[test]
    fn a_promised_file_the_program_breaks_fails_the_paste() {
        let (produced, written) = pasted(promised_picture(&[b"\x89P"], true));
        assert_eq!(
            produced.expect_err("broken").to_string(),
            "the program broke"
        );
        assert_eq!(written, b"\x89P", "the host removes what it wrote");
    }

    #[test]
    fn options_and_refusals_keep_their_meaning() {
        let options = CopyOptions {
            sensitive: true,
            this_device_only: false,
            expires_after: Some(Duration::from_secs(60)),
        };
        assert_eq!(
            waft_options(options),
            WaftOptions::new()
                .sensitive(true)
                .expires_after(Duration::from_secs(60))
        );
        assert!(matches!(
            program_error(WaftError::Changed),
            ClipboardError::Changed
        ));
        assert!(matches!(
            program_error(WaftError::Failed("gone".to_owned())),
            ClipboardError::Failed(reason) if reason == "gone"
        ));
    }
}
