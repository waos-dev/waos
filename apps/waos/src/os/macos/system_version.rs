//! The system's version and build, as the bundles WaOS signs are kept against: a change of either
//! makes Gatekeeper distrust ad-hoc signatures made before it.

use objc2_foundation::NSProcessInfo;

/// The system's version and build, as the system says them: `Version 27.0.1 (Build 26A434)`.
pub(crate) fn system_version() -> String {
    NSProcessInfo::processInfo()
        .operatingSystemVersionString()
        .to_string()
}
