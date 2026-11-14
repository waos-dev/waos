//! A program's Start menu shortcut, written through the shell's `IShellLinkW` and saved through
//! its `IPersistFile`, carrying the program's AppUserModelID in the shortcut's property store
//! (`System.AppUserModel.ID`), as Microsoft asks of an app's shortcut, set before it is saved.

// The shell's shortcut is a COM object, made, filled and saved through its interfaces, and its
// AppUserModelID is a property value allocated with the shell's allocator: that is the `unsafe`
// here.
#![allow(unsafe_code)]

use std::io;
use std::mem::ManuallyDrop;

use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, SHStrDupW, ShellLink};
use windows::core::{HSTRING, Interface, Result};

use crate::logic::program_entries::start_menu::Shortcut;

/// Writes `shortcut` at its path, replacing what is there.
pub(crate) fn write_shortcut(shortcut: &Shortcut) -> io::Result<()> {
    let _apartment = Apartment::enter()?;
    save(shortcut).map_err(io::Error::from)
}

fn save(shortcut: &Shortcut) -> Result<()> {
    // SAFETY: COM is entered on this thread for as long as the shortcut lives, and each string
    // passed outlives the call it is passed to, which copies it.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(shortcut.target.as_os_str()))?;
        link.SetArguments(&HSTRING::from(shortcut.arguments.as_str()))?;
        link.SetDescription(&HSTRING::from(shortcut.description.as_str()))?;
        if let Some(icon) = &shortcut.icon {
            link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)?;
        }
        let properties: IPropertyStore = link.cast()?;
        let id = string_value(&shortcut.app_user_model_id)?;
        properties.SetValue(&PKEY_AppUserModel_ID, &id)?;
        properties.Commit()?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(shortcut.path.as_os_str()), true)
    }
}

/// A `VT_LPWSTR` holding `text`, allocated with the task allocator, as `PropVariantClear`, which
/// the value's drop calls, frees it: the shell takes an AppUserModelID as `VT_LPWSTR` alone.
fn string_value(text: &str) -> Result<PROPVARIANT> {
    // SAFETY: a NUL-terminated string, copied into memory the value owns.
    let copy = unsafe { SHStrDupW(&HSTRING::from(text)) }?;
    Ok(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { pwszVal: copy },
            }),
        },
    })
}

/// COM entered on this thread, a single-threaded apartment, as the shell's objects ask, and left
/// when dropped.
struct Apartment;

impl Apartment {
    fn enter() -> io::Result<Apartment> {
        // SAFETY: entered here and left once, in drop, on the same thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(io::Error::from)?;
        Ok(Apartment)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: COM was entered on this thread by `enter`.
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use windows::Win32::System::Com::STGM_READ;

    use super::*;

    /// The AppUserModelID, arguments and icon of the shortcut at `path`, read back through the
    /// shell, as the taskbar reads them.
    fn read_back(path: &std::path::Path) -> (String, String, String) {
        let _apartment = Apartment::enter().unwrap();
        // SAFETY: COM is entered for the reads, and each buffer outlives the call that fills it.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let file: IPersistFile = link.cast().unwrap();
            file.Load(&HSTRING::from(path.as_os_str()), STGM_READ)
                .unwrap();
            let properties: IPropertyStore = link.cast().unwrap();
            let id = properties
                .GetValue(&PKEY_AppUserModel_ID)
                .unwrap()
                .to_string();
            let mut arguments = [0u16; 512];
            link.GetArguments(&mut arguments).unwrap();
            let mut icon = [0u16; 512];
            let mut index = 0;
            link.GetIconLocation(&mut icon, &mut index).unwrap();
            let text = |wide: &[u16]| {
                let end = wide
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(wide.len());
                String::from_utf16_lossy(&wide[..end])
            };
            (id, text(&arguments), format!("{},{index}", text(&icon)))
        }
    }

    #[test]
    fn a_shortcut_carries_the_programs_identity_its_arguments_and_its_icon() {
        let folder = tempfile::tempdir().unwrap();
        let shortcut = Shortcut {
            path: folder.path().join("Notes.lnk"),
            target: std::env::current_exe().unwrap(),
            arguments: "open --program dev.example.notes".to_owned(),
            app_user_model_id: "dev.waos.app.dev.example.notes".to_owned(),
            icon: Some(PathBuf::from(r"C:\Windows\System32\shell32.dll")),
            description: "Notes".to_owned(),
        };
        write_shortcut(&shortcut).unwrap();
        let (id, arguments, icon) = read_back(&shortcut.path);
        assert_eq!(id, "dev.waos.app.dev.example.notes");
        assert_eq!(arguments, "open --program dev.example.notes");
        assert_eq!(icon, r"C:\Windows\System32\shell32.dll,0");
    }
}
