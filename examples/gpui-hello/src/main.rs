//! A GPUI app as a WaOS program: text, a button that counts its clicks, and a text field, drawn by
//! GPUI's wgpu renderer on the program's view, with its keys, pointer and text input, over
//! `wasdk-adapter-gpui`.
//!
//! GPUI is built for a component only, as the adapter is, so the workspace's native build stays
//! free of it; natively this program does nothing.

#[cfg(target_os = "wasi")]
mod hello;
#[cfg(target_os = "wasi")]
mod text_field;

#[wasdk::main]
async fn main() {
    #[cfg(target_os = "wasi")]
    wasdk_adapter_gpui::run(hello::launch).await;
}
