//! Waft's Cupertino gallery as a program: an index of entries, each opening one screen that
//! demonstrates one Cupertino widget. The gallery is the crate Waft's own binary runs,
//! unchanged; what this file adds is the entry, `waft-embedder-wasdk`'s [`run`] in place of the
//! host `#[waft::main]` starts natively. The gallery asks for its window, since the desktop's
//! shell gives a program none, and quits when the user closes it.
//!
//! ```text
//! cargo build -p gallery --target wasm32-wasip2
//! cargo waos run -p gallery
//! ```
//!
//! [`run`]: waft_embedder_wasdk::run

use cupertino_gallery_lib::run_gallery;

#[wasdk::main]
async fn main() {
    waft_embedder_wasdk::run(|app| run_gallery(app, None)).await;
}
