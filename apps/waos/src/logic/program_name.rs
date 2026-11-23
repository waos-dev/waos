//! A program's name as its user knows it: an installed program's name, which its package gives,
//! else what it runs as, as a program `waos` sends runs under its id.

use waos::RunningProgram;
use waos_package::ProgramId;
use waos_store::Store;

/// `program`'s name: its name in `store` where it is installed there, else what it runs as.
pub(crate) fn program_name(store: &Store, program: &RunningProgram) -> String {
    name_of(store, program.name())
}

/// The name of the program that runs as `runs_as`: its name in `store` where it is installed
/// there, else `runs_as`.
pub(crate) fn name_of(store: &Store, runs_as: &str) -> String {
    let installed = runs_as.parse::<ProgramId>().ok().and_then(|id| {
        store
            .installed()
            .into_iter()
            .find(|installed| installed.manifest.id() == &id)
    });
    match installed {
        Some(installed) => installed.name(),
        None => runs_as.to_owned(),
    }
}
