use std::future::{Future, pending, ready};
use std::pin::pin;
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

use futures::StreamExt;
use futures::executor::block_on;
use waos_capabilities::{Answer, ChangedFuture, FileSystemGrant, NetworkGrant, Question};

use super::*;
use crate::bindings::wasdk::platform::views::{Insets, Metrics};
use crate::clock::Clock;
use crate::lifecycle::{Lifecycle, Presence};
use crate::screens::Screens;
use crate::shell::tests::Headless;
use crate::shell::{ClipboardReadFuture, ClipboardWriteFuture, Content, PlacedView, ShellView};
use crate::views::{ProgramView, ScreenRefreshes};

/// An item on the test's clipboard: its types, each read as its own name.
pub(super) struct TestItem(Vec<String>);

impl TestItem {
    pub(super) fn offering(types: &[&str]) -> Arc<dyn ShellClipboardItem> {
        Arc::new(TestItem(
            types.iter().map(|mime| mime.to_string()).collect(),
        ))
    }
}

impl ShellClipboardItem for TestItem {
    fn types(&self) -> &[String] {
        &self.0
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let read = if self.0.iter().any(|offered| offered == mime) {
            Ok(mime.as_bytes().to_vec())
        } else {
            Err(ClipboardError::NotFound)
        };
        Box::pin(ready(read))
    }
}

/// A shell whose write lists one item of each item written, in the types written.
pub(super) struct ListingShell;

impl ShellClipboard for ListingShell {
    fn write(&self, items: Vec<CopiedItem>, _options: CopyOptions) -> ClipboardWriteFuture {
        let listed = items
            .iter()
            .map(|item| {
                let types: Vec<&str> = item
                    .representations
                    .iter()
                    .map(|(mime, _)| mime.as_str())
                    .collect();
                TestItem::offering(&types)
            })
            .collect();
        Box::pin(ready(Ok(listed)))
    }
}

/// The clipboard switches, which a test turns as the user would; nothing else is granted.
#[derive(Default)]
struct Switches {
    read: AtomicBool,
    write: AtomicBool,
}

impl ProgramGrants for Switches {
    fn network(&self) -> NetworkGrant {
        crate::permissions::NothingGranted.network()
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        self.read.load(Ordering::SeqCst)
    }

    fn clipboard_write(&self) -> bool {
        self.write.load(Ordering::SeqCst)
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant { mounts: Vec::new() }
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        None
    }

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        None
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}

/// A program with one view, on screen and not focused, reaching `platform` as `switches` say.
fn program(
    platform: &PlatformClipboard,
    switches: Arc<Switches>,
) -> (ProgramClipboard, ProgramView) {
    let views = ProgramViews::new(
        Clock::start(),
        Lifecycle::default(),
        ScreenRefreshes::new(Arc::new(Headless::default()), Screens::new(Vec::new())),
        platform.clone(),
        crate::screenshots::tests::screenshots(),
        Weak::new(),
    );
    let view = views.add(placed_view());
    let clipboard = ProgramClipboard::new(
        platform.clone(),
        switches,
        views,
        ProgramSources::default(),
        HandedFiles::default(),
    );
    (clipboard, view)
}

fn placed_view() -> PlacedView {
    let none = Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    PlacedView {
        view: Arc::new(Headless::default()) as Arc<dyn ShellView>,
        text_input: None,
        window: None,
        metrics: Metrics {
            width: 1,
            height: 1,
            device_pixel_ratio: 1.0,
            padding: none,
            view_padding: none,
            view_insets: none,
        },
        screen: None,
        presence: ON_SCREEN,
        request: None,
    }
}

const ON_SCREEN: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: false,
};

const FOCUSED: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: true,
};

fn types_of(items: &[ListedItem]) -> Vec<Vec<String>> {
    items.iter().map(|item| item.types().to_vec()).collect()
}

/// `future`'s output, if it is ready now.
pub(super) fn now<T>(future: impl Future<Output = T>) -> Option<T> {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

#[test]
fn a_pasted_item_reads_whatever_the_read_switch_says_until_the_clipboard_changes() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    platform.set(vec![TestItem::offering(&["text/plain"])]);
    let (clipboard, view) = program(&platform, Arc::default());
    view.presence_changed(FOCUSED);
    let listed = clipboard.items();
    let read = |item: &ListedItem| block_on(clipboard.read(item, "text/plain"));
    assert!(
        matches!(read(&listed[0]), Err(ClipboardError::Denied)),
        "the switch is off"
    );
    let pasted = platform.pasted().items();
    assert_eq!(
        read(&pasted[0]).ok(),
        Some(b"text/plain".to_vec()),
        "the user pasted it"
    );
    platform.set(vec![TestItem::offering(&["text/plain"])]);
    assert!(
        matches!(read(&pasted[0]), Err(ClipboardError::Changed)),
        "what the user pasted was replaced"
    );
}

#[test]
fn a_copy_request_writes_once() {
    let mut request = CopyRequest::default();
    assert!(request.take_write());
    assert!(!request.take_write());
}

