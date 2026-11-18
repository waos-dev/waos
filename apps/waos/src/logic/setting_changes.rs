//! What a settings page shows and what the user changes on it, over the store's settings: a page
//! is the global settings, or one program's, each of whose settings follows the global one until
//! the user changes it there, and can be set back to follow it. Every layout of the library
//! shows a page as [`ShownSettings`] and changes it with a [`SettingChange`].
//!
//! A program's page also shows the machines its user picked for it, each by the name its provider
//! gives, which the user removes to take it back; the answers its user set for its pickers and
//! file dialogs, given in place of asking, each Ask until set; and its answers to opening links of
//! its own schemes, which the user removes to be asked again. They are the program's own, with no
//! global setting.
//!
//! The invariant: a change to a program's setting that follows the global one starts from the
//! global value, so what the user sees before the change is what the change is made to.

use std::path::{Path, PathBuf};

use waos::SimulatedSound;
use waos::{AudioOutput, FileDialog, MachineChoice, file_dialog_answer_refusal, machine_paths};
use waos_dev_protocol::resolved_lexically;
use waos_package::ProgramId;
use waos_store::{
    Answer, AnswerWith, Domain, DomainRule, FileSystemGrant, GlobalSettings, Mount, NetworkGrant,
    PerQuestion, ProgramSettings, Question, Role, Setting,
};

/// Whose settings a page shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SettingsOf {
    Global,
    Program(ProgramId),
}

/// One setting as a page shows it: what holds, and whether it is the global one, followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shown<T> {
    pub(crate) value: T,
    /// Always false on the global page.
    pub(crate) follows_global: bool,
}

/// A page's settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShownSettings {
    pub(crate) network: Shown<NetworkGrant>,
    pub(crate) local_network: Shown<bool>,
    pub(crate) clipboard_read: Shown<bool>,
    pub(crate) clipboard_write: Shown<bool>,
    /// Whether a record is kept of what the program does, or of what every program does.
    pub(crate) record: Shown<bool>,
    pub(crate) file_system: Shown<FileSystemGrant>,
    /// Each question's answer that holds; none to ask.
    pub(crate) answers: PerQuestion<Shown<Option<Answer>>>,
    /// What a simulated microphone records, as it holds: the program's, or the global one while
    /// its microphone follows the setting for every program.
    pub(crate) simulated_microphone: Shown<SimulatedSound>,
    /// The machines the user picked for the program, each with its name; none on the global page.
    pub(crate) machines: Vec<MachineChoice>,
    /// The answers the user set for the program's pickers and file dialogs, each none to ask; none
    /// set on the global page.
    pub(crate) answer_with: ShownAnswerWith,
    /// The program's answers to opening links of its own schemes, by scheme; none on the global
    /// page.
    pub(crate) link_schemes: Vec<(String, Answer)>,
    /// The program's answers to it holding each role it asked for; none on the global page.
    pub(crate) roles: Vec<(Role, Answer)>,
    /// What the program's pickers offer now, which its page sets their answers from.
    pub(crate) offered: Offered,
}

/// The answers set for a program's pickers and file dialogs, as its page shows them: a machine and
/// an output by the name each has where it is offered, by its id where it is not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ShownAnswerWith {
    pub(crate) machine: Option<MachineChoice>,
    pub(crate) audio_output: Option<AudioOutput>,
    /// The paths set for each file dialog.
    pub(crate) paths: AnswerWith,
}

/// What a program's pickers offer now: this computer's providers' machines and its sound outputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Offered {
    pub(crate) machines: Vec<MachineChoice>,
    pub(crate) outputs: Vec<AudioOutput>,
}

/// One of a page's settings, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    Network,
    LocalNetwork,
    ClipboardRead,
    ClipboardWrite,
    Record,
    FileSystem,
    Answer(Question),
    /// What a simulated microphone records.
    SimulatedMicrophone,
}

/// Which domains the network rule names, without the names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DomainsKind {
    Any,
    Only,
    AnyBut,
}

