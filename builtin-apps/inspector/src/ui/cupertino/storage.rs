//! The storage tab on a phone, as P1 draws a drill-down list: the program's mounts, a tile each
//! with its path, what it is, whether the program may change it and where it is on this computer;
//! a tap opens a folder in place, its entries a tile each with the size and time the system keeps;
//! and the way back goes one folder up, to the mounts and then to the report.

use waft::{
    App, AsyncCallback, BuildContext, Entity, Handle, IntoWidget, Listener, State, StateData,
    StatefulWidget, Task, Text, WidgetRef,
};
use waft_cupertino::{CupertinoListTile, CupertinoListTileChevron, CupertinoNavigationBar};
use wasdk::inspect::storage::{Entry, Mount};

use super::{back, back_by, note, scaffold, section};
use crate::clock;
use crate::state::page::{ProgramPage, Tab, open_storage_folder, show_tab};
use crate::state::storage::{Load, Storage};
use crate::words;

/// The storage page of the program chosen.
pub(super) fn storage_page(page: &Entity<ProgramPage>) -> WidgetRef {
    StoragePage { page: page.clone() }.into_widget()
}

/// The drill-down list, and where it is: no path for the mounts, a folder's path for its entries.
#[derive(Debug)]
struct StoragePage {
    page: Entity<ProgramPage>,
}

impl StatefulWidget for StoragePage {
    type State = StoragePageState;

    fn create_state(&self) -> StoragePageState {
        StoragePageState {
            state: StateData::new(),
            path: None,
            stack: Vec::new(),
        }
    }
}

struct StoragePageState {
    state: StateData<StoragePage>,
    /// The folder whose entries show; none for the mounts.
    path: Option<String>,
    /// The folders opened before it, the last being its parent.
    stack: Vec<String>,
}

impl State for StoragePageState {
    type Widget = StoragePage;
    waft::state_accessors!();

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let page = self.widget(app).page.clone();
        let path = app.get(self).path.clone();
        let tiles = {
            let read = page.read(app);
            let storage = read.storage();
            match &path {
                None => mounts(storage, page.clone(), self),
                Some(path) => entries(app, storage, path, page.clone(), self),
            }
        };
        let leading = match &path {
            None => back(words::REPORT, &page, |app, page| {
                show_tab(app, page, Tab::Report);
            }),
            Some(_) => {
                let to = app
                    .get(self)
                    .stack
                    .last()
                    .cloned()
                    .unwrap_or_else(|| words::STORAGE.to_owned());
                back_by(to, popping(self))
            }
        };
        let title = path.unwrap_or_else(|| words::STORAGE.to_owned());
        let bar = CupertinoNavigationBar::new()
            .leading(leading)
            .middle(Text::new(title));
        scaffold(bar, vec![section(None, None, tiles)])
    }
}

/// The way back one folder up, to the mounts.
fn popping(this: Handle<StoragePageState>) -> Listener {
    Listener::new(move |app| {
        this.set_state(app, |state| state.path = state.stack.pop());
    })
}

/// The mounts, a tile each.
fn mounts(
    storage: &Storage,
    page: Entity<ProgramPage>,
    this: Handle<StoragePageState>,
) -> Vec<WidgetRef> {
    match storage.mounts() {
        Load::Looking => vec![note(words::READING)],
        Load::Failed(why) => vec![note(&format!("{} {why}", words::STORAGE_UNAVAILABLE))],
        Load::Ready(mounts) if mounts.is_empty() => vec![note(words::NO_MOUNTS)],
        Load::Ready(mounts) => mounts
            .iter()
            .map(|mount| mount_tile(mount, page.clone(), this))
            .collect(),
    }
}

/// A mount's tile, which opens its folder.
fn mount_tile(
    mount: &Mount,
    page: Entity<ProgramPage>,
    this: Handle<StoragePageState>,
) -> WidgetRef {
    let said = format!(
        "{} · {} · {}",
        words::mount_kind(mount.kind),
        words::written(mount.writable),
        mount.place
    );
    CupertinoListTile::new(Text::new(mount.path.clone()))
        .subtitle(Text::new(said))
        .trailing(CupertinoListTileChevron::new())
        .on_tap(opening(page, mount.path.clone(), this))
        .into_widget()
}

/// The entries of `path`, a tile each; a folder opens it, a file says what it is.
fn entries(
    app: &App,
    storage: &Storage,
    path: &str,
    page: Entity<ProgramPage>,
    this: Handle<StoragePageState>,
) -> Vec<WidgetRef> {
    match storage.entries(path) {
        None | Some(Load::Looking) => vec![note(words::READING)],
        Some(Load::Failed(why)) => vec![note(&format!("{} {why}", words::READ_FAILED))],
        Some(Load::Ready(entries)) => entries
            .iter()
            .map(|entry| entry_tile(app, entry, path, page.clone(), this))
            .collect(),
    }
}

/// An entry's tile.
fn entry_tile(
    app: &App,
    entry: &Entry,
    parent: &str,
    page: Entity<ProgramPage>,
    this: Handle<StoragePageState>,
) -> WidgetRef {
    let path = if parent == "/" {
        format!("/{}", entry.name)
    } else {
        format!("{parent}/{}", entry.name)
    };
    let said = match (entry.size, entry.modified) {
        (Some(size), Some(modified)) => {
            format!(
                "{} · {}",
                words::bytes(size),
                clock::said_when(app, modified)
            )
        }
        (Some(size), None) => words::bytes(size),
        (None, Some(modified)) => clock::said_when(app, modified),
        (None, None) => String::new(),
    };
    let mut tile = CupertinoListTile::new(Text::new(entry.name.clone())).subtitle(Text::new(said));
    if entry.is_folder {
        tile = tile
            .trailing(CupertinoListTileChevron::new())
            .on_tap(opening(page, path, this));
    }
    tile.into_widget()
}

/// What a tap that opens `path` does: opens it, then shows it.
fn opening(
    page: Entity<ProgramPage>,
    path: String,
    this: Handle<StoragePageState>,
) -> AsyncCallback {
    std::rc::Rc::new(move |app: &mut App| {
        open_storage_folder(app, &page, &path);
        this.set_state(app, |state| {
            if let Some(current) = state.path.replace(path.clone()) {
                state.stack.push(current);
            }
        });
        Task::ready(())
    })
}
