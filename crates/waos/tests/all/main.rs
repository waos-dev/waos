//! The engine's tests, as one binary, which links the engine and compiles the test shell once for
//! all of them: each module tests one part of the engine, or one program, under the test shell. A
//! test is named after its module, so `cargo test -p waos --test all survey::` runs survey's. The
//! modules that draw, or that run a program that does, run on Apple's systems, where the engine
//! makes hardware buffers; elsewhere a test in place of each of three says so and passes.
//!
//! The invariant: the tests run alongside each other in one process, so a test reads only what
//! its own engine, shell and outputs record, or a program's lines in the log while it holds the
//! program's turn (`shell::turns`). A test that reads what every engine in the process records,
//! with nothing to tell its own from another's, as the frames the engine hands over and the glyph
//! calls it draws are, is a binary of its own beside this one: `frames_in_flight`, `two_windows`
//! and `waft_app`.

// Proving the test shell crosses threads walks from a program's handle into wgpu-core's
// registries, deeper than the default limit allows; an app embedding the engine needs the same.
#![recursion_limit = "256"]
// A file's index on its volume, which tells a file compiled again from the one before it.
#![cfg_attr(windows, feature(windows_by_handle))]

mod asking;
mod reaching;
mod shell;

mod audio;
mod back;
mod cameras;
mod clipboard;
#[cfg(target_vendor = "apple")]
mod composition_layers;
#[cfg(target_vendor = "apple")]
mod drawing;
mod ending;
mod environment;
mod fetch;
mod file_dialogs;
mod file_watching;
// The program makes symlinks, which Windows makes only with Developer Mode on or the privilege to,
// and expects Unix's answers through them.
#[cfg(unix)]
mod file_escapes;
// The program's folders hold symlinks, which Windows makes only with Developer Mode on or the
// privilege to; its grants are `file_escapes`' too.
#[cfg(unix)]
mod files;
mod fonts;
mod freezing;
#[cfg(target_vendor = "apple")]
mod hardware_buffers;
#[cfg(target_vendor = "apple")]
mod image_codecs;
#[cfg(target_vendor = "apple")]
mod input;
mod machines;
#[cfg(target_vendor = "apple")]
mod media_assets;
mod media_players;
mod menus;
mod network;
#[cfg(target_vendor = "apple")]
mod notes;
mod notifications;
mod opener;
mod output;
mod permissions;
mod roles;
mod storage;
mod survey;
mod trust;
mod video_frames;
#[cfg(target_vendor = "apple")]
mod video_player;
#[cfg(target_vendor = "apple")]
mod waft_back;
#[cfg(target_vendor = "apple")]
mod waft_http;
#[cfg(target_vendor = "apple")]
mod waft_image;
#[cfg(target_vendor = "apple")]
mod waft_menus;
#[cfg(target_vendor = "apple")]
mod waft_time;
mod workers;

#[cfg(not(target_vendor = "apple"))]
mod composition_layers {
    #[test]
    fn composition_layers_are_not_presented_on_this_system() {
        eprintln!(
            "the engine makes hardware buffers on Apple's systems; the composition layer tests do \
             not run here"
        );
    }
}

#[cfg(not(target_vendor = "apple"))]
mod drawing {
    #[test]
    fn drawing_is_not_built_on_this_system() {
        eprintln!(
            "the engine makes hardware buffers on Apple's systems; the drawing tests do not run here"
        );
    }
}

#[cfg(not(target_vendor = "apple"))]
mod hardware_buffers {
    #[test]
    fn hardware_buffers_are_not_allocated_on_this_system() {
        eprintln!(
            "the engine allocates hardware buffers on Apple's systems; the hardware buffer tests do \
             not run here"
        );
    }
}