/// What the user changes on a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SettingChange {
    NetworkOn(bool),
    Domains(DomainsKind),
    /// A domain the user typed, added to the rule's list.
    AddDomain(String),
    RemoveDomain(Domain),
    LocalNetwork(bool),
    ClipboardRead(bool),
    ClipboardWrite(bool),
    /// Whether a record is kept of what the program does; off removes what was kept.
    Record(bool),
    /// A folder of the machine, mounted at `at`, or at its own path when `at` is empty.
    AddMount {
        host: String,
        at: String,
        writable: bool,
    },
    RemoveMount(usize),
    /// A machine, by its id, taken back from the program.
    RemoveMachine(String),
    /// The answer a question gets: none to ask, which the global page alone offers; simulated for
    /// the camera and the microphone alone.
    Answer(Question, Option<Answer>),
    /// What a simulated microphone records.
    SimulatedMicrophone(SimulatedSound),
    /// The answer to opening links of a scheme, in lower case: none to ask again.
    LinkScheme(String, Option<Answer>),
    /// The answer to the program holding a role: none to ask again at its next start.
    Role(Role, Option<Answer>),
    /// The machine, by its id, the picker of machines answers: none to ask.
    MachineAnswer(Option<String>),
    /// The output, by its id, the picker of outputs answers: none to ask.
    AudioOutputAnswer(Option<String>),
    /// The paths, as typed, a file dialog answers, each a whole path of this computer that the
    /// dialog could have chosen: none to ask.
    FileDialogAnswer(FileDialog, Vec<String>),
    /// A path, as typed, added to those the dialog that opens files answers.
    AddFileToOpen(String),
    /// The path at an index taken from those the dialog that opens files answers.
    RemoveFileToOpen(usize),
    /// A program's setting follows the global one again.
    FollowGlobal(Which),
}

/// The global page.
pub(crate) fn global_page(global: &GlobalSettings) -> ShownSettings {
    ShownSettings {
        network: Shown::global(global.network.clone()),
        local_network: Shown::global(global.local_network),
        clipboard_read: Shown::global(global.clipboard_read),
        clipboard_write: Shown::global(global.clipboard_write),
        record: Shown::global(global.record),
        file_system: Shown::global(global.file_system.clone()),
        answers: PerQuestion {
            notifications: Shown::global(global.answers.notifications),
            camera: Shown::global(global.answers.camera),
            microphone: Shown::global(global.answers.microphone),
        },
        simulated_microphone: Shown::global(global.simulated_microphone),
        machines: Vec::new(),
        answer_with: ShownAnswerWith::default(),
        link_schemes: Vec::new(),
        roles: Vec::new(),
        offered: Offered::default(),
    }
}

impl<T> Shown<T> {
    /// `value`, as the global page shows it.
    fn global(value: T) -> Shown<T> {
        Shown {
            value,
            follows_global: false,
        }
    }
}

/// A program's page, its own settings `own` over `global`, its machines and outputs named as
/// `offered` names them, by their ids where nothing offers them now.
pub(crate) fn program_page(
    global: &GlobalSettings,
    own: &ProgramSettings,
    offered: &Offered,
) -> ShownSettings {
    let answer = |question: Question| match own.answers.get(question) {
        Setting::Own(answer) => Shown {
            value: Some(*answer),
            follows_global: false,
        },
        Setting::Global => Shown {
            value: *global.answers.get(question),
            follows_global: true,
        },
    };
    ShownSettings {
        network: shown(&own.network, &global.network),
        local_network: shown(&own.local_network, &global.local_network),
        clipboard_read: shown(&own.clipboard_read, &global.clipboard_read),
        clipboard_write: shown(&own.clipboard_write, &global.clipboard_write),
        record: shown(&own.record, &global.record),
        file_system: shown(&own.file_system, &global.file_system),
        answers: PerQuestion {
            notifications: answer(Question::Notifications),
            camera: answer(Question::Camera),
            microphone: answer(Question::Microphone),
        },
        simulated_microphone: shown(&own.simulated_microphone, &global.simulated_microphone),
        machines: own
            .machines
            .iter()
            .map(|id| machine_named(id, &offered.machines))
            .collect(),
        answer_with: ShownAnswerWith {
            machine: own
                .answer_with
                .machine
                .as_deref()
                .map(|id| machine_named(id, &offered.machines)),
            audio_output: own
                .answer_with
                .audio_output
                .as_deref()
                .map(|id| output_named(id, &offered.outputs)),
            paths: own.answer_with.clone(),
        },
        link_schemes: own
            .link_schemes
            .iter()
            .map(|(scheme, answer)| (scheme.clone(), *answer))
            .collect(),
        roles: own
            .roles
            .iter()
            .map(|(role, answer)| (*role, *answer))
            .collect(),
        offered: offered.clone(),
    }
}

