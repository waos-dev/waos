//! `wasdk:platform/clipboard`: the platform's clipboard as every program sees it
//! ([`PlatformClipboard`]: the shell that lists and writes it, and what programs are told it
//! changed to), and as one program reaches it ([`ProgramClipboard`]): what is on it and its
//! changes, seen only while one of the program's views has the focus, and its reads and writes, as
//! its switches say at each one, each decided and told through the program's access (`access`). The
//! user's own commands are free of the switches: a [`CopyRequest`], the user's copy or cut, writes
//! once whatever the write switch says, and a [`Paste`], the clipboard as the user pasted it, reads
//! whatever the read switch says, each file pasted mounted read-only at its own path as its
//! `text/uri-list` is read. What a program writes is made into what the shell writes in `written`,
//! and what it put on the clipboard, its `source`, answering the pastes of its promised forms and
//! files while it is there, is `sources`, each paste's answer `answers`. The interface is served to
//! a program by [`host`].
//!
//! What is on the clipboard is asked of the shell when a program lists it and when the user
//! pastes, never answered from what the engine keeps: the platform's clipboard changes without
//! the shell hearing of it at once, and a paste must be what the user sees there. What the
//! engine keeps, the listing the shell last told it, serves two things only: the changes
//! programs are told, and a program's copy being replaced.
//!
//! The invariant: the kept listing changes only by the shell's word, `WaOS::set_clipboard` for
//! another writer's copy and a write's answer for a program's own, so a change counted after a
//! program's write is a copy that replaced it; a switch is read at each read and each write,
//! inside its decision, never copied; and only a paste's items read past the read switch, and
//! only as long as the clipboard is what the user pasted, since the shell's item fails a read
//! once the clipboard has changed since it was listed.

mod answers;
pub(crate) mod host;
mod sources;
mod written;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::watch;
use waos_capabilities::ProgramGrants;

use crate::access::{Asked, ProgramAccess, Refusal};
use crate::bindings::wasdk::platform::clipboard::ClipboardError;
use crate::files::{HandedFiles, machine_paths};
use crate::shell::{ClipboardItemsFuture, CopiedItem, ShellClipboard, ShellClipboardItem};
use crate::views::ProgramViews;

pub(crate) use sources::ProgramSources;
pub use sources::{Request, Source};

/// How a copy is to be treated: what the program set on its `copy-options`, each option off
/// unless set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyOptions {
    /// A password or a code: kept out of previews, clipboard history and sync.
    pub sensitive: bool,
    /// Kept off other devices' clipboards.
    pub this_device_only: bool,
    /// Cleared after this long, if still on the clipboard.
    pub expires_after: Option<Duration>,
}

/// What the shell last told the engine is on the platform's clipboard, and how many times it
/// has told of a change since the engine started.
#[derive(Clone)]
pub(crate) struct Listing {
    items: Arc<[Arc<dyn ShellClipboardItem>]>,
    change: u64,
}

/// One item of the clipboard as a program listed it: the value behind its `clipboard-item`.
pub struct ListedItem {
    item: Arc<dyn ShellClipboardItem>,
    /// Whether the user pasted it: read whatever the read switch says, its files handed over.
    pasted: bool,
}

/// The user's copy or cut, in a view of the program: the value behind its `copy-request`, which
/// writes once whatever the write switch says.
#[derive(Default)]
pub struct CopyRequest {
    written: bool,
}

impl CopyRequest {
    /// Whether this request may write now, which it may once: true the first time it is asked.
    pub(crate) fn take_write(&mut self) -> bool {
        !std::mem::replace(&mut self.written, true)
    }
}

/// The clipboard as the user pasted it, in a view of the program: the value behind its `paste`.
pub struct Paste {
    /// The shell's listing, asked for as the user pasted, on its way or arrived.
    items: Shared<ClipboardItemsFuture>,
}

impl Paste {
    /// What was on the clipboard when the user pasted, each item read as the user's.
    pub(crate) fn items(&self) -> BoxFuture<'static, Vec<ListedItem>> {
        let items = self.items.clone();
        Box::pin(async move { listed(&items.await, true) })
    }
}

impl ListedItem {
    pub(crate) fn types(&self) -> &[String] {
        self.item.types()
    }
}

/// The platform's clipboard as the engine knows it, shared by every program; the value behind a
/// program's `clipboard` handle. Every clone is the same clipboard.
#[derive(Clone)]
pub struct PlatformClipboard(Arc<Platform>);

