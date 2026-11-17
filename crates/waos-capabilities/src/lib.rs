//! The capability traits and their records, in WIT's shape: one trait per service a program uses
//! (files, network, machines, sound devices, cameras, media, fonts), each in the shape of its
//! `wasdk:platform` or `wasdk:media` interface; [`ProgramEnvironment`], which holds one program's providers, what
//! the user lets it use and where its output goes; and [`ProgramGrants`], what the user lets one
//! program use, with its values, which the store implements over the user's settings.
//!
//! The clipboard is not a capability: the platform has one, which every program shares, and the
//! embedding app gives it to the engine whole. Only its switches are here, in [`ProgramGrants`].
//! Opening links and files is not one either: it is the system's, which the embedding app reaches;
//! only a program's answers to its link schemes are here. Nor are notifications: they are the
//! user's, shown by the system, which the embedding app reaches; only the answer to a program's
//! question for them is here.
//!
//! Fonts are the one capability whose trait is not here: its provider is valo's
//! [`FontManager`](valo_fontmgr::FontManager), the platform's fonts in the shape of Skia's
//! `SkFontMgr`, which the engine serves `wasdk:platform/fonts` from (the design's decision 33).
//! The crate re-exports [`valo_fontmgr`], so whoever provides fonts names the version the field
//! holds.
//!
//! Sound's trait, [`AudioDevices`], is the devices alone: Web Audio's graph is `web_audio_api`'s,
//! which the engine serves node for node, and a provider makes its contexts on an output, lists
//! the outputs and opens the microphone. The crate re-exports [`web_audio_api`] for the same
//! reason as [`valo_fontmgr`].
//!
//! A machine's traits, [`Machines`] and [`Machine`], are a provider of one kind of machine and a
//! machine it offers: its files from its root, its commands, with pipes or in a terminal (the
//! processes proposal's decisions 2 and 5), what it runs ([`Platform`]) and its user's login
//! shell. Which machines a program holds is among its
//! grants, by their ids.
//!
//! A provider called from the program's thread is called on a stack of the engine's, not the
//! thread's own: a program's code runs on a stack wasmtime makes for it, a fiber's, and so does
//! what it calls. A system that takes a call only on the thread's own stack is reached from
//! another thread: Android's runtime aborts the process when a thread first enters Java
//! anywhere else, and refuses each later call into Java made there.
//!
//! What a program did with what it was allowed is counted by the engine one day at a time, an
//! [`AuditDay`], and kept by whoever implements [`AuditLog`], the store for the WaOS app.
//!
//! The invariant: this crate depends on nothing of ours, so a capability crate can depend on it
//! alone. A trait here is a service's shape, never a provider: no OS, no shell, no program's
//! internals.

mod audio;
mod audit;
mod grants;
mod machines;

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use audio::{AudioDeviceError, AudioDevices, AudioOutput};
pub use audit::{
    AskedQuestion, AuditDay, AuditLog, DAYS_KEPT, DeviceSession, FolderUses, LIST_CAPS, ListCaps,
    MachineCommand, MountKind, NamedUses, Overflow, Uses,
};
pub use grants::{
    Answer, ChangedFuture, Domain, DomainError, DomainRule, FileDialog, FileSystemGrant, Mount,
    NetworkGrant, ProgramGrants, Question, Role, Setting, SimulatedSound, Switch,
};
pub use machines::{
    Architecture, Child, ChildInput, Command, ExecError, ExitStatus, InTerminal, Machine,
    MachineChoice, MachineFiles, Machines, OperatingSystem, Platform, Signal, Terminal,
    TerminalSize, WithPipes,
};
pub use valo_fontmgr;
use valo_fontmgr::FontManager;
pub use web_audio_api;

