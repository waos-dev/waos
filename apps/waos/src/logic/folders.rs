//! The app's own folders on this machine: among its caches, which the system may clear, the copies
//! of the packages handed without a path while they are asked about, and the running programs'
//! `/tmp`, each emptied when its program ends; WaOS's data folder, with the store in it, the
//! programs installed, their code compiled, and the sources added, which the user keeps; and, on
//! macOS, where the installed programs' apps go, among the user's applications.
//!
//! The invariant: the caches are under the user's caches, beside each other, so clearing WaOS's
//! caches is one folder; the store is under the user's application data, which nothing clears.

use std::path::PathBuf;

/// Where the app keeps what it may lose, and what it keeps.
pub(crate) struct AppFolders {
    /// Where the running programs' `/tmp` are made, `WaOSConfig::temporary_folders`, under the
    /// caches: on the volume of the user's home, where the store and the programs' `/data` are,
    /// so a program's file renamed from `/tmp` into `/data` is one rename.
    pub(crate) temporary_folders: PathBuf,
    /// The copies of the packages handed without a path while they are asked about
    /// (`logic::package_copies`), under the caches.
    pub(crate) package_copies: PathBuf,
    /// The store's directory: `Store::open`'s.
    pub(crate) store: PathBuf,
    /// WaOS's own preferences (`logic::preferences`), beside the store.
    pub(crate) preferences: PathBuf,
    /// The installed programs' icons as the system's lists of apps take them, beside the store:
    /// each one's `.ico` on Windows, its PNG on Linux (`logic::program_identities`).
    #[cfg(not(target_os = "android"))]
    pub(crate) program_icons: PathBuf,
    /// Where the installed programs' desktop entries go: the user's applications,
    /// `$XDG_DATA_HOME/applications` (`logic::program_entries`).
    #[cfg(target_os = "linux")]
    pub(crate) applications: PathBuf,
    /// Where the installed programs' shortcuts go: the Start menu's folder of the user's programs,
    /// in a folder of WaOS's own, as Chrome puts its web apps' in `Chrome Apps`
    /// (`logic::program_entries`).
    #[cfg(windows)]
    pub(crate) start_menu: PathBuf,
    /// WaOS's data folder, which names the port its programs' apps find it by on macOS, and keeps
    /// the record of what WaOS wrote for its programs' apps or entries; none on a phone, where
    /// WaOS writes neither.
    #[cfg(desktop)]
    pub(crate) data: PathBuf,
    /// Where developer access keeps the device's identity and the computers paired with it
    /// (`logic::developer_access`), the user's alone; not on a phone yet, where `waos` reaches no
    /// WaOS.
    #[cfg(desktop)]
    pub(crate) developer_access: PathBuf,
    /// The installed programs' apps, among the user's applications, as Chrome puts its web apps in
    /// `~/Applications/Chrome Apps`.
    #[cfg(target_os = "macos")]
    pub(crate) installed_apps: PathBuf,
}

/// The caches under the user's caches, `~/Library/Caches/WaOS` on Apple's systems,
/// `%LOCALAPPDATA%\WaOS\Cache` on Windows, `$XDG_CACHE_HOME/waos` elsewhere or `~/.cache/waos`
/// without one; and the store under the user's application data, `~/Library/Application
/// Support/WaOS` on Apple's systems, `%LOCALAPPDATA%\WaOS` on Windows, beside the caches' folder,
/// `$XDG_DATA_HOME/waos` elsewhere or `~/.local/share/waos` without one. Android names the app's
/// folders itself (`os::app_folders`).
#[cfg(not(target_os = "android"))]
pub(crate) fn app_folders() -> AppFolders {
    let caches = caches_folder();
    AppFolders {
        temporary_folders: caches.join("tmp"),
        package_copies: caches.join("packages"),
        store: data_folder().join("store"),
        preferences: data_folder().join("preferences.toml"),
        program_icons: data_folder().join("icons"),
        #[cfg(target_os = "linux")]
        applications: user_data_folder().join("applications"),
        #[cfg(windows)]
        start_menu: start_menu_programs().join("WaOS Programs"),
        #[cfg(not(target_os = "ios"))]
        data: data_folder(),
        #[cfg(not(target_os = "ios"))]
        developer_access: data_folder().join("developer"),
        #[cfg(target_os = "macos")]
        #[cfg(target_os = "macos")]
        installed_apps: home().join("Applications/WaOS Programs"),
    }
}

#[cfg(target_vendor = "apple")]
fn caches_folder() -> PathBuf {
    home().join("Library/Caches/WaOS")
}

/// `%LOCALAPPDATA%\WaOS\Cache`, a folder of its own beside the store, which stays on this
/// machine, as caches should.
#[cfg(windows)]
fn caches_folder() -> PathBuf {
    data_folder().join("Cache")
}

#[cfg(not(any(target_vendor = "apple", target_os = "android", windows)))]
fn caches_folder() -> PathBuf {
    match std::env::var_os("XDG_CACHE_HOME") {
        Some(caches) if !caches.is_empty() => PathBuf::from(caches).join("waos"),
        _ => home().join(".cache/waos"),
    }
}

#[cfg(target_vendor = "apple")]
fn data_folder() -> PathBuf {
    home().join("Library/Application Support/WaOS")
}

/// `%LOCALAPPDATA%\WaOS`, the user's application data on this machine, not the roaming
/// `%APPDATA%`: installed programs are large and belong to one machine. Under the home where the
/// shell names none.
#[cfg(windows)]
fn data_folder() -> PathBuf {
    crate::os::local_application_data()
        .unwrap_or_else(|| home().join("AppData").join("Local"))
        .join("WaOS")
}

#[cfg(not(any(target_vendor = "apple", target_os = "android", windows)))]
fn data_folder() -> PathBuf {
    user_data_folder().join("waos")
}

/// The user's data, `$XDG_DATA_HOME`, or `~/.local/share` without one.
#[cfg(not(any(target_vendor = "apple", target_os = "android", windows)))]
fn user_data_folder() -> PathBuf {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(data) if !data.is_empty() => PathBuf::from(data),
        _ => home().join(".local/share"),
    }
}

/// The Start menu's folder of the user's programs, `%APPDATA%\Microsoft\Windows\Start
/// Menu\Programs`, under the home where the shell names none.
#[cfg(windows)]
fn start_menu_programs() -> PathBuf {
    crate::os::start_menu_programs()
        .unwrap_or_else(|| home().join(r"AppData\Roaming\Microsoft\Windows\Start Menu\Programs"))
}

/// The user's home, or the root when the system cannot say: making a folder there fails with
/// the system's message, which names the path.
#[cfg(not(target_os = "android"))]
fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_package_copies_and_the_programs_tmp_are_under_the_users_caches() {
        let folders = app_folders();
        assert!(folders.temporary_folders.starts_with(home()));
        assert_eq!(
            folders.package_copies.parent(),
            folders.temporary_folders.parent()
        );
        assert_ne!(folders.package_copies, folders.temporary_folders);
    }

    #[test]
    fn the_store_is_kept_outside_the_caches() {
        let folders = app_folders();
        assert!(folders.store.starts_with(home()));
        assert!(
            !folders
                .store
                .starts_with(folders.temporary_folders.parent().unwrap())
        );
    }
}