/// The output `id` names, with the label `offered` gives it, or its id.
fn output_named(id: &str, offered: &[AudioOutput]) -> AudioOutput {
    offered
        .iter()
        .find(|output| output.id == id)
        .cloned()
        .unwrap_or_else(|| AudioOutput {
            id: id.to_owned(),
            group_id: None,
            label: id.to_owned(),
        })
}

/// The machine `id` names, with the name `offered` gives it, or its id.
fn machine_named(id: &str, offered: &[MachineChoice]) -> MachineChoice {
    offered
        .iter()
        .find(|machine| machine.id == id)
        .cloned()
        .unwrap_or_else(|| MachineChoice {
            id: id.to_owned(),
            name: id.to_owned(),
        })
}

fn shown<T: Clone>(own: &Setting<T>, global: &T) -> Shown<T> {
    Shown {
        value: own.or(global).clone(),
        follows_global: matches!(own, Setting::Global),
    }
}

/// Makes `change` to the global settings; text that names nothing is refused with why.
pub(crate) fn change_global(
    global: &mut GlobalSettings,
    change: SettingChange,
) -> Result<(), String> {
    match change {
        SettingChange::LocalNetwork(on) => global.local_network = on,
        SettingChange::ClipboardRead(on) => global.clipboard_read = on,
        SettingChange::ClipboardWrite(on) => global.clipboard_write = on,
        SettingChange::Record(on) => global.record = on,
        SettingChange::Answer(question, answer) => {
            refuse_misplaced_simulation(question, answer)?;
            global.answers.set(question, answer);
        }
        SettingChange::SimulatedMicrophone(sound) => global.simulated_microphone = sound,
        SettingChange::FollowGlobal(_)
        | SettingChange::RemoveMachine(_)
        | SettingChange::LinkScheme(..)
        | SettingChange::Role(..)
        | SettingChange::MachineAnswer(_)
        | SettingChange::AudioOutputAnswer(_)
        | SettingChange::FileDialogAnswer(..)
        | SettingChange::AddFileToOpen(_)
        | SettingChange::RemoveFileToOpen(_) => {}
        SettingChange::NetworkOn(_)
        | SettingChange::Domains(_)
        | SettingChange::AddDomain(_)
        | SettingChange::RemoveDomain(_) => change_network(&mut global.network, change)?,
        SettingChange::AddMount { .. } | SettingChange::RemoveMount(_) => {
            change_mounts(&mut global.file_system, change)?;
        }
    }
    Ok(())
}

/// Makes `change` to a program's own settings `own`, a setting that follows `global` starting
/// from the global value; text that names nothing is refused with why.
pub(crate) fn change_program(
    own: &mut ProgramSettings,
    global: &GlobalSettings,
    change: SettingChange,
) -> Result<(), String> {
    match change {
        SettingChange::LocalNetwork(on) => own.local_network = Setting::Own(on),
        SettingChange::ClipboardRead(on) => own.clipboard_read = Setting::Own(on),
        SettingChange::ClipboardWrite(on) => own.clipboard_write = Setting::Own(on),
        SettingChange::Record(on) => own.record = Setting::Own(on),
        SettingChange::Answer(question, answer) => {
            refuse_misplaced_simulation(question, answer)?;
            let setting = answer.map_or(Setting::Global, Setting::Own);
            own.answers.set(question, setting);
        }
        SettingChange::SimulatedMicrophone(sound) => {
            own.simulated_microphone = Setting::Own(sound);
        }
        SettingChange::FollowGlobal(which) => follow_global(own, which),
        SettingChange::RemoveMachine(id) => own.machines.retain(|kept| *kept != id),
        SettingChange::LinkScheme(scheme, answer) => set_link_scheme(own, scheme, answer),
        SettingChange::Role(_, Some(Answer::Simulated)) => {
            return Err("a role is allowed or not; nothing simulates it".to_owned());
        }
        SettingChange::Role(role, Some(answer)) => {
            own.roles.insert(role, answer);
        }
        SettingChange::Role(role, None) => {
            own.roles.remove(&role);
        }
        SettingChange::MachineAnswer(id) => own.answer_with.machine = id,
        SettingChange::AudioOutputAnswer(id) => own.answer_with.audio_output = id,
        SettingChange::FileDialogAnswer(dialog, typed) => {
            let paths = answer_paths(dialog, &typed)?;
            own.answer_with
                .set_file_dialog(dialog, Some(paths).filter(|paths| !paths.is_empty()));
        }
        SettingChange::AddFileToOpen(typed) => {
            let mut paths = answer_paths(FileDialog::OpenFiles, &[typed])?;
            let listed = own.answer_with.open_files.get_or_insert_default();
            paths.retain(|path| !listed.contains(path));
            listed.extend(paths);
        }
        SettingChange::RemoveFileToOpen(index) => {
            if let Some(listed) = own.answer_with.open_files.as_mut()
                && index < listed.len()
            {
                listed.remove(index);
            }
            if own
                .answer_with
                .open_files
                .as_ref()
                .is_some_and(Vec::is_empty)
            {
                own.answer_with.open_files = None;
            }
        }
        SettingChange::NetworkOn(_)
        | SettingChange::Domains(_)
        | SettingChange::AddDomain(_)
        | SettingChange::RemoveDomain(_) => {
            own.network = own_changed(&own.network, &global.network, |network| {
                change_network(network, change)
            })?;
        }
        SettingChange::AddMount { .. } | SettingChange::RemoveMount(_) => {
            own.file_system = own_changed(&own.file_system, &global.file_system, |file_system| {
                change_mounts(file_system, change)
            })?;
        }
    }
    Ok(())
}

