use std::future::{Future, pending, ready};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, Weak};
use std::task::{Context, Poll, Waker};

use futures::StreamExt;
use futures::executor::block_on;
use waos_capabilities::{Answer, ChangedFuture, FileSystemGrant, NetworkGrant, Question};

use super::*;
use crate::access::Told;
use crate::bindings::wasdk::platform::views::{Insets, Metrics};
use crate::clock::Clock;
use crate::lifecycle::{Lifecycle, Presence};
use crate::screens::Screens;
use crate::shell::tests::Headless;
use crate::shell::{
    ClipboardItemsFuture, ClipboardReadFuture, ClipboardWriteFuture, Content, PlacedView, ShellView,
};
use crate::views::{ProgramView, ScreenRefreshes};

/// An item on the test's clipboard: its types, each read as its own name, until the clipboard
/// it was listed on holds another copy.
pub(super) struct TestItem {
    types: Vec<String>,
    /// The copy it is of, among those its clipboard has held.
    copy: u64,
    copies: Arc<AtomicU64>,
}

impl TestItem {
    /// An item of a clipboard nothing else is copied onto.
    pub(super) fn offering(types: &[&str]) -> Arc<dyn ShellClipboardItem> {
        TestItem::of_copy(types, 0, Arc::default())
    }

    fn of_copy(types: &[&str], copy: u64, copies: Arc<AtomicU64>) -> Arc<dyn ShellClipboardItem> {
        Arc::new(TestItem {
            types: types.iter().map(|mime| mime.to_string()).collect(),
            copy,
            copies,
        })
    }
}

impl ShellClipboardItem for TestItem {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let read = if self.copies.load(Ordering::SeqCst) != self.copy {
            Err(ClipboardError::Changed)
        } else if self.types.iter().any(|offered| offered == mime) {
            Ok(mime.as_bytes().to_vec())
        } else {
            Err(ClipboardError::NotFound)
        };
        Box::pin(ready(read))
    }
}

/// The platform's clipboard of a test: the types of each item on it, and how many copies it has
/// held. A write lists one item of each item written, in the types written.
#[derive(Default)]
pub(super) struct TestClipboard {
    held: Mutex<Vec<Vec<String>>>,
    copies: Arc<AtomicU64>,
}

impl TestClipboard {
    /// Another writer copies items offered in `items`' types; answers them as the shell lists
    /// them, for a test whose shell tells the engine of the change.
    pub(super) fn copy(&self, items: &[&[&str]]) -> Vec<Arc<dyn ShellClipboardItem>> {
        let types = |item: &&[&str]| item.iter().map(|mime| mime.to_string()).collect();
        *self.held.lock().unwrap() = items.iter().map(types).collect();
        self.copies.fetch_add(1, Ordering::SeqCst);
        self.listed()
    }

    fn listed(&self) -> Vec<Arc<dyn ShellClipboardItem>> {
        let copy = self.copies.load(Ordering::SeqCst);
        let held = self.held.lock().unwrap();
        let item = |types: &Vec<String>| {
            let types: Vec<&str> = types.iter().map(String::as_str).collect();
            TestItem::of_copy(&types, copy, Arc::clone(&self.copies))
        };
        held.iter().map(item).collect()
    }
}

impl ShellClipboard for TestClipboard {
    fn items(&self) -> ClipboardItemsFuture {
        Box::pin(ready(self.listed()))
    }

    fn write(&self, items: Vec<CopiedItem>, _options: CopyOptions) -> ClipboardWriteFuture {
        let written: Vec<Vec<&str>> = items
            .iter()
            .map(|item| {
                let types = item.representations.iter();
                types.map(|(mime, _)| mime.as_str()).collect()
            })
            .collect();
        let written: Vec<&[&str]> = written.iter().map(Vec::as_slice).collect();
        Box::pin(ready(Ok(self.copy(&written))))
    }
}

/// The engine's clipboard over a test's, which the test copies onto as another writer would.
fn platform() -> (PlatformClipboard, Arc<TestClipboard>) {
    let shell = Arc::new(TestClipboard::default());
    let platform = PlatformClipboard::new(Some(Arc::clone(&shell) as Arc<dyn ShellClipboard>));
    (platform, shell)
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
        ScreenRefreshes::new(
            Arc::new(Headless::default()),
            Screens::new(Vec::new()),
            crate::running::Programs::default(),
        ),
        platform.clone(),
        crate::screenshots::tests::screenshots(),
        Weak::new(),
    );
    let request = views.new_request(None, String::new());
    let view = views.add(request, placed_view());
    let clipboard = ProgramClipboard::new(
        platform.clone(),
        ProgramAccess::new(switches),
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
    let (platform, shell) = platform();
    shell.copy(&[&["text/plain"]]);
    let (clipboard, view) = program(&platform, Arc::default());
    view.presence_changed(FOCUSED);
    let listed = block_on(clipboard.items());
    let read = |item: &ListedItem| block_on(clipboard.read(item, "text/plain"));
    assert!(
        matches!(read(&listed[0]), Err(ClipboardError::Denied)),
        "the switch is off"
    );
    let pasted = block_on(platform.pasted().items());
    assert_eq!(
        read(&pasted[0]).ok(),
        Some(b"text/plain".to_vec()),
        "the user pasted it"
    );
    shell.copy(&[&["text/plain"]]);
    assert!(
        matches!(read(&pasted[0]), Err(ClipboardError::Changed)),
        "what the user pasted was replaced"
    );
}

