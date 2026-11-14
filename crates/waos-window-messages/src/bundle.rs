//! What a program's app bundle tells its window process, as WaOS writes it and the window process
//! reads it: the keys WaOS adds to the bundle's `Info.plist` beside Apple's, as Chrome adds its
//! `CrAppMode` keys, the entry its loader calls in WaOS's window library, and the argument WaOS
//! launches it with.
//!
//! The invariant: the entry's name carries its version, as Chrome's `ChromeAppModeStart_v<N>`
//! does. A window library that changes what it expects of a bundle renames it, so a bundle's
//! older loader finds no entry, its launch fails, and WaOS rebuilds the bundle.

/// The program's id.
pub const PROGRAM_ID_KEY: &str = "WaOSProgramId";

/// WaOS's data folder, which names the port WaOS is found by (`bootstrap_name`).
pub const DATA_FOLDER_KEY: &str = "WaOSDataFolder";

/// Where WaOS's window library is, which the loader loads.
pub const WINDOW_LIBRARY_KEY: &str = "WaOSWindowLibrary";

/// The WaOS a window process starts when none answers: its app bundle, or its executable where it
/// runs outside one.
pub const WAOS_KEY: &str = "WaOSApp";

/// The window library's entry, which the loader calls with nothing and which answers the
/// process's exit status.
pub const WINDOW_MAIN: &str = "waos_window_main_v1";

/// The argument WaOS launches a program's app with, which then connects to it only.
pub const LAUNCHED_BY_WAOS: &str = "--launched-by-waos";