struct Platform {
    shell: Option<Arc<dyn ShellClipboard>>,
    listing: watch::Sender<Listing>,
}

impl PlatformClipboard {
    /// The clipboard `shell` lists and writes, no change told yet; with none, no program has a
    /// clipboard.
    pub(crate) fn new(shell: Option<Arc<dyn ShellClipboard>>) -> PlatformClipboard {
        let empty = Listing {
            items: Arc::from([]),
            change: 0,
        };
        PlatformClipboard(Arc::new(Platform {
            shell,
            listing: watch::Sender::new(empty),
        }))
    }

    /// Whether the platform has a clipboard.
    pub(crate) fn exists(&self) -> bool {
        self.0.shell.is_some()
    }

    /// What is on the clipboard now, as the user pastes it: the shell is asked here, as the
    /// user pastes, whenever the program reads the answer.
    pub(crate) fn pasted(&self) -> Paste {
        Paste {
            items: self.items().shared(),
        }
    }

    /// What is on the clipboard as this is called, asked of the shell; nothing on a platform
    /// without a clipboard.
    fn items(&self) -> ClipboardItemsFuture {
        match &self.0.shell {
            Some(shell) => shell.items(),
            None => Box::pin(std::future::ready(Vec::new())),
        }
    }

    /// What the shell says is on the clipboard now: a change, which replaces the copy before
    /// it, and which programs are told. Answers the change's number.
    pub(crate) fn set(&self, items: Vec<Arc<dyn ShellClipboardItem>>) -> u64 {
        let mut change = 0;
        self.0.listing.send_modify(|listing| {
            listing.items = Arc::from(items);
            listing.change += 1;
            change = listing.change;
        });
        change
    }

    /// What the shell last told is on the clipboard.
    fn listing(&self) -> Listing {
        self.0.listing.borrow().clone()
    }

    fn changes(&self) -> watch::Receiver<Listing> {
        self.0.listing.subscribe()
    }

    /// The number of the change the shell last told.
    fn change(&self) -> u64 {
        self.0.listing.borrow().change
    }

    /// Replaces the clipboard with `items` through the shell; answers the number of the change
    /// the write is, with what the shell listed after it.
    fn write(
        &self,
        items: Vec<CopiedItem>,
        options: CopyOptions,
    ) -> BoxFuture<'static, Result<u64, ClipboardError>> {
        let platform = self.clone();
        let written = self
            .0
            .shell
            .as_ref()
            .map(|shell| shell.write(items, options));
        Box::pin(async move {
            let Some(written) = written else {
                return Err(ClipboardError::Unsupported);
            };
            Ok(platform.set(written.await?))
        })
    }
}

/// The clipboard as one program reaches it.
pub(crate) struct ProgramClipboard {
    platform: PlatformClipboard,
    /// Where each read and write is decided, by the program's switches.
    access: ProgramAccess,
    views: ProgramViews,
    sources: ProgramSources,
    /// What the files the user pastes are mounted through.
    handed: HandedFiles,
}

impl ProgramClipboard {
    /// The clipboard of the program whose views are `views`, as its switches in `access` let it
    /// read and write, its copies kept in `sources`, the files the user pastes into it mounted
    /// through `handed`.
    pub(crate) fn new(
        platform: PlatformClipboard,
        access: ProgramAccess,
        views: ProgramViews,
        sources: ProgramSources,
        handed: HandedFiles,
    ) -> ProgramClipboard {
        ProgramClipboard {
            platform,
            access,
            views,
            sources,
            handed,
        }
    }

    pub(crate) fn platform(&self) -> &PlatformClipboard {
        &self.platform
    }

