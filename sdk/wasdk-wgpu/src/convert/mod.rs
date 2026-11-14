//! Conversions between wgpu's vocabulary and wasi:webgpu's.
//!
//! Every conversion is total: a wgpu value without a wasi counterpart panics naming the gap
//! (the list is in the crate documentation); nothing is silently substituted. The wasi records
//! spell out every optional member the WebGPU dictionaries default, so the host never fills in
//! a default wgpu did not ask for.

mod binding;
mod capabilities;
mod format;
mod render;
mod sampler;
mod surface;

pub use binding::*;
pub use capabilities::*;
pub use format::*;
pub use render::*;
pub use sampler::*;
pub use surface::*;

/// A wgpu label as the WIT carries it.
pub fn label(label: wgpu::Label<'_>) -> Option<String> {
    label.map(str::to_owned)
}
