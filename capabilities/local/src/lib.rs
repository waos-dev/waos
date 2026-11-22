//! This computer's defaults, assembled: one provider per capability from the `local-*` crates, as
//! a `ProgramEnvironment`, for an embedder that wants what the machine in front of the user has.
//!
//! The invariant: this crate assembles; it implements nothing itself. A provider lives in its own
//! `local-*` crate, so an embedder can pick one without the rest, or in valo: the platform's font
//! manager, which [`LocalCapabilities`] makes once, so every program it serves shares its
//! lookups and its files' mappings; this computer's sound devices, `waos-local-audio`'s; and this
//! computer as a machine, `waos-local-processes`', its commands run in the user's login
//! environment, read once for every program.

use std::sync::{Arc, Mutex};

use waos_capabilities::valo_fontmgr::{self, FontManager};
use waos_capabilities::{AudioDevices, Machines, ProgramEnvironment};
use waos_local_audio::LocalAudio;
use waos_local_processes::{LocalMachines, LoginEnvironment};

pub use waos_local_processes::{HomeFolder, THIS_COMPUTER};

/// This computer's providers, made once and shared by every program they serve: an embedder keeps
/// one and gives each program it runs an [`environment`](LocalCapabilities::environment).
pub struct LocalCapabilities {
    /// This computer's fonts: valo's font manager for this platform (the design's decision 33).
    fonts: Arc<Mutex<dyn FontManager>>,
    /// This computer's sound devices, through the system's audio.
    audio: Arc<dyn AudioDevices>,
    /// This computer, as a machine a program's user may pick for it.
    machines: Arc<dyn Machines>,
}

impl LocalCapabilities {
    /// This computer's providers. On a platform without a font API the font manager scans the
    /// system's font folders as it is made, so make one and keep it.
    pub fn new() -> LocalCapabilities {
        LocalCapabilities::with_audio(LocalAudio::new())
    }

    /// This computer's providers, its sound without the microphone, for an app that may not record
    /// sound, as one whose bundle does not say why it does.
    pub fn without_microphone() -> LocalCapabilities {
        LocalCapabilities::with_audio(LocalAudio::without_microphone())
    }

    fn with_audio(audio: LocalAudio) -> LocalCapabilities {
        LocalCapabilities {
            fonts: Arc::new(Mutex::new(valo_fontmgr::platform())),
            audio: Arc::new(audio),
            machines: Arc::new(LocalMachines::new(LoginEnvironment::of_login_shell())),
        }
    }

    /// These providers, `home` the user's home on this computer, where the embedder knows it
    /// better than the login environment, as on Android, which gives an app no home of its own.
    pub fn with_home(mut self, home: HomeFolder) -> LocalCapabilities {
        let login = LoginEnvironment::of_login_shell();
        self.machines = Arc::new(LocalMachines::with_home(login, home));
        self
    }

    /// A program's environment of this computer's providers, the same ones every program gets,
    /// with its output left to whoever runs it.
    pub fn environment(&self) -> ProgramEnvironment {
        ProgramEnvironment {
            fonts: Some(Arc::clone(&self.fonts)),
            audio: Some(Arc::clone(&self.audio)),
            machines: vec![Arc::clone(&self.machines)],
            ..ProgramEnvironment::default()
        }
    }
}

impl Default for LocalCapabilities {
    fn default() -> LocalCapabilities {
        LocalCapabilities::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_environment_shares_one_font_manager() {
        let local = LocalCapabilities::new();
        let (first, second) = (local.environment(), local.environment());
        let (first, second) = (first.fonts.unwrap(), second.fonts.unwrap());
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn every_environment_has_this_computer_s_sound() {
        let local = LocalCapabilities::new();
        let (first, second) = (local.environment(), local.environment());
        let (first, second) = (first.audio.unwrap(), second.audio.unwrap());
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn every_environment_has_this_computer_as_its_one_machine_provider() {
        let local = LocalCapabilities::new();
        let (first, second) = (local.environment(), local.environment());
        assert_eq!((first.machines.len(), second.machines.len()), (1, 1));
        assert!(Arc::ptr_eq(&first.machines[0], &second.machines[0]));
    }
}