/// The providers one program is served by, chosen when it starts: one field per capability,
/// each a provider of that capability's trait, and where the program's output goes. The engine
/// takes one with every program it runs.
///
/// Step 7 of the design's work order adds each capability's field as it arrives, with its
/// interface shown to the lead first.
#[derive(Default)]
pub struct ProgramEnvironment {
    /// Where what the program writes to its standard output and error goes, as it writes it;
    /// with none, the engine writes it to its log (the design's decision 31).
    pub output: Option<Arc<dyn ProgramOutput>>,
    /// The fonts the program is given: the manager its `wasdk:platform/fonts` answers from,
    /// behind a lock since one manager serves every program and caches its lookups (the design's
    /// decision 35). With none, every answer is empty, as on a machine with no fonts installed.
    pub fonts: Option<Arc<Mutex<dyn FontManager>>>,
    /// What the user lets the program use, read at each check. With none, nothing is granted:
    /// every switch is off, nothing is mounted, and every question is denied without asking.
    pub grants: Option<Arc<dyn ProgramGrants>>,
    /// The program's own folders, which it sees at `/data` and `/cache` whatever its grants.
    /// With none, it has neither.
    pub folders: Option<ProgramFolders>,
    /// The sound devices the program's graph plays on and records from. With none, it has no
    /// sound: its `get-audio` answers none.
    pub audio: Option<Arc<dyn AudioDevices>>,
    /// The providers of the machines the program may be given, each of one kind, which its picker
    /// offers the user. With none, it has no machines: its `get-machines` answers none.
    pub machines: Vec<Arc<dyn Machines>>,
    /// The variables the program reads besides the engine's own, each its name and value, as
    /// whoever runs it gave them: a development run's `--env` (the design's decision 68). With
    /// none, it reads only the engine's own, `TMPDIR`, which wins where both name one.
    pub variables: Vec<(String, String)>,
}

/// A program's own folders on the machine, free of any grant: what it keeps, which it sees at
/// `/data`, and what it keeps that may be thrown away, at `/cache`. Whoever runs the program
/// makes them; the engine mounts them, writable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramFolders {
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl fmt::Debug for ProgramEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgramEnvironment")
            .field("output", &self.output.as_ref().map(|_| "chosen"))
            .field("fonts", &self.fonts.as_ref().map(|_| "given"))
            .field("grants", &self.grants.as_ref().map(|_| "given"))
            .field("folders", &self.folders)
            .field("audio", &self.audio.as_ref().map(|_| "given"))
            .field("machines", &self.machines.len())
            .field("variables", &self.variables.len())
            .finish()
    }
}

/// Where a program's standard output and error go: whoever runs the program chooses, as
/// `Command::stdout` does for a process. A run from `cargo waos` writes to its connection; the
/// app's log window, later, is another.
pub trait ProgramOutput: Send + Sync {
    /// Bytes the program wrote to `stream`, as it wrote them: a line may come in pieces, and a
    /// prompt without its newline is a write of its own. Called from the program's thread, so
    /// it must not wait on anything the program's thread does.
    fn write(&self, stream: OutputStream, bytes: &[u8]);
}

/// Which of a program's two standard streams some bytes went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_says_whether_an_output_was_chosen_without_printing_it() {
        struct Nowhere;
        impl ProgramOutput for Nowhere {
            fn write(&self, _stream: OutputStream, _bytes: &[u8]) {}
        }
        assert_eq!(
            format!("{:?}", ProgramEnvironment::default()),
            "ProgramEnvironment { output: None, fonts: None, grants: None, folders: None, audio: None, machines: 0, variables: 0 }"
        );
        let chosen = ProgramEnvironment {
            output: Some(Arc::new(Nowhere)),
            ..ProgramEnvironment::default()
        };
        assert_eq!(
            format!("{chosen:?}"),
            "ProgramEnvironment { output: Some(\"chosen\"), fonts: None, grants: None, folders: None, audio: None, machines: 0, variables: 0 }"
        );
    }

    #[test]
    fn an_environment_says_whether_fonts_were_given_without_listing_them() {
        let given = ProgramEnvironment {
            fonts: Some(Arc::new(Mutex::new(NoFonts))),
            ..ProgramEnvironment::default()
        };
        assert_eq!(
            format!("{given:?}"),
            "ProgramEnvironment { output: None, fonts: Some(\"given\"), grants: None, folders: None, audio: None, machines: 0, variables: 0 }"
        );
    }

    /// A machine with no fonts installed.
    struct NoFonts;

    impl FontManager for NoFonts {
        fn family(&mut self, _family: &str) -> Vec<valo_fontmgr::Typeface> {
            Vec::new()
        }

        fn match_character(
            &mut self,
            _family: Option<&str>,
            _style: valo_fontmgr::Style,
            _locales: &[&str],
            _character: char,
        ) -> Option<valo_fontmgr::Typeface> {
            None
        }

        fn system_font(
            &mut self,
            _size: f32,
            _style: valo_fontmgr::Style,
        ) -> Option<valo_fontmgr::Typeface> {
            None
        }

        fn families(&self) -> Vec<String> {
            Vec::new()
        }

        fn face_count(&self) -> usize {
            0
        }

        fn watch(
            &mut self,
            _on_change: Box<dyn Fn() + Send + Sync>,
        ) -> Option<valo_fontmgr::Watch> {
            None
        }
    }
}
