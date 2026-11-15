//! The WaOS app: the desktop and phone shells in Waft, with an entry per OS. It embeds the engine,
//! gives each program this computer's capabilities, and implements the shell's traits: where views
//! go, prompts, file dialogs, menu bars, the system's opener and its notifications, the picker of a
//! sound's output and what shows who records.
//!
//! The app is this library, in the shape Waft's CLI builds an app from. On a desktop `main` is its
//! entry (`desktop_entry`): the `waos` binary calls it, and so does WaOS.app, which `cargo waft
//! build macos -p waos-app` makes. On Android Waft's host calls `android_main` once for the process,
//! which runs `android_entry`, with the library on the screen; on iOS the binary calls `main` from
//! `ios_entry`, which shows the library on the screen too. Built so far, the third and fourth parts of step 5a of the design's work order and the
//! library of step 6: the app starts the engine with what Waft reports of the machine and keeps it
//! current, shows each view a program asks for in an Waft window of its own, and listens for
//! `cargo waos run` on the socket `waos-dev-protocol` names, running each program sent on a thread of its
//! own in this process with its output back on its connection; its own window is the library's
//! panel above WaOS's Dock icon, where the programs installed open and are removed, sources are
//! added and their programs installed, and a `.wapk` opened with WaOS is installed; quitting asks
//! every program, and stops them all once every one agrees. `waos open <file>... --program <id>`
//! opens files with a program, through the WaOS running on this machine or a WaOS it starts.
//!
//! The code is in Waft's layers as the experiment's app had them: `logic` is plain Rust, tested
//! without Waft's UI, `state` ties it to the application, `ui` holds the widgets, and `os` reads
//! what Waft's host does not say, the Dock's preferences.
//!
//! The invariant: the app is the shell and the embedder, never the engine. What the engine does
//! for a program lives in `waos`; this crate supplies what the person sees and decides.

// Waft's observers and states take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]
// Proving that a type holding a program's handle crosses threads walks into wgpu-core's
// registries, deeper than the default limit allows (the engine's README).
#![recursion_limit = "256"]

#[cfg(target_os = "android")]
mod android_entry;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop_entry;
#[cfg(target_os = "ios")]
mod ios_entry;
mod logic;
mod os;
mod state;
mod ui;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop_entry::main;
#[cfg(target_os = "ios")]
pub use ios_entry::main;

/// Writes what `RUST_LOG` picks of the app's and the engine's events to `writer`, coloured where
/// `ansi`; without it, the app's own and what programs print at `info`, and warnings elsewhere.
/// Android gives an app no environment to set, so this default is all a phone's log shows: a
/// program's own lines are in it so it can be debugged there.
fn write_traces<W>(writer: W, ansi: bool)
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_unset| {
        EnvFilter::new(format!(
            "warn,waos_app=info,{}=info",
            waos::targets::PROGRAM
        ))
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(ansi)
        .init();
}
