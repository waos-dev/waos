//! GPUI over wasdk: GPUI's platform for a program, its windows, input, frames and the rest over
//! the SDK, drawn by `gpui_wgpu` over `wasdk-wgpu`.
//!
//! ```ignore
//! #[wasdk::main]
//! async fn main() {
//!     wasdk_adapter_gpui::run(|cx| { cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Root)); }).await;
//! }
//! ```
//!
//! `run` makes the program's GPU device, the platform over it and GPUI's app, launches the app
//! with `on_launch`, and then turns the event loop until the app quits; `run_with` sets the app
//! up first with GPUI's own builder (its assets, its HTTP client).
//!
//! Only a component has GPUI here: natively the crate is empty, so the workspace's native build
//! does not build GPUI.

#![cfg(target_os = "wasi")]

mod clipboard_text;
mod cursor;
mod dispatcher;
mod display;
mod event_loop;
mod events;
mod file_dialogs;
mod keys;
mod logging;
mod platform;
mod pointer;
mod slot;
mod status_bar;
mod text_input;
mod text_system;
mod window;

use std::rc::Rc;

use futures::channel::mpsc;
use gpui::{App, Application};
use gpui_wgpu::WgpuContext;

pub use gpui;
pub use status_bar::{StatusBarContentStyle, set_status_bar_style};

use crate::event_loop::EventLoop;
use crate::events::Events;
use crate::platform::WasdkPlatform;

/// Runs a GPUI app as a WaOS program until it quits.
pub async fn run(on_launch: impl FnOnce(&mut App) + 'static) {
    run_with(|application| application, on_launch).await;
}

/// Runs a GPUI app as a WaOS program until it quits, its `Application` set up first by
/// `configure` with GPUI's own builder: its assets, HTTP client, quit mode and the rest.
///
/// ```ignore
/// wasdk_adapter_gpui::run_with(|app| app.with_assets(Assets), |cx| { /* ... */ }).await;
/// ```
pub async fn run_with(
    configure: impl FnOnce(Application) -> Application,
    on_launch: impl FnOnce(&mut App) + 'static,
) {
    logging::init();
    let context = match WgpuContext::new_wasi(wasdk_wgpu::instance()).await {
        Ok(context) => context,
        Err(error) => {
            log::error!("no GPU for the program: {error:#}");
            return;
        }
    };
    let (notices, notices_received) = mpsc::unbounded();
    let events = Events::open(notices_received);
    let platform = Rc::new(WasdkPlatform::new(context, notices));
    WasdkPlatform::make_current(&platform);
    let application = Application::with_platform(Rc::clone(&platform) as Rc<dyn gpui::Platform>);
    let _app = configure(application).run_embedded(on_launch);
    EventLoop::new(platform, events).run().await;
}
