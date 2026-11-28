//! Why a setting could not be read or kept, in words the person using WaOS can act on.

use std::error::Error;
use std::fmt;
use std::io;

use waos_package::ProgramId;

/// Why a setting could not be read or kept.
#[derive(Debug)]
pub enum SettingsError {
    /// The store could not read or keep it, in its words.
    Store(io::Error),
    /// The app is being removed, and its settings go with it.
    Removing(ProgramId),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::Store(error) => error.fmt(f),
            SettingsError::Removing(id) => write!(f, "{id} is being removed"),
        }
    }
}

impl Error for SettingsError {}

impl From<io::Error> for SettingsError {
    fn from(error: io::Error) -> SettingsError {
        SettingsError::Store(error)
    }
}
