//! What the user lets apps use: the global settings, which every app follows until the user sets
//! its own, and one app's own settings, each a [`Setting`] that is the global default or its own
//! value.
//!
//! The global settings start as the design's defaults: the network on for any domain, the local
//! network off, the clipboard on both ways, nothing mounted, and every question to be asked. An
//! app's answers to opening links of a scheme of its own are its own alone, with no global
//! default: each scheme is asked of its user once; and so are its roles, the machines its user
//! picked for it, and the answers its user set for its pickers and file dialogs, given in place of
//! asking.

use std::collections::BTreeMap;
use std::path::PathBuf;

use waos_capabilities::{
    Answer, DomainRule, FileDialog, FileSystemGrant, NetworkGrant, Question, Role, Setting,
    SimulatedSound,
};

/// What every app follows until the user sets its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalSettings {
    pub network: NetworkGrant,
    /// Whether programs may reach this computer and the local network.
    pub local_network: bool,
    pub clipboard_read: bool,
    pub clipboard_write: bool,
    pub file_system: FileSystemGrant,
    /// The answer each question gets without asking; none to ask the user.
    pub answers: PerQuestion<Option<Answer>>,
    /// What a microphone answered simulated records.
    pub simulated_microphone: SimulatedSound,
    /// Whether a record is kept of what each program does, its audit summary: on until the user
    /// turns it off.
    pub record: bool,
}

impl Default for GlobalSettings {
    fn default() -> GlobalSettings {
        GlobalSettings {
            network: NetworkGrant {
                on: true,
                domains: DomainRule::Any,
            },
            local_network: false,
            clipboard_read: true,
            clipboard_write: true,
            file_system: FileSystemGrant::default(),
            answers: PerQuestion::default(),
            simulated_microphone: SimulatedSound::default(),
            record: true,
        }
    }
}

/// One app's own settings, each the global default until the user sets it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppSettings {
    pub network: Setting<NetworkGrant>,
    pub local_network: Setting<bool>,
    pub clipboard_read: Setting<bool>,
    pub clipboard_write: Setting<bool>,
    /// Its mounts: a change applies at once to the program running.
    pub file_system: Setting<FileSystemGrant>,
    /// The user's answer to each question, or the global one.
    pub answers: PerQuestion<Setting<Answer>>,
    /// What its microphone records while answered simulated, or the global one.
    pub simulated_microphone: Setting<SimulatedSound>,
    /// Whether a record is kept of what it does, or the global one; off, what was kept of it
    /// is removed.
    pub record: Setting<bool>,
    /// The user's answer to the program opening links of each scheme it asked for, by the
    /// scheme in lower case: those other than `http`, `https` and `mailto`, which open freely.
    pub link_schemes: BTreeMap<String, Answer>,
    /// The user's answer to it holding each role it asked for: its own alone, since a role has
    /// no setting for every program.
    pub roles: BTreeMap<Role, Answer>,
    /// The machines the user picked for it, by their ids, in the order picked: its own alone.
    pub machines: Vec<String>,
    /// The answers the user set for its pickers and file dialogs: its own alone.
    pub answer_with: AnswerWith,
}

/// What a program's pickers and file dialogs answer in place of asking its user, as its user set
/// it; each none to ask.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnswerWith {
    /// A machine, by its id.
    pub machine: Option<String>,
    /// A sound output, by its id.
    pub audio_output: Option<String>,
    /// Paths on the machine, absolute.
    pub open_file: Option<PathBuf>,
    pub open_files: Option<Vec<PathBuf>>,
    pub open_folder: Option<PathBuf>,
    pub save_file: Option<PathBuf>,
}

impl AnswerWith {
    /// The paths set for `dialog`; none to ask.
    pub fn file_dialog(&self, dialog: FileDialog) -> Option<Vec<PathBuf>> {
        match dialog {
            FileDialog::OpenFile => self.open_file.clone().map(|path| vec![path]),
            FileDialog::OpenFiles => self.open_files.clone(),
            FileDialog::OpenFolder => self.open_folder.clone().map(|path| vec![path]),
            FileDialog::SaveFile => self.save_file.clone().map(|path| vec![path]),
        }
    }

    /// Sets `paths` as the answer to `dialog`, or none to ask; the first path alone for a dialog
    /// that answers one, and an empty list asks.
    pub fn set_file_dialog(&mut self, dialog: FileDialog, paths: Option<Vec<PathBuf>>) {
        let paths = paths.filter(|paths| !paths.is_empty());
        let first = paths.as_ref().and_then(|paths| paths.first().cloned());
        match dialog {
            FileDialog::OpenFile => self.open_file = first,
            FileDialog::OpenFiles => self.open_files = paths,
            FileDialog::OpenFolder => self.open_folder = first,
            FileDialog::SaveFile => self.save_file = first,
        }
    }
}

/// One value for each question.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PerQuestion<T> {
    pub notifications: T,
    pub camera: T,
    pub microphone: T,
}

impl<T> PerQuestion<T> {
    pub fn get(&self, question: Question) -> &T {
        match question {
            Question::Notifications => &self.notifications,
            Question::Camera => &self.camera,
            Question::Microphone => &self.microphone,
        }
    }

    pub fn set(&mut self, question: Question, value: T) {
        match question {
            Question::Notifications => self.notifications = value,
            Question::Camera => self.camera = value,
            Question::Microphone => self.microphone = value,
        }
    }
}

