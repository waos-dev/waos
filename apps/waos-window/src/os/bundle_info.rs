//! What the program's app bundle says of the window process: its `Info.plist`, as WaOS wrote it
//! (`waos_window_messages::bundle`), read through the process's main bundle, which is the program's
//! since the loader loaded this library into it.

use std::path::PathBuf;

use objc2_foundation::{NSBundle, NSString};
use waos_package::ProgramId;
use waos_window_messages::bundle::{DATA_FOLDER_KEY, PROGRAM_ID_KEY, WAOS_KEY};

/// What the bundle says.
#[derive(Debug)]
pub(crate) struct BundleInfo {
    /// The program the window process is.
    pub(crate) program: ProgramId,
    /// WaOS's data folder, which names the port WaOS is found by.
    pub(crate) data_folder: PathBuf,
    /// The WaOS to start when none answers.
    pub(crate) waos: PathBuf,
}

impl BundleInfo {
    /// What the process's bundle says; why not, where it says too little.
    pub(crate) fn read() -> Result<BundleInfo, String> {
        let program = string(PROGRAM_ID_KEY)?;
        Ok(BundleInfo {
            program: ProgramId::try_from(program.clone())
                .map_err(|error| format!("{PROGRAM_ID_KEY} {program:?}: {error}"))?,
            data_folder: PathBuf::from(string(DATA_FOLDER_KEY)?),
            waos: PathBuf::from(string(WAOS_KEY)?),
        })
    }
}

/// The string the bundle's `Info.plist` gives `key`.
fn string(key: &str) -> Result<String, String> {
    let value = NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&NSString::from_str(key))
        .ok_or_else(|| format!("the bundle names no {key}"))?;
    let value = value
        .downcast::<NSString>()
        .map_err(|_| format!("the bundle's {key} is not a string"))?;
    Ok(value.to_string())
}
