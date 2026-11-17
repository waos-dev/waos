//! The storage tab, as WinUI's `TreeView` draws a folder: each mount of the program at the top, a
//! row of its path, what it is, whether the program may change it and where it is on this
//! computer; each folder opening lazily, its entries a row each with the size and time the system
//! keeps; and a folder that cannot be read saying why in its place.
//!
//! The invariant: the tree is the page's storage state and no other, so a folder is read once and
//! a tab switch reads nothing again.

use waft::{App, Entity, Expanded, IntoWidget, Row, SizedBox, Text, WidgetRef};
use waft_winui::{
    FluentSymbol, FontIcon, TextBlockStyle, ThemeResources, TreeView, TreeViewNode,
    TreeViewSelectionMode,
};
use wasdk::inspect::storage::{Entry, Mount};

use super::{caption, code_line, said};
use crate::clock;
use crate::state::page::{ProgramPage, close_storage_folder, open_storage_folder};
use crate::state::storage::{Load, Storage};
use crate::words;

/// The storage of the program chosen, its mounts and the folders opened so far.
pub(super) fn storage(
    app: &mut App,
    page: &Entity<ProgramPage>,
    resources: &ThemeResources,
) -> WidgetRef {
    let read = page.read(app);
    let state = read.storage();
    match state.mounts() {
        Load::Looking => said(words::READING, resources),
        Load::Failed(why) => said(&format!("{} {why}", words::STORAGE_UNAVAILABLE), resources),
        Load::Ready(mounts) if mounts.is_empty() => said(words::NO_MOUNTS, resources),
        Load::Ready(mounts) => {
            let nodes: Vec<TreeViewNode> = mounts
                .iter()
                .map(|mount| mount_node(app, page, state, mount, resources))
                .collect();
            let page = page.clone();
            let opening = page.clone();
            TreeView::new(nodes, Vec::new(), |_app, _| {})
                .selection_mode(TreeViewSelectionMode::None)
                .can_drag_items(false)
                .can_reorder_items(false)
                .allow_drop(false)
                .expanding(move |app, event| open_storage_folder(app, &opening, &event.node))
                .collapsed(move |app, event| close_storage_folder(app, &page, &event.node))
                .into_widget()
        }
    }
}

/// One mount, its folder's entries beneath it once it is opened.
fn mount_node(
    app: &App,
    page: &Entity<ProgramPage>,
    state: &Storage,
    mount: &Mount,
    resources: &ThemeResources,
) -> TreeViewNode {
    let row = Row::new()
        .spacing(8.0)
        .children([
            FontIcon::symbol(FluentSymbol::Folder)
                .font_size(14.0)
                .into_widget(),
            Text::new(mount.path.clone())
                .style(super::code_style(
                    TextBlockStyle::BodyStrong,
                    resources.text_fill_color_primary,
                    resources,
                ))
                .into_widget(),
            caption(words::mount_kind(mount.kind), resources),
            caption(words::written(mount.writable), resources),
            Expanded::new(code_line(
                &mount.place,
                TextBlockStyle::Caption,
                resources.text_fill_color_secondary,
                resources,
            ))
            .into_widget(),
        ])
        .into_widget();
    folder_node(app, page, state, &mount.path, row, resources)
}

/// A folder's node, its entries beneath it once they are read.
fn folder_node(
    app: &App,
    page: &Entity<ProgramPage>,
    state: &Storage,
    path: &str,
    row: WidgetRef,
    resources: &ThemeResources,
) -> TreeViewNode {
    let expanded = state.is_expanded(path);
    let (children, unrealized) = match state.entries(path) {
        Some(Load::Ready(entries)) => (
            entries
                .iter()
                .map(|entry| entry_node(app, page, state, path, entry, resources))
                .collect(),
            false,
        ),
        Some(Load::Looking) => (
            vec![TreeViewNode::text(
                format!("reading:{path}"),
                words::READING,
            )],
            true,
        ),
        Some(Load::Failed(why)) => (
            vec![TreeViewNode::text(
                format!("failed:{path}"),
                format!("{} {why}", words::READ_FAILED),
            )],
            false,
        ),
        None => (Vec::new(), true),
    };
    TreeViewNode::new(path.to_owned(), row)
        .children(children)
        .is_expanded(expanded)
        .has_unrealized_children(unrealized)
}

/// One entry of `parent`: a folder node, or a file's row.
fn entry_node(
    app: &App,
    page: &Entity<ProgramPage>,
    state: &Storage,
    parent: &str,
    entry: &Entry,
    resources: &ThemeResources,
) -> TreeViewNode {
    let path = if parent == "/" {
        format!("/{}", entry.name)
    } else {
        format!("{parent}/{}", entry.name)
    };
    let mut row = vec![
        FontIcon::symbol(if entry.is_folder {
            FluentSymbol::Folder
        } else {
            FluentSymbol::Document
        })
        .font_size(14.0)
        .into_widget(),
        Text::new(entry.name.clone()).into_widget(),
        Expanded::new(SizedBox::new()).into_widget(),
    ];
    if let Some(size) = entry.size {
        row.push(
            Text::new(words::bytes(size))
                .style(super::code_style(
                    TextBlockStyle::Caption,
                    resources.text_fill_color_secondary,
                    resources,
                ))
                .into_widget(),
        );
    }
    if let Some(modified) = entry.modified {
        row.push(caption(&clock::said_when(modified.seconds), resources));
    }
    let row = Row::new().spacing(8.0).children(row).into_widget();
    if entry.is_folder {
        folder_node(app, page, state, &path, row, resources)
    } else {
        TreeViewNode::new(path, row)
    }
}
