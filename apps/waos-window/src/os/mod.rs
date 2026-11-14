//! What the window process asks of the system itself: the library's entry, which the loader calls
//! (`entry`); what the program's app bundle says (`bundle_info`); WaOS started, hidden
//! (`starting_waos`); what a view shows, WaOS's layers, hosted in the window (`layer_host`); the
//! app brought forward as a window that takes the focus shows (`activation`); and the user's
//! attention asked for the app in the background as it shows a prompt's dialog (`attention`). A
//! program's windows are an app of its own on macOS alone.
//!
//! The invariant: wrapping the system is what this module is for, and the entry and the layer host
//! are the places in the library where `unsafe` is written.

mod activation;
mod attention;
mod bundle_info;
mod entry;
mod layer_host;
mod starting_waos;

pub(crate) use activation::come_forward;
pub(crate) use attention::Attention;
pub(crate) use bundle_info::BundleInfo;
pub(crate) use layer_host::{LayerHost, check_layer_hosts};
pub(crate) use starting_waos::start_waos_hidden;
