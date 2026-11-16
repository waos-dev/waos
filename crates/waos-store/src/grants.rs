//! The user's settings as the engine reads them: held in memory over their files, so a check
//! reads no file, and written through to the files under the store's lock; each change is heard
//! by every [`ProgramGrants::changed`] asked for before it. A program's grants are its own
//! settings over the global ones, whether it is installed or run in development.
//!
//! The invariant: settings that cannot be read grant nothing. A program whose own file or the
//! global one is damaged has every switch off, nothing mounted, no machine, every question
//! and link scheme denied, and no answer set for a picker or a dialog, which then asks, rather
//! than more than the user gave it, and the failure is said to whoever
//! subscribes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;
use waos_capabilities::{
    Answer, ChangedFuture, DomainRule, FileDialog, FileSystemGrant, Mount, NetworkGrant,
    ProgramGrants, Question, Role, Setting, SimulatedSound,
};
use waos_package::ProgramId;

use crate::settings::{GlobalSettings, Holding, ProgramSettings};
use crate::{StoreError, settings_file};

/// The file of settings, at the store's top for the global ones and in a program's folder for
/// its own.
pub(crate) const SETTINGS: &str = "settings.toml";

/// Every program's settings, as the store keeps them: read from their files once, and written
/// through.
pub(crate) struct Settings {
    directory: PathBuf,
    /// The global settings, once read.
    global: Mutex<Option<GlobalSettings>>,
    /// Each program's own settings, once read.
    programs: Mutex<HashMap<ProgramId, ProgramSettings>>,
    changed: Arc<Notify>,
}

impl Settings {
    /// The settings of the store in `directory`.
    pub(crate) fn new(directory: &Path) -> Settings {
        Settings {
            directory: directory.to_path_buf(),
            global: Mutex::default(),
            programs: Mutex::default(),
            changed: Arc::default(),
        }
    }

    pub(crate) fn global(&self) -> Result<GlobalSettings, StoreError> {
        let mut global = held(&self.global);
        if let Some(settings) = global.as_ref() {
            return Ok(settings.clone());
        }
        let settings = settings_file::read_global(&self.global_path())?;
        *global = Some(settings.clone());
        Ok(settings)
    }

    /// Changes the global settings as `change` does, and writes them. The caller holds the
    /// store's lock.
    pub(crate) fn change_global(
        &self,
        change: impl FnOnce(&mut GlobalSettings),
    ) -> Result<(), StoreError> {
        let mut settings = self.global()?;
        change(&mut settings);
        settings_file::write_global(&self.global_path(), &settings)?;
        *held(&self.global) = Some(settings);
        self.changed.notify_waiters();
        Ok(())
    }

    /// The program `id`'s own settings, in `folder`, its folder.
    pub(crate) fn program(
        &self,
        id: &ProgramId,
        folder: &Path,
    ) -> Result<ProgramSettings, StoreError> {
        let mut programs = held(&self.programs);
        if let Some(settings) = programs.get(id) {
            return Ok(settings.clone());
        }
        let settings = settings_file::read_program(&folder.join(SETTINGS))?;
        programs.insert(id.clone(), settings.clone());
        Ok(settings)
    }

    /// Changes the program `id`'s own settings, in `folder`, as `change` does, and writes them.
    /// The caller holds the store's lock and knows the program is installed.
    pub(crate) fn change_program(
        &self,
        id: &ProgramId,
        folder: &Path,
        change: impl FnOnce(&mut ProgramSettings),
    ) -> Result<(), StoreError> {
        let mut settings = self.program(id, folder)?;
        change(&mut settings);
        settings_file::write_program(&folder.join(SETTINGS), &settings)?;
        held(&self.programs).insert(id.clone(), settings);
        self.changed.notify_waiters();
        Ok(())
    }

    /// Forgets the program `id`'s settings, which went with its folder.
    pub(crate) fn forget(&self, id: &ProgramId) {
        held(&self.programs).remove(id);
        self.changed.notify_waiters();
    }

    /// Resolves at the next change of any settings.
    pub(crate) fn changed(&self) -> ChangedFuture {
        Box::pin(Arc::clone(&self.changed).notified_owned())
    }

    fn global_path(&self) -> PathBuf {
        self.directory.join(SETTINGS)
    }
}

/// A program's grants: its own settings over the global ones, read live, its answers kept in its
/// settings.
pub(crate) struct StoreGrants {
    pub(crate) store: crate::Store,
    pub(crate) id: ProgramId,
}

impl StoreGrants {
    /// What `read` reads of what holds for the program, or `nothing` when its settings cannot
    /// be read.
    fn read<T>(&self, read: impl FnOnce(&Holding<'_>) -> T, nothing: T) -> T {
        match self.settings() {
            Ok((global, own)) => read(&Holding {
                global: &global,
                own: &own,
            }),
            Err(error) => {
                tracing::warn!("a program is granted nothing: {error}");
                nothing
            }
        }
    }

    /// The global settings and the program's own.
    fn settings(&self) -> Result<(GlobalSettings, ProgramSettings), StoreError> {
        self.store.settings_of(&self.id)
    }
}

impl ProgramGrants for StoreGrants {
    fn network(&self) -> NetworkGrant {
        self.read(|holding| holding.network(), nothing_network())
    }