/// The program's answer to opening links of `scheme` set to `answer`, or forgotten, to be asked
/// again.
fn set_link_scheme(own: &mut ProgramSettings, scheme: String, answer: Option<Answer>) {
    let scheme = scheme.trim().to_ascii_lowercase();
    match answer {
        Some(answer) => {
            own.link_schemes.insert(scheme, answer);
        }
        None => {
            own.link_schemes.remove(&scheme);
        }
    }
}

/// The paths `typed` as `dialog` answers them: each resolved as its person wrote it, then a whole
/// path of this computer that the dialog could choose now, one alone for a dialog that answers
/// one; else why not.
fn answer_paths(dialog: FileDialog, typed: &[String]) -> Result<Vec<PathBuf>, String> {
    if dialog != FileDialog::OpenFiles && typed.len() > 1 {
        return Err("this dialog answers one path".to_owned());
    }
    typed
        .iter()
        .map(|text| {
            let path = resolved_lexically(Path::new(text.trim()));
            match file_dialog_answer_refusal(dialog, &path) {
                Some(why) => Err(format!("{} {why}", path.display())),
                None => Ok(path),
            }
        })
        .collect()
}

/// The program's own value of a setting, `own` or else `global`, as `change` changes it.
fn own_changed<T: Clone>(
    own: &Setting<T>,
    global: &T,
    change: impl FnOnce(&mut T) -> Result<(), String>,
) -> Result<Setting<T>, String> {
    let mut value = own.or(global).clone();
    change(&mut value)?;
    Ok(Setting::Own(value))
}

fn follow_global(own: &mut ProgramSettings, which: Which) {
    match which {
        Which::Network => own.network = Setting::Global,
        Which::LocalNetwork => own.local_network = Setting::Global,
        Which::ClipboardRead => own.clipboard_read = Setting::Global,
        Which::ClipboardWrite => own.clipboard_write = Setting::Global,
        Which::Record => own.record = Setting::Global,
        Which::FileSystem => own.file_system = Setting::Global,
        Which::Answer(question) => {
            own.answers.set(question, Setting::Global);
            if question == Question::Microphone {
                own.simulated_microphone = Setting::Global;
            }
        }
        Which::SimulatedMicrophone => own.simulated_microphone = Setting::Global,
    }
}

/// Whether `question`'s device can be WaOS's own: the camera's and the microphone's.
pub(crate) fn simulatable(question: Question) -> bool {
    matches!(question, Question::Camera | Question::Microphone)
}

/// Why `answer` cannot be `question`'s: notifications have no device to simulate.
fn refuse_misplaced_simulation(question: Question, answer: Option<Answer>) -> Result<(), String> {
    if answer == Some(Answer::Simulated) && !simulatable(question) {
        return Err("only the camera and the microphone can be simulated".to_owned());
    }
    Ok(())
}

