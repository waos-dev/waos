//! The docs' copy. `packages/waos-site/content` is the one place the docs are edited, and the
//! site renders them; `cargo waos` carries the same files, built into it, so `cargo waos docs`
//! answers with the docs of its own version. The copy is in `waos-cli`'s folder because a
//! published crate carries only what is under it.
//!
//! The invariant a later editor will break: the copy is never edited. `cargo xtask sync-docs`
//! replaces it whole, and `tests/docs_copy.rs` names every file that differs from the site's.

use crate::folder_copy::FolderCopy;

/// The site's content in `waos-cli`, less the reference's pages for each interface, which the
/// site writes from the WIT at each build and nothing commits.
pub const COPY: FolderCopy = FolderCopy {
    source: "packages/waos-site/content",
    destination: "crates/waos-cli/content",
    left_out: &["docs/reference/wasdk-"],
};
