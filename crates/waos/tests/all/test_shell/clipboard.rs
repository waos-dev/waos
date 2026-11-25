//! The shell's clipboard: what a program writes, held in memory as the platform holds it and
//! listed back after each write, and pastes a test makes of it as another application would,
//! each form given, promised or a file; and what another application copies, which replaces
//! what is on it and which the shell tells the engine of.
//!
//! An item listed reads only while the copy it is of is the one on the clipboard.

use std::future::{Future, ready};
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use waos::bindings::wasdk::platform::clipboard::ClipboardError;
use waos::{
    ClipboardItemsFuture, ClipboardReadFuture, ClipboardWriteFuture, Content, CopiedItem,
    CopyOptions, FileSink, ShellClipboard, ShellClipboardItem, WaOS,
};

/// What the last program to write put on the clipboard, and how it is to be treated; and what
/// is on the clipboard now, a program's copy or another application's, as it is listed.
#[derive(Default)]
pub struct TestClipboard {
    copy: Mutex<Option<(Vec<CopiedItem>, CopyOptions)>>,
    listed: Mutex<Vec<Arc<dyn ShellClipboardItem>>>,
    /// How many copies the clipboard has held, the last being the one on it.
    copies: Arc<AtomicU64>,
}

impl TestClipboard {
    /// Another application copies `items`: they are what is on the clipboard, and `engine` is
    /// told of the change, as a shell tells it.
    pub fn another_application_copies(
        &self,
        engine: &WaOS,
        items: Vec<Arc<dyn ShellClipboardItem>>,
    ) {
        engine.set_clipboard(self.hold(items));
    }

    /// Another application copies `items` and the shell does not hear of it, as no system tells
    /// an application of a copy made while it is in the background: they are what is on the
    /// clipboard, and the engine is told nothing.
    pub fn another_application_copies_unheard(&self, items: Vec<Arc<dyn ShellClipboardItem>>) {
        self.hold(items);
    }

    /// `items` are what is on the clipboard from now on; answers them as it lists them.
    fn hold(&self, items: Vec<Arc<dyn ShellClipboardItem>>) -> Vec<Arc<dyn ShellClipboardItem>> {
        let mut listed = self.listed.lock().unwrap();
        let copy = self.copies.fetch_add(1, Ordering::SeqCst) + 1;
        *listed = items
            .into_iter()
            .map(|item| OfCopy::of(item, copy, &self.copies))
            .collect();
        listed.clone()
    }

    /// How the copy on the clipboard is to be treated.
    pub fn options(&self) -> CopyOptions {
        self.copy.lock().unwrap().as_ref().expect("a copy").1
    }

    /// The types each item of the copy was written in; none before a program writes.
    pub fn written_types(&self) -> Vec<Vec<String>> {
        let copy = self.copy.lock().unwrap();
        let Some((items, _)) = copy.as_ref() else {
            return Vec::new();
        };
        items
            .iter()
            .map(|item| {
                item.representations
                    .iter()
                    .map(|(mime, _)| mime.clone())
                    .collect()
            })
            .collect()
    }

    /// Pastes `item` in `mime`, given or promised, as another application would: none when it
    /// was promised and the program gave nothing in time.
    pub fn paste(&self, item: usize, mime: &str) -> Option<Vec<u8>> {
        let produced = {
            let copy = self.copy.lock().unwrap();
            let (items, _) = copy.as_ref().expect("a copy");
            match form(&items[item], mime) {
                Content::Data(bytes) => return Some(bytes.clone()),
                Content::Promise(produce) => produce(),
                Content::File(_) | Content::FilePromise { .. } => panic!("{mime} is a file"),
            }
        };
        block_on(produced)
    }

    /// The file `item` carries in `mime`, by its path on the machine.
    pub fn file(&self, item: usize, mime: &str) -> PathBuf {
        let copy = self.copy.lock().unwrap();
        let (items, _) = copy.as_ref().expect("a copy");
        match form(&items[item], mime) {
            Content::File(path) => path.clone(),
            _ => panic!("{mime} is no file"),
        }
    }

    /// Pastes the file `item` promises in `mime`, as a receiver would: its bytes, once the
    /// program has made it whole.
    pub fn paste_promised_file(&self, item: usize, mime: &str) -> io::Result<Vec<u8>> {
        let file = PastedFile::default();
        let produced = {
            let copy = self.copy.lock().unwrap();
            let (items, _) = copy.as_ref().expect("a copy");
            match form(&items[item], mime) {
                Content::FilePromise { produce, .. } => produce(Box::new(file.clone())),
                _ => panic!("{mime} is no promised file"),
            }
        };
        block_on(produced)?;
        Ok(file.0.lock().unwrap().clone())
    }
}