    fn local_network(&self) -> bool {
        self.read(|holding| holding.local_network(), false)
    }

    fn clipboard_read(&self) -> bool {
        self.read(|holding| holding.clipboard_read(), false)
    }

    fn clipboard_write(&self) -> bool {
        self.read(|holding| holding.clipboard_write(), false)
    }

    fn file_system(&self) -> FileSystemGrant {
        self.read(|holding| holding.file_system(), FileSystemGrant::default())
    }

    fn answer(&self, question: Question) -> Option<Answer> {
        self.read(|holding| holding.answer(question), Some(Answer::Denied))
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        let id = &self.id;
        let kept = self.store.change_program_settings(id, |settings| {
            settings.answers.set(question, Setting::Own(answer));
        });
        if let Err(error) = kept {
            tracing::warn!("the answer to {question:?} for {id} is lost: {error}");
        }
    }

    fn link_scheme_answer(&self, scheme: &str) -> Option<Answer> {
        self.read(
            |holding| holding.link_scheme_answer(scheme),
            Some(Answer::Denied),
        )
    }

    fn set_link_scheme_answer(&self, scheme: &str, answer: Answer) {
        let id = &self.id;
        let kept = self.store.change_program_settings(id, |settings| {
            settings.link_schemes.insert(scheme.to_owned(), answer);
        });
        if let Err(error) = kept {
            tracing::warn!("the answer to opening {scheme} links for {id} is lost: {error}");
        }
    }

    /// Off where the settings cannot be read, as nothing granted keeps the least.
    fn keeps_record(&self) -> bool {
        self.read(|holding| holding.keeps_record(), false)
    }

    fn role(&self, role: Role) -> Option<Answer> {
        self.read(|holding| holding.role(role), Some(Answer::Denied))
    }

    fn set_role(&self, role: Role, answer: Answer) {
        let id = &self.id;
        let kept = self.store.change_program_settings(id, |settings| {
            settings.roles.insert(role, answer);
        });
        if let Err(error) = kept {
            tracing::warn!("the answer to {id} holding the {role:?} role is lost: {error}");
        }
    }

    fn machines(&self) -> Vec<String> {
        self.read(|holding| holding.machines(), Vec::new())
    }

    /// The machine joins the program's own, unless it is among them.
    fn keep_machine(&self, id: &str) {
        let program = &self.id;
        let kept = self.store.change_program_settings(program, |settings| {
            if !settings.machines.iter().any(|kept| kept == id) {
                settings.machines.push(id.to_owned());
            }
        });
        if let Err(error) = kept {
            tracing::warn!(
                "the machine {id} picked for {program} is kept for its run alone: {error}"
            );
        }
    }

    /// The pick joins the program's own mounts, in place of one at its path: a program following
    /// the mounts for every program has them as its own from then on, as a change the user makes
    /// on its page does.
    fn keep_picked(&self, mount: Mount) {
        let id = &self.id;
        let global = match self.store.global_settings() {
            Ok(global) => global.file_system,
            Err(error) => {
                tracing::warn!("a pick of {id} is kept for its run alone: {error}");
                return;
            }
        };
        let kept = self.store.change_program_settings(id, |settings| {
            let mut own = settings.file_system.or(&global).clone();
            own.mounts.retain(|kept| kept.at != mount.at);
            own.mounts.push(mount);
            settings.file_system = Setting::Own(own);
        });
        if let Err(error) = kept {
            tracing::warn!("a pick of {id} is kept for its run alone: {error}");
        }
    }

    fn machine_answer(&self) -> Option<String> {
        self.read(|holding| holding.answer_with().machine.clone(), None)
    }

    fn audio_output_answer(&self) -> Option<String> {
        self.read(|holding| holding.answer_with().audio_output.clone(), None)
    }

    fn file_dialog_answer(&self, dialog: FileDialog) -> Option<Vec<PathBuf>> {
        self.read(|holding| holding.answer_with().file_dialog(dialog), None)
    }

    /// Silence where the settings cannot be read, as nothing granted is quietest.
    fn simulated_microphone(&self) -> SimulatedSound {
        self.read(
            |holding| holding.simulated_microphone(),
            SimulatedSound::Silence,
        )
    }

    fn changed(&self) -> ChangedFuture {
        self.store.settings_changed()
    }
}

fn nothing_network() -> NetworkGrant {
    NetworkGrant {
        on: false,
        domains: DomainRule::Only(Vec::new()),
    }
}

/// A lock over settings in memory, held for one read or write: a panic while it was held left
/// the value as it was before or after that one write.
fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
