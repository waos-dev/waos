//! What this computer runs: on Unix's systems what `uname(2)` names, read as `uname -sm` is on an
//! SSH machine; on Windows the processor Windows itself runs on (`windows::native_architecture`),
//! not the one WaOS may be emulated on.

use waos_capabilities::Platform;

/// What this computer runs.
#[cfg(unix)]
pub(crate) fn this_computer() -> Platform {
    let names = rustix::system::uname();
    Platform::of_uname(
        &names.sysname().to_string_lossy(),
        &names.machine().to_string_lossy(),
    )
}

/// What this computer runs.
#[cfg(windows)]
pub(crate) fn this_computer() -> Platform {
    Platform {
        os: waos_capabilities::OperatingSystem::Windows,
        arch: crate::windows::native_architecture(),
    }
}

#[cfg(test)]
mod tests {
    use waos_capabilities::{Architecture, OperatingSystem};

    use super::*;

    #[test]
    fn this_computer_runs_what_rust_built_waos_for() {
        let platform = this_computer();
        let os = match std::env::consts::OS {
            "macos" => OperatingSystem::Macos,
            "linux" | "android" => OperatingSystem::Linux,
            "windows" => OperatingSystem::Windows,
            other => OperatingSystem::Other(other.to_owned()),
        };
        assert_eq!(platform.os, os);
        let arch = match std::env::consts::ARCH {
            "aarch64" => Architecture::Aarch64,
            "x86_64" => Architecture::X86_64,
            other => Architecture::Other(other.to_owned()),
        };
        assert_eq!(platform.arch, arch);
    }
}