/// A promised file as a receiver gets it: the bytes written into it, in order.
#[derive(Clone, Default)]
struct PastedFile(Arc<Mutex<Vec<u8>>>);

impl FileSink for PastedFile {
    fn write<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Box::pin(ready(Ok(())))
    }
}

/// `item`'s form in `mime`.
fn form<'a>(item: &'a CopiedItem, mime: &str) -> &'a Content {
    let found = item
        .representations
        .iter()
        .find(|(offered, _)| offered == mime);
    &found.unwrap_or_else(|| panic!("no {mime}")).1
}

impl ShellClipboard for TestClipboard {
    fn items(&self) -> ClipboardItemsFuture {
        Box::pin(ready(self.listed.lock().unwrap().clone()))
    }

    /// Holds the copy, and lists one item for each written, in the types written, a file's as
    /// `text/uri-list`.
    fn write(&self, items: Vec<CopiedItem>, options: CopyOptions) -> ClipboardWriteFuture {
        let listed = self.hold(items.iter().map(ListedCopy::of).collect());
        *self.copy.lock().unwrap() = Some((items, options));
        Box::pin(ready(Ok(listed)))
    }
}

/// An item as the clipboard lists it: `item`, read only while the copy it is of is the one on
/// the clipboard.
struct OfCopy {
    item: Arc<dyn ShellClipboardItem>,
    copy: u64,
    copies: Arc<AtomicU64>,
}

impl OfCopy {
    fn of(
        item: Arc<dyn ShellClipboardItem>,
        copy: u64,
        copies: &Arc<AtomicU64>,
    ) -> Arc<dyn ShellClipboardItem> {
        Arc::new(OfCopy {
            item,
            copy,
            copies: Arc::clone(copies),
        })
    }
}

impl ShellClipboardItem for OfCopy {
    fn types(&self) -> &[String] {
        self.item.types()
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        if self.copies.load(Ordering::SeqCst) != self.copy {
            return Box::pin(ready(Err(ClipboardError::Changed)));
        }
        self.item.read(mime)
    }
}

fn listed_types(item: &CopiedItem) -> Vec<String> {
    let mut types: Vec<String> = Vec::new();
    for (mime, content) in &item.representations {
        let offered = match content {
            Content::Data(_) | Content::Promise(_) => mime.as_str(),
            Content::File(_) | Content::FilePromise { .. } => "text/uri-list",
        };
        if !types.iter().any(|known| known == offered) {
            types.push(offered.to_owned());
        }
    }
    types
}

/// An item of a program's copy as the test's clipboard listed it: its forms given at once read
/// as given, a promise or a file through the test's paste methods instead.
struct ListedCopy {
    types: Vec<String>,
    given: Vec<(String, Vec<u8>)>,
}

impl ListedCopy {
    fn of(item: &CopiedItem) -> Arc<dyn ShellClipboardItem> {
        let given = item
            .representations
            .iter()
            .filter_map(|(mime, content)| match content {
                Content::Data(bytes) => Some((mime.clone(), bytes.clone())),
                Content::Promise(_) | Content::File(_) | Content::FilePromise { .. } => None,
            });
        Arc::new(ListedCopy {
            types: listed_types(item),
            given: given.collect(),
        })
    }
}

impl ShellClipboardItem for ListedCopy {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let given = self.given.iter().find(|(offered, _)| offered == mime);
        let read = given.map_or(Err(ClipboardError::Unsupported), |(_, bytes)| {
            Ok(bytes.clone())
        });
        Box::pin(ready(read))
    }
}

/// Text another application copied: an item offered in `text/plain` alone.
pub struct CopiedText {
    text: String,
    types: [String; 1],
}

impl CopiedText {
    pub fn item(text: &str) -> Arc<dyn ShellClipboardItem> {
        Arc::new(CopiedText {
            text: text.to_owned(),
            types: ["text/plain".to_owned()],
        })
    }
}

impl ShellClipboardItem for CopiedText {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let read = if mime == "text/plain" {
            Ok(self.text.clone().into_bytes())
        } else {
            Err(ClipboardError::NotFound)
        };
        Box::pin(ready(read))
    }
}

/// A file another application copied: an item offered in `text/uri-list` alone, as its `file:`
/// URL.
pub struct CopiedFile {
    url: String,
    types: [String; 1],
}

impl CopiedFile {
    pub fn item(path: &std::path::Path) -> Arc<dyn ShellClipboardItem> {
        let url = url::Url::from_file_path(path).expect("an absolute path");
        Arc::new(CopiedFile {
            url: format!("{url}\r\n"),
            types: ["text/uri-list".to_owned()],
        })
    }
}

impl ShellClipboardItem for CopiedFile {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let read = if mime == "text/uri-list" {
            Ok(self.url.clone().into_bytes())
        } else {
            Err(ClipboardError::NotFound)
        };
        Box::pin(ready(read))
    }
}
