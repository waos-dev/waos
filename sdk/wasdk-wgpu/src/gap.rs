//! The one place a request the boundary cannot carry becomes a panic. The list of what
//! `wasi:webgpu@0.3.0-rc.2` cannot express is in the crate documentation.

use std::fmt::Display;

/// `what` has no counterpart in wasi:webgpu.
#[track_caller]
pub(crate) fn wasi_gap(what: impl Display) -> ! {
    panic!(
        "wasdk-wgpu: {what} has no counterpart in wasi:webgpu@0.3.0-rc.2 (see the crate documentation)"
    )
}

/// `what` needs the wgpu feature `feature`, which wasi:webgpu can neither report nor request.
#[track_caller]
pub(crate) fn feature_gap(feature: wgpu::Features, what: impl Display) -> ! {
    panic!("wasdk-wgpu: {what} needs {feature:?}, which wasi:webgpu@0.3.0-rc.2 cannot provide")
}