/// What holds for an app whose own settings are `own`, under `global`.
pub(crate) struct Holding<'a> {
    pub(crate) global: &'a GlobalSettings,
    pub(crate) own: &'a AppSettings,
}

impl Holding<'_> {
    pub(crate) fn network(&self) -> NetworkGrant {
        self.own.network.or(&self.global.network).clone()
    }

    pub(crate) fn local_network(&self) -> bool {
        *self.own.local_network.or(&self.global.local_network)
    }

    pub(crate) fn clipboard_read(&self) -> bool {
        *self.own.clipboard_read.or(&self.global.clipboard_read)
    }

    pub(crate) fn clipboard_write(&self) -> bool {
        *self.own.clipboard_write.or(&self.global.clipboard_write)
    }

    pub(crate) fn keeps_record(&self) -> bool {
        *self.own.record.or(&self.global.record)
    }

    pub(crate) fn file_system(&self) -> FileSystemGrant {
        self.own.file_system.or(&self.global.file_system).clone()
    }

    pub(crate) fn answer(&self, question: Question) -> Option<Answer> {
        match self.own.answers.get(question) {
            Setting::Own(answer) => Some(*answer),
            Setting::Global => *self.global.answers.get(question),
        }
    }

    pub(crate) fn simulated_microphone(&self) -> SimulatedSound {
        *self
            .own
            .simulated_microphone
            .or(&self.global.simulated_microphone)
    }

    pub(crate) fn link_scheme_answer(&self, scheme: &str) -> Option<Answer> {
        self.own.link_schemes.get(scheme).copied()
    }

    pub(crate) fn role(&self, role: Role) -> Option<Answer> {
        self.own.roles.get(&role).copied()
    }

    pub(crate) fn machines(&self) -> Vec<String> {
        self.own.machines.clone()
    }

    pub(crate) fn answer_with(&self) -> &AnswerWith {
        &self.own.answer_with
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_follows_the_global_settings_until_it_has_its_own() {
        let global = GlobalSettings::default();
        let mut own = AppSettings::default();
        let holding = Holding {
            global: &global,
            own: &own,
        };
        assert!(holding.network().on && holding.clipboard_read() && holding.clipboard_write());
        assert_eq!(holding.file_system(), FileSystemGrant::default());
        assert_eq!(holding.answer(Question::Camera), None, "to be asked");

        own.network = Setting::Own(NetworkGrant {
            on: false,
            domains: DomainRule::Any,
        });
        own.answers
            .set(Question::Camera, Setting::Own(Answer::Denied));
        let holding = Holding {
            global: &global,
            own: &own,
        };
        assert!(!holding.network().on);
        assert_eq!(holding.answer(Question::Camera), Some(Answer::Denied));
        assert_eq!(holding.answer(Question::Microphone), None);
    }

    #[test]
    fn a_simulated_microphone_plays_the_tone_unless_silence_is_chosen_for_it_or_every_program() {
        let mut global = GlobalSettings::default();
        let mut own = AppSettings::default();
        fn holding(global: &GlobalSettings, own: &AppSettings) -> SimulatedSound {
            Holding { global, own }.simulated_microphone()
        }
        assert_eq!(holding(&global, &own), SimulatedSound::Tone);
        global.simulated_microphone = SimulatedSound::Silence;
        assert_eq!(
            holding(&global, &own),
            SimulatedSound::Silence,
            "the global one"
        );
        own.simulated_microphone = Setting::Own(SimulatedSound::Tone);
        assert_eq!(holding(&global, &own), SimulatedSound::Tone, "its own");
    }

    #[test]
    fn a_link_scheme_is_asked_until_the_program_has_its_answer() {
        let global = GlobalSettings::default();
        let mut own = AppSettings::default();
        own.link_schemes
            .insert("zoommtg".to_owned(), Answer::Granted);
        let holding = Holding {
            global: &global,
            own: &own,
        };
        assert_eq!(holding.link_scheme_answer("zoommtg"), Some(Answer::Granted));
        assert_eq!(holding.link_scheme_answer("tel"), None, "to be asked");
    }

    #[test]
    fn a_dialog_answers_the_paths_set_for_it_and_a_single_one_its_first() {
        let mut set = AnswerWith::default();
        let (a, b) = (PathBuf::from("/a.txt"), PathBuf::from("/b.txt"));
        set.set_file_dialog(FileDialog::OpenFile, Some(vec![a.clone(), b.clone()]));
        set.set_file_dialog(FileDialog::OpenFiles, Some(vec![a.clone(), b.clone()]));
        assert_eq!(set.file_dialog(FileDialog::OpenFile), Some(vec![a.clone()]));
        assert_eq!(set.file_dialog(FileDialog::OpenFiles), Some(vec![a, b]));
        assert_eq!(set.file_dialog(FileDialog::SaveFile), None, "to be asked");
        set.set_file_dialog(FileDialog::OpenFiles, Some(Vec::new()));
        assert_eq!(
            set.file_dialog(FileDialog::OpenFiles),
            None,
            "none set asks"
        );
    }

    #[test]
    fn a_global_answer_holds_for_every_program_without_its_own() {
        let mut global = GlobalSettings::default();
        global
            .answers
            .set(Question::Notifications, Some(Answer::Granted));
        let own = AppSettings::default();
        let holding = Holding {
            global: &global,
            own: &own,
        };
        assert_eq!(
            holding.answer(Question::Notifications),
            Some(Answer::Granted)
        );
    }
}
