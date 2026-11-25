//! The registry settings a program's entry makes under `HKEY_CURRENT_USER`, through the registry's
//! own calls: string values set, each key made where it is missing, and keys deleted whole or
//! single values deleted from keys other apps share; and the shell told file associations changed
//! once they did (`SHChangeNotify`), so Explorer reads them again.

// The registry and the shell are reached through their C calls, handed key handles and
// NUL-terminated wide strings: that is the `unsafe` here.
#![allow(unsafe_code)]

use std::io;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use windows::core::{HSTRING, PCWSTR};

use crate::logic::program_entries::RegistrySetting;
use crate::logic::program_entries::file_associations::RegistryValue;

/// Sets each of `values`, then tells the shell file associations changed.
pub(crate) fn set_registry_values(values: &[RegistryValue]) -> io::Result<()> {
    for value in values {
        set(value)?;
    }
    associations_changed();
    Ok(())
}

/// Takes back each of `settings`, one already gone being no failure, then tells the shell file
/// associations changed.
pub(crate) fn delete_registry_settings(settings: &[RegistrySetting]) -> io::Result<()> {
    for setting in settings {
        let deleted = match setting {
            RegistrySetting::Key(key) => {
                let key = HSTRING::from(key.as_str());
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &key) }
            }
            RegistrySetting::Value { key, name } => {
                let (key, name) = (HSTRING::from(key.as_str()), HSTRING::from(name.as_str()));
                unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &name) }
            }
        };
        if deleted != ERROR_FILE_NOT_FOUND {
            checked(deleted)?;
        }
    }
    associations_changed();
    Ok(())
}

/// Sets `value`, its key made where it is missing.
fn set(value: &RegistryValue) -> io::Result<()> {
    let key = HSTRING::from(value.key.as_str());
    let mut opened = HKEY::default();
    checked(unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &key,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut opened,
            None,
        )
    })?;
    let name = value.name.as_deref().map(HSTRING::from);
    let name = name
        .as_ref()
        .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr()));
    let data: Vec<u8> = value
        .data
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let set = unsafe { RegSetValueExW(opened, name, None, REG_SZ, Some(&data)) };
    // The value is set or not by now; a key that fails to close leaks a handle, no more.
    let _ = unsafe { RegCloseKey(opened) };
    checked(set)
}

/// Tells the shell that file associations changed, as Microsoft asks of whoever changes them.
fn associations_changed() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

/// `error` as a failure, unless it is none.
fn checked(error: WIN32_ERROR) -> io::Result<()> {
    if error == ERROR_SUCCESS {
        return Ok(());
    }
    Err(io::Error::from_raw_os_error(error.0 as i32))
}
