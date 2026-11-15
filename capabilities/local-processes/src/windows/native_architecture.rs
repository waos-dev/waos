//! The processor Windows runs on: `IsWow64Process2`'s native machine, which is the computer's own
//! even where WaOS is an x64 build emulated on ARM64, where `GetNativeSystemInfo` answers x64.

use waos_capabilities::Architecture;
use windows_sys::Win32::System::SystemInformation::{
    IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};

/// The architecture of the processor Windows runs on; `other` with the machine's number where it
/// is neither ARM64 nor x64, or where Windows does not say.
pub(crate) fn native_architecture() -> Architecture {
    let mut process: IMAGE_FILE_MACHINE = 0;
    let mut native: IMAGE_FILE_MACHINE = 0;
    // SAFETY: the current process's pseudo handle, which needs no closing, and two numbers of ours
    // for Windows to write.
    let answered = unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) };
    if answered == 0 {
        return Architecture::Other("unknown".to_owned());
    }
    match native {
        IMAGE_FILE_MACHINE_ARM64 => Architecture::Aarch64,
        IMAGE_FILE_MACHINE_AMD64 => Architecture::X86_64,
        other => Architecture::Other(format!("{other:#06x}")),
    }
}
