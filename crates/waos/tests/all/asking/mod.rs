//! The permissions of the app a test runs, as WaOS hands them: the real ones over settings in
//! memory at WaOS's defaults (`TestShell::permissions`), its user asked at first use, with the
//! changes a test makes to the app's own settings first.

use waos_services::permissions::{AppSettings, Permissions, ProgramId};

use crate::test_shell::TestShell;

/// The id the tests' app runs as, which its permissions are kept by, and which its user is told.
pub fn app() -> ProgramId {
    "dev.waos.test.app".parse().unwrap()
}

/// The permissions `shell`'s user is asked through, [`app`]'s own settings changed by `change`.
pub fn permissions_with(shell: &TestShell, change: impl FnOnce(&mut AppSettings)) -> Permissions {
    let permissions = shell.permissions();
    permissions
        .change_app_settings(&app(), change)
        .expect("settings in memory keep every change");
    permissions
}
