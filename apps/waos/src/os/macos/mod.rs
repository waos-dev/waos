//! What WaOS asks of macOS itself: each program's app of its own: its bundle, written, signed and
//! registered (`program_bundle`), the code hash of a bundle or of the process that sent a message
//! (`code_signature`), the app opened through Launch Services and its process's end
//! (`launch_services`), WaOS's activation yielded to it as what the user must see shows there
//! (`activation`), the system's version the bundles are signed on (`system_version`), the key in
//! the Keychain the record of the bundles is vouched for with (`keychain`), the layer context
//! each program view's layers are hosted from, which its app's window shows (`layer_contexts`),
//! a program's hardware buffer handed to the main thread with the picture its window's resize
//! waits for (`hardware_buffers`), System Settings at the notifications of a program's app
//! (`system_settings`), each change of the user's time zone (`time_zone_changes`), the frames
//! of WaOS's simulated camera, `IOSurface`s under `CVPixelBuffer`s drawn by the CPU
//! (`pattern_frames`), Waft's video decoders for a WaOS that is no Waft app
//! (`video_decoders`), and the dev server announced through Bonjour (`dev_announcement`).
//!
//! The invariant: wrapping the system is what this module is for, and `code_signature`,
//! `launch_services`, `layer_contexts`, `hardware_buffers`, `time_zone_changes`, `pattern_frames`
//! and `dev_announcement` are the places in the app where `unsafe` is written.

mod activation;
mod code_signature;
mod dev_announcement;
mod hardware_buffers;
mod keychain;
mod launch_services;
mod layer_contexts;
mod pattern_frames;
mod program_bundle;
mod system_settings;
mod system_version;
mod time_zone_changes;
mod video_decoders;

pub(crate) use activation::yield_activation_to;
pub(crate) use code_signature::code_hash_of_process;
pub(crate) use dev_announcement::{DevServerAnnouncement, announce_dev_server};
pub(crate) use hardware_buffers::CrossThreadBuffer;
#[cfg(test)]
pub(crate) use hardware_buffers::test_buffer;
pub(crate) use keychain::safe_storage_key;
pub(crate) use launch_services::{open_app, wait_for_exit};
pub(crate) use layer_contexts::{LayerContext, check_layer_contexts};
#[cfg(test)]
pub(crate) use pattern_frames::read_pattern_frame;
pub(crate) use pattern_frames::{CrossThreadFrame, pattern_frame, surface_of};
pub(crate) use program_bundle::{
    BundleContents, WaosFiles, delete as delete_bundle, write as write_bundle,
};
pub(crate) use system_settings::notification_settings_link;
pub(crate) use system_version::system_version;
pub(crate) use time_zone_changes::{TimeZoneChanges, watch_time_zone};
pub(crate) use video_decoders::video_decoders;