fn change_network(network: &mut NetworkGrant, change: SettingChange) -> Result<(), String> {
    match change {
        SettingChange::NetworkOn(on) => network.on = on,
        SettingChange::Domains(kind) => network.domains = domains_as(&network.domains, kind),
        SettingChange::AddDomain(text) => {
            let domain: Domain = text.parse().map_err(|error| format!("{error}"))?;
            if let Some(domains) = domain_list(&mut network.domains)
                && !domains.contains(&domain)
            {
                domains.push(domain);
            }
        }
        SettingChange::RemoveDomain(domain) => {
            if let Some(domains) = domain_list(&mut network.domains) {
                domains.retain(|listed| listed != &domain);
            }
        }
        _ => unreachable!("a change of the network"),
    }
    Ok(())
}

/// `domains` as a rule of `kind`, keeping its list where both name one.
fn domains_as(domains: &DomainRule, kind: DomainsKind) -> DomainRule {
    let listed = match domains {
        DomainRule::Any => Vec::new(),
        DomainRule::Only(listed) | DomainRule::AnyBut(listed) => listed.clone(),
    };
    match kind {
        DomainsKind::Any => DomainRule::Any,
        DomainsKind::Only => DomainRule::Only(listed),
        DomainsKind::AnyBut => DomainRule::AnyBut(listed),
    }
}

/// The kind of `domains`.
pub(crate) fn domains_kind(domains: &DomainRule) -> DomainsKind {
    match domains {
        DomainRule::Any => DomainsKind::Any,
        DomainRule::Only(_) => DomainsKind::Only,
        DomainRule::AnyBut(_) => DomainsKind::AnyBut,
    }
}

fn domain_list(domains: &mut DomainRule) -> Option<&mut Vec<Domain>> {
    match domains {
        DomainRule::Any => None,
        DomainRule::Only(listed) | DomainRule::AnyBut(listed) => Some(listed),
    }
}

fn change_mounts(file_system: &mut FileSystemGrant, change: SettingChange) -> Result<(), String> {
    match change {
        SettingChange::AddMount { host, at, writable } => {
            file_system.mounts.push(mount_of(&host, &at, writable)?);
        }
        SettingChange::RemoveMount(index) => {
            if index < file_system.mounts.len() {
                file_system.mounts.remove(index);
            }
        }
        _ => unreachable!("a change of the mounts"),
    }
    Ok(())
}

