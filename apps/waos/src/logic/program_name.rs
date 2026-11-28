//! A program's name as its user knows it: an installed program's name, which its package gives,
//! else what it runs as, as a program `waos` sends runs under its id.

use waos::AppInstance;
use waos_package::ProgramId;
use waos_services::packages::PackageManager;

/// `program`'s name: its name in `packages` where it is installed there, else what it runs as.
pub(crate) fn program_name(packages: &PackageManager, program: &AppInstance) -> String {
    name_of(packages, program.name())
}

/// The name of the program that runs as `runs_as`: its name in `packages` where it is installed
/// there, else `runs_as`.
pub(crate) fn name_of(packages: &PackageManager, runs_as: &str) -> String {
    let installed = runs_as
        .parse::<ProgramId>()
        .ok()
        .and_then(|id| packages.installed_program(&id));
    match installed {
        Some(installed) => installed.name(),
        None => runs_as.to_owned(),
    }
}
