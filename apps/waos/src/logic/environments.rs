//! What each program runs with: this computer's capabilities, made once and shared, its sound
//! devices among them unless others are given; what the user lets it use, read live from the
//! store's settings; and its own folders, which the store keeps.
//! A program is the one its id names, whether the library opened it or `waos` sent it
//! (decision 11 of the permissions proposal): installed or not, it has its own settings over the
//! global ones, and its own `/data` and `/cache`.

use std::sync::Arc;

use waos::{AudioDevices, AudioOutput, MachineChoice, ProgramEnvironment};
use waos_local::LocalCapabilities;
use waos_package::ProgramId;
use waos_store::{Store, StoreError};

use super::setting_changes::Offered;

/// The environments the app's programs run in.
pub(crate) struct ProgramEnvironments {
    capabilities: LocalCapabilities,
    /// The sound devices every program plays on and records from; none for no sound.
    audio: Option<Arc<dyn AudioDevices>>,
    store: Store,
}

impl ProgramEnvironments {
    /// Environments over `capabilities`, this computer's, and `store`'s settings and folders.
    pub(crate) fn new(capabilities: LocalCapabilities, store: Store) -> Arc<ProgramEnvironments> {
        let audio = capabilities.environment().audio;
        ProgramEnvironments::with_audio(capabilities, audio, store)
    }

    /// [`new`](Self::new), with `audio` as the sound devices in place of `capabilities`' own.
    pub(crate) fn with_audio(
        capabilities: LocalCapabilities,
        audio: Option<Arc<dyn AudioDevices>>,
        store: Store,
    ) -> Arc<ProgramEnvironments> {
        Arc::new(ProgramEnvironments {
            capabilities,
            audio,
            store,
        })
    }

    /// What the program `id` runs with: its settings, and its own folders, made at its first
    /// run; folders that cannot be made refuse the run.
    pub(crate) fn of(&self, id: &ProgramId) -> Result<ProgramEnvironment, StoreError> {
        Ok(ProgramEnvironment {
            grants: Some(self.store.grants(id)),
            folders: Some(self.store.folders(id)?),
            audio: self.audio.clone(),
            ..self.capabilities.environment()
        })
    }

    /// What the programs' pickers offer now: the machines and the sound outputs.
    pub(crate) fn offered(&self) -> Offered {
        Offered {
            machines: self.machines_offered(),
            outputs: self.outputs_offered(),
        }
    }

    /// The sound outputs this computer offers the programs, as the user knows them; none without
    /// sound devices.
    fn outputs_offered(&self) -> Vec<AudioOutput> {
        self.audio
            .as_ref()
            .map(|devices| devices.outputs())
            .unwrap_or_default()
    }

    /// The machines this computer's providers offer the programs, as the user knows them.
    fn machines_offered(&self) -> Vec<MachineChoice> {
        self.capabilities
            .environment()
            .machines
            .iter()
            .flat_map(|machines| machines.offered())
            .collect()
    }
}
