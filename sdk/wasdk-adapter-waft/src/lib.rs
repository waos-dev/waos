//! Waft over wasdk: Waft's embedder for a program, the platform it sees over the SDK's `views`,
//! `screens`, `system`, `windows`, `lifecycle`, `fonts`, `clipboard`, `menus`, `file_dialogs`,
//! `opener`, `notifications`, `media_players`, `media_assets`, `image_codecs` and `cameras`, drawn
//! with valo over `wasdk-wgpu`. [`run`] is the body of a program's `main`.
//!
//! It maps Waft's model one to one (the design's decision 4): the views the shell gave and the
//! ones that arrive later, each carrying its request's id and initial data; each view's events,
//! carried field for field into Waft's Flutter shapes; each view's frames through its own
//! `request-frame` and `present`, a refresh serving several views drawn as one frame of them all,
//! each view holding the composition layers its scene places as it presents, which Waft's
//! compositor makes as the WIT's ([`composition`]);
//! windows through `request-view` and `view.window()`; the screens and the screen each view is on;
//! light or dark and the languages; the lifecycle state and the requests to end; the clipboard, its
//! items, writes and changes, flushed as the program ends, and the user's copy, cut and paste and
//! Edit's other commands, which a view hears; the menu bar, context menus and tray icons
//! ([`menus`]); the file dialogs ([`file_dialogs`]); the system's opener ([`opener`]) and what the
//! user opens with the program ([`open_requests`]); the system's notifications and the clicks on
//! them ([`notifications`]); the system's players, their pictures on the program's composition
//! layers ([`players`]); the system's reading of media without playing it ([`media_assets`]); the
//! system's image codecs ([`image_codecs`]), which Waft's own images decode with too
//! ([`image_decoding`]); and the system's cameras, their previews on those
//! layers too
//! ([`cameras`]). The program's fonts are the engine's, as decisions 36 and 37 set
//! them ([`fonts`]).
//!
//! The modules: [`event_loop`] turns every event into one push into Waft's client, [`events`]
//! merges the streams it reads, and [`refreshes`] gathers a refresh's frames from the streams of
//! the views it serves; [`platform`] is what Waft asks of the machine, [`views`] what it draws
//! into, [`frame_drawing`] whether a frame is drawn there and [`renderer`] what it draws with,
//! [`composition`] the compositor its layers are made on, and [`placements`] where a frame places
//! each, as the WIT carries it, [`windows`] the windows around views, [`screens`] the screens,
//! [`fonts`] the program's fonts and their glyphs, [`clipboard`] the clipboard, [`menus`] the
//! menus, [`file_dialogs`] the file dialogs, [`opener`] the opener, [`open_requests`] what the user
//! opens with the program, [`notifications`] the notifications, [`players`] the players,
//! [`media_assets`] the reading of media, [`image_codecs`] the image codecs, [`image_decoding`]
//! the decoding of Waft's own images, [`cameras`] the cameras,
//! [`dispatcher`] where the framework asks to be woken, [`text_input`] where the app's text is
//! edited and [`typing`] what a key the framework did not handle types there, [`lifecycle_states`]
//! how much of the program its user sees, [`app_exit`] how the program ends; and [`pointer`],
//! [`keys`], [`focus`], [`text_editing`], [`system`] and [`mouse_cursor`] the conversions between
//! Waft's vocabulary and the WIT's.
//!
//! The invariant: this crate depends on `sdk/` and Waft only, nothing of the engine or the
//! capabilities. It lives in `sdk/` until its API settles.

mod app_exit;
mod cameras;
mod caption_settings;
mod clipboard;
mod composition;
mod cues;
mod dispatcher;
mod event_loop;
mod events;
mod file_dialogs;
mod focus;
mod fonts;
mod frame_drawing;
mod gpu_interop;
mod hardware_buffers;
mod image_codecs;
mod image_decoding;
mod keys;
mod lifecycle_states;
mod media_assets;
mod menus;
mod moments;
mod mouse_cursor;
mod notifications;
mod open_requests;
mod opener;
mod overlay_style;
mod placements;
mod platform;
mod players;
mod pointer;
mod refreshes;
mod renderer;
mod screens;
mod shown_videos;
mod system;
pub mod targets;
mod text_editing;
mod text_input;
mod typing;
mod video_frames;
mod views;
mod windows;

use std::rc::Rc;

use futures::channel::mpsc;
use waft_embedder::PlatformRef;
use waft_foundation::App;
use waft_shell::Shell;

use crate::event_loop::EventLoop;
use crate::events::Events;
use crate::platform::WasdkPlatform;
use crate::renderer::Renderer;

/// Runs an Waft app until it quits, then flushes the clipboard: the body of a program's `main`.
/// `setup` is where `run_app` is called, which builds the app in the views the shell gave, or asks
/// for a window when it gave none.
///
/// ```ignore
/// #[wasdk::main]
/// async fn main() {
///     wasdk_adapter_waft::run(|app| run_app(app, CupertinoApp::new().home(Home))).await;
/// }
/// ```
///
/// # Panics
///
/// When the engine gives no WebGPU adapter or device.
pub async fn run(setup: impl FnOnce(&mut App)) {
    let (notice_sender, notices) = mpsc::unbounded();
    let mut events = Events::open(notices);
    let renderer = Rc::new(Renderer::new().await);
    let platform = Rc::new(WasdkPlatform::new(renderer, notice_sender, events.opener()));
    for view in wasdk::views::views() {
        if let Some(view) = platform.add_view(view) {
            events.add_view(&view);
        }
    }
    let client = Shell::new(Rc::clone(&platform) as PlatformRef, setup);
    let mut event_loop = EventLoop::new(Rc::clone(&platform), client, events);
    event_loop.run().await;
    // While the app and the tasks answering its pastes are still there, as AppKit asks inside
    // `terminate:`: the forms it promises outlive the program, and its promised files end with it.
    platform.flush_clipboard().await;
}