#[test]
fn what_is_on_the_clipboard_is_seen_only_while_a_view_has_the_focus() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    platform.set(vec![TestItem::offering(&["text/plain"])]);
    let (clipboard, view) = program(&platform, Arc::default());
    assert!(clipboard.items().is_empty(), "no view has the focus");

    view.presence_changed(FOCUSED);
    assert_eq!(types_of(&clipboard.items()), [["text/plain"]]);
    view.presence_changed(ON_SCREEN);
    assert!(clipboard.items().is_empty());
}

#[test]
fn changes_arrive_while_focused_and_one_made_unfocused_as_the_focus_comes() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let (clipboard, view) = program(&platform, Arc::default());
    view.presence_changed(FOCUSED);
    let mut changes = clipboard.changes();
    assert!(now(changes.next()).is_none(), "nothing changed yet");

    platform.set(vec![TestItem::offering(&["text/html", "text/plain"])]);
    let told = now(changes.next())
        .flatten()
        .expect("a change, told at once");
    assert_eq!(types_of(&told), [["text/html", "text/plain"]]);

    view.presence_changed(ON_SCREEN);
    platform.set(vec![TestItem::offering(&["image/png"])]);
    platform.set(vec![TestItem::offering(&["text/uri-list"])]);
    assert!(now(changes.next()).is_none(), "unseen without the focus");
    view.presence_changed(FOCUSED);
    let told = now(changes.next())
        .flatten()
        .expect("told as the focus comes");
    assert_eq!(
        types_of(&told),
        [["text/uri-list"]],
        "the clipboard as it is now"
    );
}

#[test]
fn a_read_is_the_read_switchs_and_fails_once_the_clipboard_changed() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    platform.set(vec![TestItem::offering(&["text/plain"])]);
    let switches = Arc::new(Switches::default());
    let (clipboard, view) = program(&platform, Arc::clone(&switches));
    view.presence_changed(FOCUSED);
    let items = clipboard.items();

    let read = || block_on(clipboard.read(&items[0], "text/plain"));
    assert!(matches!(read(), Err(ClipboardError::Denied)));
    switches.read.store(true, Ordering::SeqCst);
    assert_eq!(read().ok(), Some(b"text/plain".to_vec()));
    platform.set(Vec::new());
    assert!(matches!(read(), Err(ClipboardError::Changed)));
}

#[test]
fn the_write_switch_is_read_at_each_write() {
    let switches = Arc::new(Switches::default());
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let (clipboard, _view) = program(&platform, Arc::clone(&switches));
    assert!(!clipboard.may_write());
    switches.write.store(true, Ordering::SeqCst);
    assert!(clipboard.may_write());
}

#[test]
fn a_write_is_the_clipboards_listing_from_then_on_and_counts_as_a_change() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let before = platform.change();
    let item = CopiedItem {
        representations: vec![("text/plain".to_owned(), Content::Data(b"hi".to_vec()))],
    };
    let change = block_on(platform.write(vec![item], CopyOptions::default())).ok();
    let change = change.expect("the shell took the write");
    assert_eq!(change, before + 1);
    assert_eq!(platform.listing().items[0].types(), ["text/plain"]);
}

#[test]
fn a_platform_without_a_clipboard_writes_nothing() {
    let platform = PlatformClipboard::new(None);
    assert!(!platform.exists());
    let written = block_on(platform.write(Vec::new(), CopyOptions::default()));
    assert!(matches!(written, Err(ClipboardError::Unsupported)));
}

#[test]
fn a_flush_or_a_stop_asks_for_promised_forms_and_never_a_promised_file() {
    use super::written::{WrittenForm, WrittenItem, promised_forms};

    let items = [
        WrittenItem(vec![
            ("text/plain".to_owned(), WrittenForm::Promise),
            ("text/html".to_owned(), WrittenForm::Promise),
        ]),
        WrittenItem(vec![(
            "image/png".to_owned(),
            WrittenForm::FilePromise("picture.png".to_owned()),
        )]),
    ];
    let asked = promised_forms(&items);
    let asked: Vec<(u32, &str)> = asked
        .iter()
        .map(|form| (form.item, form.mime.as_str()))
        .collect();
    assert_eq!(asked, [(0, "text/plain"), (0, "text/html")]);
}

#[test]
fn a_pasted_uri_list_names_each_file_by_the_path_a_program_names_it() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("a b.txt");
    std::fs::write(&file, "pasted").unwrap();
    let machine_url = url::Url::from_file_path(&file).unwrap();
    let uris = format!("# a comment\r\n{machine_url}\r\nhttps://example.com/\r\n");
    let handed = HandedFiles::default();

    let listed = String::from_utf8(program_uri_list(uris.as_bytes(), &handed)).unwrap();

    let named = machine_paths::in_program(&file).unwrap();
    let mut program_url = url::Url::parse("file:///").unwrap();
    program_url.set_path(named.to_str().unwrap());
    assert_eq!(
        listed,
        format!("# a comment\r\n{program_url}\r\nhttps://example.com/\r\n")
    );
    assert!(
        program_url.as_str().ends_with("/a%20b.txt"),
        "{program_url}"
    );
    #[cfg(windows)]
    assert!(
        program_url.path().starts_with("/c/"),
        "the drive as a program names it: {program_url}"
    );
}
