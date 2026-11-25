//! What programs are written with: the generated bindings of the WIT packages `wasdk:platform` and
//! `wasdk:media`, under the program's world in `wasdk:program`, passed through, and what
//! generation cannot produce.
//!
//! Each module is the interface of that name in `wit/`, as wit-bindgen generates it: records and
//! enums as the WIT names them, a resource as a struct with methods and its `constructor` as
//! `new`, a plain function as a function of the module, a `borrow<T>` parameter as `&T`,
//! `async func` as `async fn` — wit-bindgen generates owned parameters (`String`, `Vec<T>`) for
//! every `async func` because its future may outlive the caller's borrows, while a plain
//! synchronous `func` borrows (`&str`, `&[T]`) — `stream<T>` as a [`StreamReader`], read item by item with
//! `next().await`, or many at once with `read(buffer).await`, which says with a [`StreamResult`]
//! whether the writer is gone, a stream the program gives made with [`wit_stream::new`] and
//! written through its [`StreamWriter`], and `future<T>` as a [`FutureReader`], awaited for its
//! value, one the program gives made with [`wit_future::new`] and written through its
//! [`FutureWriter`]. [`webgpu`] is `wasi:webgpu`'s own interface, [`monotonic_clock`] the clock a
//! view's `Instant` is on, [`system_clock`] the one whose `instant` a notification is scheduled
//! at, [`filesystem`] `wasi:filesystem/types` 0.3, whose `descriptor` a machine's root and a
//! watched folder are, [`mounts`] what shows a folder the program holds at a path of its own files, [`preopens`] `wasi:filesystem/preopens` 0.3, the root `/` a program opens
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
//! the runtime's stream and future types the signatures name; [`keys`], Flutter's names of
//! the numbers a key event carries, as plain constants; and, with the `raw-window-handle`
//! feature, a view as the window other libraries take, `raw-window-handle`'s traits on
//! [`views::View`] and [`view_of_window_handle`] to read one back. No hand-written wrapper over a
//! generated type. It names nothing of the engine or the capabilities, and pins one wit-bindgen
//! per release.
//!
//! ```ignore
//! use wasdk::views::{self, ViewChange};
//! use wasdk::windows::WindowConfig;
//!
//! #[wasdk::main]
//! async fn main() {
//!     let mut changes = views::view_changes();
//!     let request = views::request_view(&WindowConfig::new(), None).expect("a window");
//!     while let Some(change) = changes.next().await {
//!         if let ViewChange::Added(view) = change {
//!             println!("view {} arrived for request {}", view.id(), view.request_id());
//!             assert_eq!(view.request_id(), request);
//!         }
//!     }
//! }
//! ```

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        // The crate's copy of the repository's `wit/`, made by `cargo xtask sync-wit`.
        path: "wit",
        // The program's world without its `wasi:cli/run`, which Rust's `std` exports, with every
        // role's interfaces: a component imports only what it calls, so a program that never
        // calls a role's functions imports nothing of it, and asks for no role.
        world: "wasdk:program/inspector-base",
        generate_all,
        pub_export_macro: true,
        // Plain blocking calls, though `async` in the WIT: wgpu's backend waits on each in place,
        // inside `Device::poll` or as the program yields, where native wgpu waits on the GPU.
        sync: [
            "import:wasi:webgpu/webgpu@0.3.0-rc.2#[method]gpu-buffer.map-async",
            "import:wasi:webgpu/webgpu@0.3.0-rc.2#[method]gpu-queue.on-submitted-work-done",
        ],
    });
}

#[doc(hidden)]
#[path = "entry.rs"]
pub mod __entry;

pub mod keys;

#[cfg(feature = "raw-window-handle")]
mod window_handle;
#[cfg(feature = "raw-window-handle")]
pub use window_handle::view_of_window_handle;

/// `wasdk:inspect`, what a program with the inspector role sees of the others: the devices and
/// the programs on each, their audit summaries and their live traces. Calling any of it asks for
/// the role, which WaOS asks the user to grant at the program's start; a program not granted it
/// does not start.
#[cfg(feature = "inspector")]
pub use bindings::wasdk::inspect;
pub use bindings::wasdk::media::{
    cameras, caption_settings, image_codecs, media_assets, media_players, video_decoders,
    video_frames, video_streams,
};
pub use bindings::wasdk::platform::{
    audio, clipboard, file_dialogs, file_watching, fonts, gpu_interop, hardware_buffers, lifecycle,
    machines, menus, mounts, notifications, opener, permissions, screens, service_extensions,
    system, trust, views, windows, workers,
};
pub use bindings::wasi::clocks::{monotonic_clock, system_clock};
pub use bindings::wasi::filesystem::preopens;
pub use bindings::wasi::filesystem::types as filesystem;
/// WASI 0.3's HTTP client, `wasi:http/client`, with its `types`: a `Request` built by
/// `Request::new`, `client::send` awaited for the `Response`, and the response's body a stream
/// read with `Response::consume_body`.
///
/// `Request::new` answers the request and a future of its transmission, and takes a future of its
/// trailers. Both report on a request in flight, so neither completes before `client::send` has
/// sent it: the transmission future resolves once the request, body and all, has gone, and the
/// host reads the trailers future only as it sends. So send first and await the transmission
/// after it, or drop it where nothing follows the transmission; a request with no body leaves its
/// trailers future at `wit_future::new`'s default, its writer dropped, and never writes it.
/// Awaiting the transmission, or writing and awaiting the trailers, before `send` waits for a
/// send that has not begun: it never completes, and no error comes.
///
/// ```ignore
/// let (_no_trailers, trailers) = wit_future::new(|| Ok(None));
/// let (request, sent) = Request::new(Fields::new(), None, trailers, None);
/// request.set_scheme(Some(&Scheme::Https)).unwrap();
/// request.set_authority(Some("example.com")).unwrap();
/// request.set_path_with_query(Some("/")).unwrap();
/// let response = client::send(request).await?;
/// let _ = sent.await; // the request has gone; drop `sent` instead to follow nothing
/// ```
pub use bindings::wasi::http;
pub use bindings::wasi::sockets;
pub use bindings::wasi::webgpu::webgpu;
pub use bindings::{wit_future, wit_stream};
pub use wasdk_macros::{main, worker};
pub use wit_bindgen::rt::async_support::{
    FutureReader, FutureWriter, StreamReader, StreamResult, StreamWriter, block_on,
    spawn_local as spawn, yield_async,
};