/// The mount of the folder `host` at `at`, or at its own path when `at` is empty, as a program
/// names this computer's path (`waos::machine_paths`: `C:\Users\me` is `/c/Users/me` on
/// Windows): `host` a path of this computer from its top, `at` a program's.
fn mount_of(host: &str, at: &str, writable: bool) -> Result<Mount, String> {
    let host = PathBuf::from(host.trim());
    let Some(own) = machine_paths::in_program(&host) else {
        return Err(format!(
            "the folder is a path from the top, such as {}",
            if cfg!(windows) {
                r"C:\Users\me\Documents"
            } else {
                "/Users/me/Documents"
            }
        ));
    };
    let at = match at.trim() {
        "" => own,
        at if at.starts_with('/') => PathBuf::from(at),
        _ => {
            return Err(
                "where the program sees it is a path from the top, such as /Documents".to_owned(),
            );
        }
    };
    Ok(Mount { host, at, writable })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(name: &str) -> Domain {
        name.parse().unwrap()
    }

    #[test]
    fn a_programs_setting_follows_the_global_one_until_changed_and_then_starts_from_it() {
        let mut global = GlobalSettings::default();
        global.network.domains = DomainRule::Only(vec![domain("example.com")]);
        let mut own = ProgramSettings::default();
        let page = program_page(&global, &own, &Offered::default());
        assert!(page.network.follows_global);
        assert_eq!(page.network.value, global.network);

        change_program(
            &mut own,
            &global,
            SettingChange::AddDomain("api.test.org".into()),
        )
        .unwrap();
        let page = program_page(&global, &own, &Offered::default());
        assert!(!page.network.follows_global);
        assert_eq!(
            page.network.value.domains,
            DomainRule::Only(vec![domain("example.com"), domain("api.test.org")])
        );

        change_program(
            &mut own,
            &global,
            SettingChange::FollowGlobal(Which::Network),
        )
        .unwrap();
        assert!(
            program_page(&global, &own, &Offered::default())
                .network
                .follows_global
        );
    }

    #[test]
    fn the_local_network_is_off_until_turned_on_and_a_program_follows_the_global_switch() {
        let mut global = GlobalSettings::default();
        let mut own = ProgramSettings::default();
        assert!(!global_page(&global).local_network.value, "off by default");
        change_global(&mut global, SettingChange::LocalNetwork(true)).unwrap();
        let page = program_page(&global, &own, &Offered::default());
        assert!(page.local_network.value && page.local_network.follows_global);

        change_program(&mut own, &global, SettingChange::LocalNetwork(false)).unwrap();
        let page = program_page(&global, &own, &Offered::default());
        assert!(!page.local_network.value && !page.local_network.follows_global);
        change_program(
            &mut own,
            &global,
            SettingChange::FollowGlobal(Which::LocalNetwork),
        )
        .unwrap();
        assert!(
            program_page(&global, &own, &Offered::default())
                .local_network
                .value
        );
    }

    #[test]
    fn a_rule_of_domains_keeps_its_list_between_only_and_all_but() {
        let mut global = GlobalSettings::default();
        change_global(&mut global, SettingChange::Domains(DomainsKind::Only)).unwrap();
        change_global(&mut global, SettingChange::AddDomain("example.com".into())).unwrap();
        change_global(&mut global, SettingChange::Domains(DomainsKind::AnyBut)).unwrap();
        assert_eq!(
            global.network.domains,
            DomainRule::AnyBut(vec![domain("example.com")])
        );
        change_global(
            &mut global,
            SettingChange::RemoveDomain(domain("example.com")),
        )
        .unwrap();
        assert_eq!(global.network.domains, DomainRule::AnyBut(Vec::new()));
    }

    #[test]
    fn text_that_names_no_domain_is_refused_and_changes_nothing() {
        let mut global = GlobalSettings::default();
        change_global(&mut global, SettingChange::Domains(DomainsKind::Only)).unwrap();
        let error = change_global(&mut global, SettingChange::AddDomain("no domain".into()));
        assert!(error.unwrap_err().contains("is not a domain"));
        assert_eq!(global.network.domains, DomainRule::Only(Vec::new()));
    }

    /// A folder of this computer, and where a program names it as its own path.
    #[cfg(unix)]
    const DOCUMENTS: (&str, &str) = ("/Users/me/Documents", "/Users/me/Documents");
    #[cfg(windows)]
    const DOCUMENTS: (&str, &str) = (r"C:\Users\me\Documents", "/c/Users/me/Documents");

    #[test]
    fn a_folder_is_mounted_at_its_own_path_unless_another_is_given() {
        let (documents, own_path) = DOCUMENTS;
        let mut global = GlobalSettings::default();
        let add = |at: &str| SettingChange::AddMount {
            host: documents.into(),
            at: at.into(),
            writable: false,
        };
        change_global(&mut global, add("")).unwrap();
        change_global(&mut global, add("/Documents")).unwrap();
        let mounts = &global.file_system.mounts;
        assert_eq!(mounts[0].at, std::path::PathBuf::from(own_path));
        assert_eq!(mounts[1].at, std::path::PathBuf::from("/Documents"));
        assert!(
            change_global(&mut global, add("Documents")).is_err(),
            "not from the top"
        );
        change_global(&mut global, SettingChange::RemoveMount(0)).unwrap();
        assert_eq!(global.file_system.mounts.len(), 1);
    }

    #[test]
    fn a_programs_machines_are_shown_by_their_names_and_removed_by_their_ids() {
        let global = GlobalSettings::default();
        let mut own = ProgramSettings {
            machines: vec!["this-computer".to_owned(), "gone.example.com".to_owned()],
            ..ProgramSettings::default()
        };
        let offered = Offered {
            machines: vec![MachineChoice {
                id: "this-computer".to_owned(),
                name: "This computer".to_owned(),
            }],
            outputs: Vec::new(),
        };
        let names: Vec<String> = program_page(&global, &own, &offered)
            .machines
            .into_iter()
            .map(|machine| machine.name)
            .collect();
        assert_eq!(names, ["This computer", "gone.example.com"]);
        assert!(global_page(&global).machines.is_empty());

        let remove = SettingChange::RemoveMachine("this-computer".to_owned());
        change_program(&mut own, &global, remove).unwrap();
        assert_eq!(own.machines, ["gone.example.com"]);
    }

    #[test]
    fn a_programs_pickers_answer_what_its_page_set_named_as_offered_and_by_id_when_not() {
        let global = GlobalSettings::default();
        let mut own = ProgramSettings::default();
        let offered = Offered {
            machines: vec![MachineChoice {
                id: "this-computer".to_owned(),
                name: "This computer".to_owned(),
            }],
            outputs: vec![AudioOutput {
                id: "headphones".to_owned(),
                group_id: None,
                label: "Headphones".to_owned(),
            }],
        };
        let page = program_page(&global, &own, &offered);
        assert_eq!(
            page.answer_with,
            ShownAnswerWith::default(),
            "every one asks"
        );
        for change in [
            SettingChange::MachineAnswer(Some("this-computer".to_owned())),
            SettingChange::AudioOutputAnswer(Some("unplugged".to_owned())),
        ] {
            change_program(&mut own, &global, change).unwrap();
        }
        let page = program_page(&global, &own, &offered);
        let machine = page.answer_with.machine.unwrap();
        assert_eq!(machine.name, "This computer");
        assert_eq!(page.answer_with.audio_output.unwrap().label, "unplugged");
        assert!(global_page(&global).answer_with.machine.is_none());

        change_program(&mut own, &global, SettingChange::MachineAnswer(None)).unwrap();
        assert_eq!(own.answer_with.machine, None, "asks again");
    }

    #[test]
    fn a_dialogs_answer_is_a_path_it_could_have_chosen_and_one_that_is_not_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let model = scratch.path().join("model.glb");
        std::fs::write(&model, "glTF").unwrap();
        let typed = |path: &Path| path.display().to_string();
        let global = GlobalSettings::default();
        let mut own = ProgramSettings::default();
        let open = SettingChange::FileDialogAnswer(FileDialog::OpenFile, vec![typed(&model)]);
        change_program(&mut own, &global, open).unwrap();
        assert_eq!(own.answer_with.open_file.as_deref(), Some(model.as_path()));

        let climbing = scratch.path().join("photos").join("..").join("model.glb");
        let climbed = SettingChange::FileDialogAnswer(FileDialog::OpenFile, vec![typed(&climbing)]);
        change_program(&mut own, &global, climbed).unwrap();
        assert_eq!(
            own.answer_with.open_file.as_deref(),
            Some(model.as_path()),
            "a typed `..` is resolved before it is kept"
        );

        let missing = scratch.path().join("missing.glb");
        let refused = SettingChange::FileDialogAnswer(FileDialog::OpenFile, vec![typed(&missing)]);
        let why = change_program(&mut own, &global, refused).unwrap_err();
        assert!(why.ends_with("does not exist"), "{why}");
        assert_eq!(
            own.answer_with.open_file.as_deref(),
            Some(model.as_path()),
            "unchanged"
        );
        let relative =
            SettingChange::FileDialogAnswer(FileDialog::SaveFile, vec!["out.pdf".into()]);
        assert!(change_program(&mut own, &global, relative).is_err());

        let save = scratch.path().join("out.pdf");
        let saving = SettingChange::FileDialogAnswer(FileDialog::SaveFile, vec![typed(&save)]);
        change_program(&mut own, &global, saving).unwrap();
        assert_eq!(
            own.answer_with.save_file,
            Some(save),
            "a file to save need not exist"
        );
        let ask = SettingChange::FileDialogAnswer(FileDialog::OpenFile, Vec::new());
        change_program(&mut own, &global, ask).unwrap();
        assert_eq!(own.answer_with.open_file, None);
    }

    #[test]
    fn files_to_open_are_added_once_each_and_removed_until_the_dialog_asks_again() {
        let scratch = tempfile::tempdir().unwrap();
        let (a, b) = (scratch.path().join("a.txt"), scratch.path().join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let global = GlobalSettings::default();
        let mut own = ProgramSettings::default();
        for path in [&a, &b, &a] {
            let add = SettingChange::AddFileToOpen(path.display().to_string());
            change_program(&mut own, &global, add).unwrap();
        }
        assert_eq!(own.answer_with.open_files, Some(vec![a, b.clone()]));
        change_program(&mut own, &global, SettingChange::RemoveFileToOpen(0)).unwrap();
        assert_eq!(own.answer_with.open_files, Some(vec![b]));
        change_program(&mut own, &global, SettingChange::RemoveFileToOpen(0)).unwrap();
        assert_eq!(
            own.answer_with.open_files, None,
            "asks again once none is left"
        );
    }

    #[test]
    fn a_link_scheme_answered_is_shown_and_forgotten_to_be_asked_again() {
        let global = GlobalSettings::default();
        let mut own = ProgramSettings::default();
        let allow = SettingChange::LinkScheme("ZoomMtg".to_owned(), Some(Answer::Granted));
        change_program(&mut own, &global, allow).unwrap();
        let page = program_page(&global, &own, &Offered::default());
        assert_eq!(page.link_schemes, [("zoommtg".to_owned(), Answer::Granted)]);
        let forget = SettingChange::LinkScheme("zoommtg".to_owned(), None);
        change_program(&mut own, &global, forget).unwrap();
        assert!(own.link_schemes.is_empty());
    }

    #[test]
    fn a_role_answered_is_shown_and_forgotten_to_be_asked_again_never_simulated() {
        let global = GlobalSettings::default();
        let mut own = ProgramSettings::default();
        let allow = SettingChange::Role(Role::Inspector, Some(Answer::Granted));
        change_program(&mut own, &global, allow).unwrap();
        let page = program_page(&global, &own, &Offered::default());
        assert_eq!(page.roles, [(Role::Inspector, Answer::Granted)]);
        let simulate = SettingChange::Role(Role::Inspector, Some(Answer::Simulated));
        assert!(change_program(&mut own, &global, simulate).is_err());
        let forget = SettingChange::Role(Role::Inspector, None);
        change_program(&mut own, &global, forget).unwrap();
        assert!(own.roles.is_empty());
    }

    #[test]
    fn the_camera_and_the_microphone_are_simulated_and_notifications_never() {
        let mut global = GlobalSettings::default();
        let simulate = |question| SettingChange::Answer(question, Some(Answer::Simulated));
        change_global(&mut global, simulate(Question::Camera)).unwrap();
        let why = change_global(&mut global, simulate(Question::Notifications)).unwrap_err();
        assert!(why.contains("only the camera and the microphone"), "{why}");
        assert_eq!(global.answers.notifications, None, "unchanged");

        let mut own = ProgramSettings::default();
        change_program(&mut own, &global, simulate(Question::Microphone)).unwrap();
        let silent = SettingChange::SimulatedMicrophone(SimulatedSound::Silence);
        change_program(&mut own, &global, silent).unwrap();
        let page = program_page(&global, &own, &Offered::default());
        assert_eq!(
            page.answers.camera.value,
            Some(Answer::Simulated),
            "every program's"
        );
        assert!(page.answers.camera.follows_global);
        assert_eq!(page.simulated_microphone.value, SimulatedSound::Silence);

        let silent = SettingChange::SimulatedMicrophone(SimulatedSound::Silence);
        change_program(&mut own, &global, silent).unwrap();
        let sound_follows = SettingChange::FollowGlobal(Which::SimulatedMicrophone);
        change_program(&mut own, &global, sound_follows).unwrap();
        assert_eq!(own.simulated_microphone, Setting::Global, "the sound alone");
        assert_eq!(
            own.answers.get(Question::Microphone),
            &Setting::Own(Answer::Simulated)
        );
        let silent = SettingChange::SimulatedMicrophone(SimulatedSound::Silence);
        change_program(&mut own, &global, silent).unwrap();
        let follow = SettingChange::FollowGlobal(Which::Answer(Question::Microphone));
        change_program(&mut own, &global, follow).unwrap();
        assert_eq!(
            own.simulated_microphone,
            Setting::Global,
            "follows with its answer"
        );
        let page = program_page(&global, &own, &Offered::default());
        assert_eq!(
            page.simulated_microphone.value,
            SimulatedSound::Tone,
            "the default"
        );
    }

    #[test]
    fn a_programs_answer_is_its_own_or_follows_the_global_one() {
        let mut global = GlobalSettings::default();
        change_global(
            &mut global,
            SettingChange::Answer(Question::Camera, Some(Answer::Denied)),
        )
        .unwrap();
        let mut own = ProgramSettings::default();
        let page = program_page(&global, &own, &Offered::default());
        assert_eq!(page.answers.camera.value, Some(Answer::Denied));
        assert!(page.answers.camera.follows_global);
        change_program(
            &mut own,
            &global,
            SettingChange::Answer(Question::Camera, Some(Answer::Granted)),
        )
        .unwrap();
        let page = program_page(&global, &own, &Offered::default());
        assert_eq!(page.answers.camera.value, Some(Answer::Granted));
        assert!(!page.answers.camera.follows_global);
    }
}
