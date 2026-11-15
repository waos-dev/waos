//! What programs are written with: the generated bindings of the WIT packages `wasdk:platform` and
//! `wasdk:media`, under the program's world in `wasdk:program`, passed through, and what
//! generation cannot produce.
//!
//! Each module is the interface of that name in `wit/`, as wit-bindgen generates it: records and
//! enums as the WIT names them, a resource as a struct with methods and its `constructor` as
//! `new`, a plain function as a function of the module, a `borrow<T>` parameter as `&T`,
//! `async func` as `async fn`, `stream<T>` as a [`StreamReader`], read item by item with
//! `next().await`, or many at once with `read(buffer).await`, which says with a [`StreamResult`]
//! whether the writer is gone, a stream the program gives made with [`wit_stream::new`] and
//! written through its [`StreamWriter`], and `future<T>` as a [`FutureReader`], awaited for its
//! value, one the program gives made with [`wit_future::new`] and written through its
//! [`FutureWriter`]. [`webgpu`] is `wasi:webgpu`'s own interface, [`monotonic_clock`] the clock a
//! view's `Instant` is on, [`system_clock`] the one whose `instant` a notification is scheduled
//! at, [`filesystem`] `wasi:filesystem/types` 0.3, whose `descriptor` a machine's root and a
//! watched folder are, [`preopens`] `wasi:filesystem/preopens` 0.3, the root `/` a program opens
//! its own folders and its mounts from as `std::fs` does,
//! [`http`] WASI 0.3's HTTP client, `wasi:http/client` with its `types`, whose `send` a program
//! awaits and which the system's proxy and certificate check serve, and [`sockets`] WASI 0.3's
//! sockets, `wasi:sockets/types` and `ip-name-lookup`, whose connections, streams and lookups a
//! program awaits. `std::net`'s sockets are the same, called by `std`, under the same rules.
//!
//! The invariant: programs write against the generated bindings, and this crate adds only what
//! generation cannot produce: [`main`], the entry; [`worker`], a worker's entry, which the
//! program's one `worker-entry` export finds by name; [`spawn`], the executor every call runs on,
//! and [`block_on`], which runs a future to its end where code that cannot await needs its answer;
//! the runtime's stream and future types the signatures name; and [`keys`], Flutter's names of
//! the numbers a key event carries, as plain constants. No hand-written wrapper over a generated
//! type. It names nothing of the engine or the capabilities, and pins one wit-bindgen
//! per release.
//!
//! ```ignore
//! use wasdk::views::{self, ViewChange};
//!
//! #[wasdk::main]
//! async fn main() {
//!     let mut changes = views::view_changes();
//!     for view in views::views() {
//!         println!("given view {}", view.id());
//!     }
//!     while let Some(change) = changes.next().await {
//!         if let ViewChange::Added(view) = change {
//!             println!("view {} arrived for request {:?}", view.id(), view.request_id());
//!         }
//!     }
//! }
//! ```

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        // The crate's copy of the repository's `wit/`, made by `cargo xtask sync-wit`.
        path: "wit",
        // The program's world without its `wasi:cli/run`, which Rust's `std` exports.
        world: "wasdk:program/program-base",
        generate_all,
        pub_export_macro: true,
    });
}

#[doc(hidden)]
#[path = "entry.rs"]
pub mod __entry;

pub mod keys;

pub use bindings::wasdk::media::{
    cameras, caption_settings, image_codecs, media_assets, media_players, video_frames,
};
pub use bindings::wasdk::platform::{
    audio, clipboard, file_dialogs, file_watching, fonts, gpu_interop, hardware_buffers, lifecycle,
    machines, menus, notifications, opener, permissions, screens, system, trust, views, windows,
    workers,
};
pub use bindings::wasi::clocks::{monotonic_clock, system_clock};
pub use bindings::wasi::filesystem::preopens;
pub use bindings::wasi::filesystem::types as filesystem;
pub use bindings::wasi::http;
pub use bindings::wasi::sockets;
pub use bindings::wasi::webgpu::webgpu;
pub use bindings::{wit_future, wit_stream};
pub use wasdk_macros::{main, worker};
pub use wit_bindgen::rt::async_support::{
    FutureReader, FutureWriter, StreamReader, StreamResult, StreamWriter, block_on,
    spawn_local as spawn, yield_async,
};