#[test]
fn a_program_lists_what_another_writer_copied_though_the_shell_told_of_no_change() {
    let (platform, shell) = platform();
    let switches = Arc::new(Switches::default());
    switches.read.store(true, Ordering::SeqCst);
    let (clipboard, view) = program(&platform, switches);
    view.presence_changed(FOCUSED);
    assert!(block_on(clipboard.items()).is_empty(), "nothing copied yet");

    shell.copy(&[&["text/html", "text/plain"]]);
    let listed = block_on(clipboard.items());
    assert_eq!(types_of(&listed), [["text/html", "text/plain"]]);
    let read = block_on(clipboard.read(&listed[0], "text/plain"));
    assert_eq!(read.ok(), Some(b"text/plain".to_vec()));
    assert_eq!(platform.change(), 0, "and no change was told");
}

#[test]
fn a_paste_is_the_clipboard_as_it_was_when_the_user_pasted() {
    let (platform, shell) = platform();
    let (clipboard, _view) = program(&platform, Arc::default());
    shell.copy(&[&["text/plain"]]);
    let paste = platform.pasted();
    shell.copy(&[&["image/png"]]);

    let pasted = block_on(paste.items());
    assert_eq!(types_of(&pasted), [["text/plain"]], "not the later copy");
    assert!(
        matches!(
            block_on(clipboard.read(&pasted[0], "text/plain")),
            Err(ClipboardError::Changed)
        ),
        "and what the user pasted is gone"
    );
    assert_eq!(
        types_of(&block_on(paste.items())),
        [["text/plain"]],
        "a paste kept lists nothing newer"
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
    let (platform, shell) = platform();
    shell.copy(&[&["text/plain"]]);
    let (clipboard, view) = program(&platform, Arc::default());
    let items = || block_on(clipboard.items());
    assert!(items().is_empty(), "no view has the focus");

    view.presence_changed(FOCUSED);
    assert_eq!(types_of(&items()), [["text/plain"]]);
    view.presence_changed(ON_SCREEN);
    assert!(items().is_empty());
}

#[test]
fn changes_arrive_while_focused_and_one_made_unfocused_as_the_focus_comes() {
    let (platform, _shell) = platform();
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
    let (platform, shell) = platform();
    shell.copy(&[&["text/plain"]]);
    let switches = Arc::new(Switches::default());
    let (clipboard, view) = program(&platform, Arc::clone(&switches));
    view.presence_changed(FOCUSED);
    let items = block_on(clipboard.items());

    let read = || block_on(clipboard.read(&items[0], "text/plain"));
    assert!(matches!(read(), Err(ClipboardError::Denied)));
    switches.read.store(true, Ordering::SeqCst);
    assert_eq!(read().ok(), Some(b"text/plain".to_vec()));
    shell.copy(&[]);
    assert!(matches!(read(), Err(ClipboardError::Changed)));
}

#[test]
fn the_write_switch_is_read_at_each_write() {
    let switches = Arc::new(Switches::default());
    let (platform, _shell) = platform();
    let (clipboard, _view) = program(&platform, Arc::clone(&switches));
    assert!(!clipboard.may_write(&["text/plain"]));
    switches.write.store(true, Ordering::SeqCst);
    assert!(clipboard.may_write(&["text/plain"]));
}

#[test]
fn a_read_and_a_write_the_switches_refuse_are_told_refused_by_them() {
    let (platform, shell) = platform();
    shell.copy(&[&["text/plain"]]);
    let (clipboard, view) = program(&platform, Arc::default());
    view.presence_changed(FOCUSED);
    let listed = block_on(clipboard.items());

    assert!(block_on(clipboard.read(&listed[0], "text/plain")).is_err());
    assert!(!clipboard.may_write(&["text/plain"]));
    assert_eq!(
        clipboard.access.refusals(),
        ["clipboard-read", "clipboard-write"]
    );
    let pasted = block_on(platform.pasted().items());
    assert!(block_on(clipboard.read(&pasted[0], "text/plain")).is_ok());
    assert!(
        matches!(clipboard.access.told().last(), Some(Told::Allowed(..))),
        "a paste is read past the switch, and told allowed"
    );
}

#[test]
fn a_write_is_what_programs_are_told_from_then_on_and_counts_as_a_change() {
    let (platform, _shell) = platform();
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