    /// What is on the clipboard as this is called, while one of the program's views has the
    /// focus; nothing otherwise.
    pub(crate) fn items(&self) -> BoxFuture<'static, Vec<ListedItem>> {
        if !*self.views.focus().borrow() {
            return Box::pin(std::future::ready(Vec::new()));
        }
        let items = self.platform.items();
        Box::pin(async move { listed(&items.await, false) })
    }

    /// What is on the clipboard at each change from now on, while one of the program's views
    /// has the focus; a change made while none has it, as one gains it.
    pub(crate) fn changes(&self) -> BoxStream<'static, Vec<ListedItem>> {
        let watch = ChangesWatch {
            listing: self.platform.changes(),
            focus: self.views.focus(),
            told: self.platform.change(),
        };
        stream::unfold(watch, ChangesWatch::next_change).boxed()
    }

    /// `item`'s content in `mime`, if the read switch is on or the user pasted the item, and, as
    /// the shell's item says, the clipboard has not changed since it was listed. Each file of a
    /// pasted item's `text/uri-list` is mounted read-only at its own path before the content is
    /// answered, and named in it by that path ([`program_uri_list`]).
    pub(crate) fn read(
        &self,
        item: &ListedItem,
        mime: &str,
    ) -> BoxFuture<'static, Result<Vec<u8>, ClipboardError>> {
        let used = Asked::ClipboardRead {
            form: mime,
            pasted: item.pasted,
        };
        let pasted_or_switched_on = |grants: &dyn ProgramGrants| {
            if item.pasted || grants.clipboard_read() {
                Ok(())
            } else {
                Err(Refusal::by("clipboard-read"))
            }
        };
        if self.access.decide(used, pasted_or_switched_on).is_err() {
            return Box::pin(std::future::ready(Err(ClipboardError::Denied)));
        }
        let read = item.item.read(mime);
        if !(item.pasted && mime == URI_LIST) {
            return read;
        }
        let handed = self.handed.clone();
        Box::pin(async move { Ok(program_uri_list(&read.await?, &handed)) })
    }

    /// Whether the program may write `forms` to the clipboard: whether the write switch is on.
    pub(crate) fn may_write(&self, forms: &[&str]) -> bool {
        let switched_on = |grants: &dyn ProgramGrants| {
            if grants.clipboard_write() {
                Ok(())
            } else {
                Err(Refusal::by("clipboard-write"))
            }
        };
        self.access
            .decide(Asked::ClipboardWrite { forms }, switched_on)
            .is_ok()
    }

    pub(crate) fn sources(&self) -> &ProgramSources {
        &self.sources
    }
}

/// Each of `items`, as a program lists it, or the user pasted it where `pasted`.
fn listed(items: &[Arc<dyn ShellClipboardItem>], pasted: bool) -> Vec<ListedItem> {
    let listed = items.iter().map(|item| ListedItem {
        item: Arc::clone(item),
        pasted,
    });
    listed.collect()
}

/// The type a file on the clipboard is read in: its `file:` URL, one per line.
const URI_LIST: &str = "text/uri-list";

/// `uris`, a `text/uri-list` of the machine, as the program reads it: each file handed to the
/// program read-only at its own path, and its URL naming that path, as a program names this
/// computer's paths ([`machine_paths`]): on Windows `file:///C:/Users/me/a.txt` is
/// `file:///c/Users/me/a.txt`. A file a program has no path for is left out; comments and other
/// URLs are kept as they are.
fn program_uri_list(uris: &[u8], handed: &HandedFiles) -> Vec<u8> {
    let mut list = String::new();
    for line in String::from_utf8_lossy(uris).lines() {
        let named = match machine_file(line) {
            Some(file) => program_file_url(&file).inspect(|_| handed.hand(&file)),
            None => Some(line.to_owned()),
        };
        if let Some(named) = named {
            list.push_str(&named);
            list.push_str("\r\n");
        }
    }
    list.into_bytes()
}

/// The machine's file a line of a `text/uri-list` names by its `file:` URL; none for a comment, an
/// empty line or another URL.
fn machine_file(line: &str) -> Option<PathBuf> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    url::Url::parse(line).ok()?.to_file_path().ok()
}

/// The `file:` URL of `file`, a file of the machine, at the path a program names it by; none for
/// one a program has no path for.
fn program_file_url(file: &Path) -> Option<String> {
    let named = machine_paths::in_program(file)?;
    let mut url = url::Url::parse("file:///").expect("a file URL");
    url.set_path(named.to_str()?);
    Some(url.into())
}

/// One stream of a program's clipboard changes: the clipboard and the focus as they change, and
/// the change it last told.
struct ChangesWatch {
    listing: watch::Receiver<Listing>,
    focus: watch::Receiver<bool>,
    told: u64,
}

impl ChangesWatch {
    /// The next change not told, once one of the program's views has the focus; the stream ends
    /// with the engine or the program.
    async fn next_change(mut self) -> Option<(Vec<ListedItem>, ChangesWatch)> {
        loop {
            let listing = self.listing.borrow_and_update().clone();
            let focused = *self.focus.borrow_and_update();
            if focused && listing.change != self.told {
                self.told = listing.change;
                return Some((listed(&listing.items, false), self));
            }
            tokio::select! {
                changed = self.listing.changed() => changed.ok()?,
                changed = self.focus.changed() => changed.ok()?,
            }
        }
    }
}

#[cfg(test)]
mod tests;
