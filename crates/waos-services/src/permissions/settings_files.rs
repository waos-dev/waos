//! The settings kept as TOML files in a directory ([`SettingsFiles`]): the global ones in
//! `settings.toml` at its top, and each app's own in `programs/<id>/settings.toml`, in the app's
//! folder, beside its install. Both have the same keys; a key missing from the global file is the
//! default, and one missing from an app's file follows the global setting:
//!
//! ```toml
//! clipboard-read = true
//! clipboard-write = false
//! record = true                             # a record of what the app does, its audit summary
//!
//! [network]
//! on = true
//! domains = { only = ["example.com"] }      # or "any", or { any-but = [...] }
//!
//! [[mounts]]
//! host = "/Users/me/Documents"
//! at = "/Users/me/Documents"
//! writable = false
//!
//! [answers]
//! notifications = "granted"                 # or "denied"; missing to ask
//! camera = "simulated"                      # the camera and the microphone: or "simulated" too
//!
//! [simulated]
//! microphone = "tone"                       # or "silence": what a simulated microphone records
//!
//! [link-schemes]                            # an app's own alone
//! zoommtg = "granted"                       # or "denied"; missing to ask
//!
//! [roles]                                   # an app's own alone
//! inspector = "granted"                     # or "denied"; missing to ask at its next start
//!
//! [answer-with]                             # an app's own alone; each missing to ask
//! machine = "this-computer"                 # a machine's id
//! audio-output = "BuiltInSpeakerDevice"     # a sound output's id
//! open-file = "/Users/me/model.glb"
//! open-files = ["/Users/me/a.txt", "/Users/me/b.txt"]
//! open-folder = "/Users/me/project"
//! save-file = "/tmp/out.pdf"
//! ```
//!
//! An app's `mounts = []` is its own list, empty: no folder, whatever the global ones are. The
//! global file has no `link-schemes`: an app's user answers each scheme for it; nor `roles`,
//! which its user grants each app by name; nor `machines = ["this-computer"]`, an app's own list
//! of the machines its user picked for it; nor `answer-with`, the answers its user set for its
//! pickers and file dialogs.
//!
//! The invariant a later editor will break: these are the files and the places WaOS has always
//! kept the settings in, which people have; a change of a key or a place loses what they set.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use waos_capabilities::{
    Answer, Domain, DomainRule, FileSystemGrant, Mount, NetworkGrant, Question, Role, Setting,
    SimulatedSound,
};
use waos_package::ProgramId;

use crate::files::write_whole;
use crate::permissions::SettingsStore;
use crate::permissions::settings::{AnswerWith, AppSettings, GlobalSettings, PerQuestion};

/// The file of settings, at the directory's top for the global ones and in an app's folder for
/// its own.
const SETTINGS: &str = "settings.toml";

/// The folder of the apps' folders, in the directory.
const PROGRAMS: &str = "programs";

/// The settings as files in a directory, WaOS's own: the one the package manager keeps the apps
/// in, each app's settings in its folder.
pub struct SettingsFiles {
    directory: PathBuf,
}

impl SettingsFiles {
    /// The settings in `directory`.
    pub fn new(directory: &Path) -> SettingsFiles {
        SettingsFiles {
            directory: directory.to_path_buf(),
        }
    }

    fn global_path(&self) -> PathBuf {
        self.directory.join(SETTINGS)
    }

    fn app_folder(&self, app: &ProgramId) -> PathBuf {
        self.directory.join(PROGRAMS).join(app.as_str())
    }
}

impl SettingsStore for SettingsFiles {
    /// None where the app has no folder: neither installed nor run, nor set before either.
    fn load(&self, app: &ProgramId) -> io::Result<Option<AppSettings>> {
        let folder = self.app_folder(app);
        if !folder.is_dir() {
            return Ok(None);
        }
        read_app(&folder.join(SETTINGS)).map(Some)
    }

    /// Makes the app's folder where it has none, so a setting is kept before its install or its
    /// first run.
    fn save(&self, app: &ProgramId, settings: &AppSettings) -> io::Result<()> {
        let folder = self.app_folder(app);
        fs::create_dir_all(&folder).map_err(at(&folder))?;
        write_app(&folder.join(SETTINGS), settings)
    }

    /// Removes the app's file alone: its folder is the package manager's.
    fn forget(&self, app: &ProgramId) -> io::Result<()> {
        let path = self.app_folder(app).join(SETTINGS);
        match fs::remove_file(&path) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(at(&path)(error)),
            _ => Ok(()),
        }
    }

    fn apps(&self) -> io::Result<Vec<ProgramId>> {
        let folder = self.directory.join(PROGRAMS);
        let folders = match fs::read_dir(&folder) {
            Ok(folders) => folders,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(at(&folder)(error)),
        };
        let mut apps: Vec<ProgramId> = folders
            .flatten()
            .filter(|folder| folder.path().join(SETTINGS).is_file())
            .filter_map(|folder| folder.file_name().to_str()?.parse().ok())
            .collect();
        apps.sort();
        Ok(apps)
    }

    fn load_global(&self) -> io::Result<GlobalSettings> {
        read_global(&self.global_path())
    }

    fn save_global(&self, settings: &GlobalSettings) -> io::Result<()> {
        fs::create_dir_all(&self.directory).map_err(at(&self.directory))?;
        write_global(&self.global_path(), settings)
    }
}

/// Either file as TOML spells it: a key that is missing is not set.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SettingsFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    clipboard_read: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clipboard_write: Option<bool>,
    /// Whether a record is kept of what the app does.
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<NetworkFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_network: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mounts: Option<Vec<MountFile>>,
    #[serde(default, skip_serializing_if = "AnswersFile::is_empty")]
    answers: AnswersFile,
    #[serde(default, skip_serializing_if = "SimulatedFile::is_empty")]
    simulated: SimulatedFile,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    link_schemes: BTreeMap<String, AnswerFile>,
    /// By the role's name, `inspector`; a name this version does not know is ignored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    roles: BTreeMap<String, AnswerFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    machines: Vec<String>,
    #[serde(default, skip_serializing_if = "AnswerWithFile::is_empty")]
    answer_with: AnswerWithFile,
}

#[derive(Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct AnswerWithFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    machine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_files: Option<Vec<PathBuf>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_folder: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_file: Option<PathBuf>,
}

impl AnswerWithFile {
    fn is_empty(&self) -> bool {
        *self == AnswerWithFile::default()
    }
}

#[derive(Serialize, Deserialize)]
struct NetworkFile {
    on: bool,
    domains: DomainsFile,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DomainsFile {
    Any,
    Only(Vec<String>),
    AnyBut(Vec<String>),
}

#[derive(Serialize, Deserialize)]
struct MountFile {
    host: PathBuf,
    at: PathBuf,
    writable: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct AnswersFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    notifications: Option<AnswerFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    camera: Option<DeviceAnswerFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    microphone: Option<DeviceAnswerFile>,
}

/// What the devices of WaOS's own give, each missing for the default.
#[derive(Default, Serialize, Deserialize)]
struct SimulatedFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    microphone: Option<SoundFile>,
}

impl SimulatedFile {
    fn is_empty(&self) -> bool {
        self.microphone.is_none()
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SoundFile {
    Tone,
    Silence,
}

/// The camera's or the microphone's answer, which may be simulated too.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DeviceAnswerFile {
    Granted,
    Denied,
    Simulated,
}

impl AnswersFile {
    fn is_empty(&self) -> bool {
        self.notifications.is_none() && self.camera.is_none() && self.microphone.is_none()
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AnswerFile {
    Granted,
    Denied,
}

/// The global settings in the file at `path`: the defaults where there is no file, and for each
/// key it is missing.
fn read_global(path: &Path) -> io::Result<GlobalSettings> {
    let file = read(path)?;
    let defaults = GlobalSettings::default();
    let unreadable = unreadable_at(path);
    Ok(GlobalSettings {
        network: match file.network {
            Some(network) => network_of(network).map_err(unreadable)?,
            None => defaults.network,
        },
        local_network: file.local_network.unwrap_or(defaults.local_network),
        clipboard_read: file.clipboard_read.unwrap_or(defaults.clipboard_read),
        clipboard_write: file.clipboard_write.unwrap_or(defaults.clipboard_write),
        record: file.record.unwrap_or(defaults.record),
        file_system: file.mounts.map_or(defaults.file_system, file_system_of),
        answers: answers_of(&file.answers, |answer| answer),
        simulated_microphone: file
            .simulated
            .microphone
            .map_or(defaults.simulated_microphone, sound_of),
    })
}

/// Writes `settings`, the global ones, to the file at `path`, whole.
fn write_global(path: &Path, settings: &GlobalSettings) -> io::Result<()> {
    let file = SettingsFile {
        clipboard_read: Some(settings.clipboard_read),
        clipboard_write: Some(settings.clipboard_write),
        record: Some(settings.record),
        network: Some(network_file(&settings.network)),
        local_network: Some(settings.local_network),
        mounts: Some(mounts_file(&settings.file_system)),
        answers: answers_file(&settings.answers, |answer| *answer),
        simulated: SimulatedFile {
            microphone: Some(sound_file(settings.simulated_microphone)),
        },
        link_schemes: BTreeMap::new(),
        roles: BTreeMap::new(),
        machines: Vec::new(),
        answer_with: AnswerWithFile::default(),
    };
    write(path, &file)
}

/// An app's own settings in the file at `path`: each the global one where there is no file,
/// and for each key it is missing.
fn read_app(path: &Path) -> io::Result<AppSettings> {
    let file = read(path)?;
    let unreadable = unreadable_at(path);
    Ok(AppSettings {
        network: match file.network {
            Some(network) => Setting::Own(network_of(network).map_err(unreadable)?),
            None => Setting::Global,
        },
        local_network: own(file.local_network),
        clipboard_read: own(file.clipboard_read),
        clipboard_write: own(file.clipboard_write),
        record: own(file.record),
        file_system: own(file.mounts.map(file_system_of)),
        answers: answers_of(&file.answers, own),
        simulated_microphone: own(file.simulated.microphone.map(sound_of)),
        link_schemes: file
            .link_schemes
            .into_iter()
            .map(|(scheme, answer)| (scheme, answer_of(answer)))
            .collect(),
        roles: file
            .roles
            .into_iter()
            .filter_map(|(name, answer)| Some((role_named(&name)?, answer_of(answer))))
            .collect(),
        machines: file.machines,
        answer_with: answer_with_of(file.answer_with),
    })
}

/// Writes `settings`, an app's own, to the file at `path`, whole: a setting that follows the
/// global one is left out.
fn write_app(path: &Path, settings: &AppSettings) -> io::Result<()> {
    let file = SettingsFile {
        clipboard_read: own_value(&settings.clipboard_read).copied(),
        clipboard_write: own_value(&settings.clipboard_write).copied(),
        record: own_value(&settings.record).copied(),
        network: own_value(&settings.network).map(network_file),
        local_network: own_value(&settings.local_network).copied(),
        mounts: own_value(&settings.file_system).map(mounts_file),
        answers: answers_file(&settings.answers, |answer| own_value(answer).copied()),
        simulated: SimulatedFile {
            microphone: own_value(&settings.simulated_microphone)
                .copied()
                .map(sound_file),
        },
        link_schemes: settings
            .link_schemes
            .iter()
            .map(|(scheme, answer)| (scheme.clone(), answer_file(*answer)))
            .collect(),
        roles: settings
            .roles
            .iter()
            .map(|(role, answer)| (role_name(*role).to_owned(), answer_file(*answer)))
            .collect(),
        machines: settings.machines.clone(),
        answer_with: answer_with_file(&settings.answer_with),
    };
    write(path, &file)
}

fn read(path: &Path) -> io::Result<SettingsFile> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(SettingsFile::default()),
        Err(error) => return Err(at(path)(error)),
    };
    toml::from_str(&text).map_err(unreadable_at(path))
}

fn write(path: &Path, file: &SettingsFile) -> io::Result<()> {
    let text = toml::to_string(file).expect("the settings are TOML");
    write_whole(path, text.as_bytes()).map_err(at(path))
}

/// For `map_err`: an IO error at `path`, said with it.
fn at(path: &Path) -> impl FnOnce(io::Error) -> io::Error + '_ {
    move |error| io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// For `map_err`: the file at `path` is not as WaOS writes it, changed by hand or damaged, for the
/// reason `error` gives.
fn unreadable_at<E: fmt::Display>(path: &Path) -> impl FnOnce(E) -> io::Error + '_ {
    move |error| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("{} is not as WaOS wrote it: {error}", path.display()),
        )
    }
}

/// A role's name in the file.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::Inspector => "inspector",
    }
}

fn role_named(name: &str) -> Option<Role> {
    Role::ALL.into_iter().find(|role| role_name(*role) == name)
}

fn own<T>(value: Option<T>) -> Setting<T> {
    value.map_or(Setting::Global, Setting::Own)
}

fn own_value<T>(setting: &Setting<T>) -> Option<&T> {
    match setting {
        Setting::Global => None,
        Setting::Own(value) => Some(value),
    }
}

fn network_of(file: NetworkFile) -> Result<NetworkGrant, waos_capabilities::DomainError> {
    let domains = |names: Vec<String>| -> Result<Vec<Domain>, _> {
        names.iter().map(|name| name.parse()).collect()
    };
    Ok(NetworkGrant {
        on: file.on,
        domains: match file.domains {
            DomainsFile::Any => DomainRule::Any,
            DomainsFile::Only(names) => DomainRule::Only(domains(names)?),
            DomainsFile::AnyBut(names) => DomainRule::AnyBut(domains(names)?),
        },
    })
}

fn network_file(network: &NetworkGrant) -> NetworkFile {
    let names = |domains: &[Domain]| domains.iter().map(Domain::to_string).collect();
    NetworkFile {
        on: network.on,
        domains: match &network.domains {
            DomainRule::Any => DomainsFile::Any,
            DomainRule::Only(domains) => DomainsFile::Only(names(domains)),
            DomainRule::AnyBut(domains) => DomainsFile::AnyBut(names(domains)),
        },
    }
}

fn file_system_of(mounts: Vec<MountFile>) -> FileSystemGrant {
    FileSystemGrant {
        mounts: mounts
            .into_iter()
            .map(|mount| Mount {
                host: mount.host,
                at: mount.at,
                writable: mount.writable,
            })
            .collect(),
    }
}

fn mounts_file(file_system: &FileSystemGrant) -> Vec<MountFile> {
    file_system
        .mounts
        .iter()
        .map(|mount| MountFile {
            host: mount.host.clone(),
            at: mount.at.clone(),
            writable: mount.writable,
        })
        .collect()
}

/// Each question's answer in `file`, as `setting` makes a value of it.
fn answers_of<T>(file: &AnswersFile, setting: impl Fn(Option<Answer>) -> T) -> PerQuestion<T> {
    let device = |answer: Option<DeviceAnswerFile>| setting(answer.map(device_answer_of));
    PerQuestion {
        notifications: setting(file.notifications.map(answer_of)),
        camera: device(file.camera),
        microphone: device(file.microphone),
    }
}

/// The answers `answers` keeps, as `kept` reads each.
fn answers_file<T>(answers: &PerQuestion<T>, kept: impl Fn(&T) -> Option<Answer>) -> AnswersFile {
    let device = |question: Question| kept(answers.get(question)).map(device_answer_file);
    AnswersFile {
        notifications: kept(answers.get(Question::Notifications)).map(answer_file),
        camera: device(Question::Camera),
        microphone: device(Question::Microphone),
    }
}

fn answer_with_of(file: AnswerWithFile) -> AnswerWith {
    AnswerWith {
        machine: file.machine,
        audio_output: file.audio_output,
        open_file: file.open_file,
        open_files: file.open_files,
        open_folder: file.open_folder,
        save_file: file.save_file,
    }
}

fn answer_with_file(answer_with: &AnswerWith) -> AnswerWithFile {
    AnswerWithFile {
        machine: answer_with.machine.clone(),
        audio_output: answer_with.audio_output.clone(),
        open_file: answer_with.open_file.clone(),
        open_files: answer_with.open_files.clone(),
        open_folder: answer_with.open_folder.clone(),
        save_file: answer_with.save_file.clone(),
    }
}

fn answer_of(file: AnswerFile) -> Answer {
    match file {
        AnswerFile::Granted => Answer::Granted,
        AnswerFile::Denied => Answer::Denied,
    }
}

/// `answer`, of a question or a link scheme that has no device to simulate: the settings' changes
/// refuse to keep a simulated one, so none is.
fn answer_file(answer: Answer) -> AnswerFile {
    match answer {
        Answer::Granted => AnswerFile::Granted,
        Answer::Denied => AnswerFile::Denied,
        Answer::Simulated => unreachable!("only the camera and the microphone are simulated"),
    }
}

fn device_answer_of(file: DeviceAnswerFile) -> Answer {
    match file {
        DeviceAnswerFile::Granted => Answer::Granted,
        DeviceAnswerFile::Denied => Answer::Denied,
        DeviceAnswerFile::Simulated => Answer::Simulated,
    }
}

fn device_answer_file(answer: Answer) -> DeviceAnswerFile {
    match answer {
        Answer::Granted => DeviceAnswerFile::Granted,
        Answer::Denied => DeviceAnswerFile::Denied,
        Answer::Simulated => DeviceAnswerFile::Simulated,
    }
}

fn sound_of(file: SoundFile) -> SimulatedSound {
    match file {
        SoundFile::Tone => SimulatedSound::Tone,
        SoundFile::Silence => SimulatedSound::Silence,
    }
}

fn sound_file(sound: SimulatedSound) -> SoundFile {
    match sound {
        SimulatedSound::Tone => SoundFile::Tone,
        SimulatedSound::Silence => SoundFile::Silence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_kind_set() -> AppSettings {
        let mut settings = AppSettings {
            record: Setting::Own(false),
            network: Setting::Own(NetworkGrant {
                on: true,
                domains: DomainRule::AnyBut(vec!["tracker.example".parse().unwrap()]),
            }),
            local_network: Setting::Own(true),
            clipboard_read: Setting::Own(false),
            clipboard_write: Setting::Global,
            file_system: Setting::Own(FileSystemGrant {
                mounts: vec![Mount {
                    host: "/Users/me/Documents".into(),
                    at: "/Documents".into(),
                    writable: true,
                }],
            }),
            answers: PerQuestion::default(),
            simulated_microphone: Setting::Global,
            link_schemes: BTreeMap::from([
                ("tel".to_owned(), Answer::Denied),
                ("zoommtg".to_owned(), Answer::Granted),
            ]),
            roles: BTreeMap::from([(Role::Inspector, Answer::Granted)]),
            machines: vec!["this-computer".to_owned()],
            answer_with: AnswerWith {
                machine: Some("this-computer".to_owned()),
                audio_output: Some("BuiltInSpeakerDevice".to_owned()),
                open_file: Some("/Users/me/model.glb".into()),
                open_files: Some(vec!["/Users/me/a.txt".into(), "/Users/me/b.txt".into()]),
                open_folder: Some("/Users/me/project".into()),
                save_file: Some("/tmp/out.pdf".into()),
            },
        };
        settings
            .answers
            .set(Question::Camera, Setting::Own(Answer::Denied));
        settings
            .answers
            .set(Question::Microphone, Setting::Own(Answer::Simulated));
        settings.simulated_microphone = Setting::Own(SimulatedSound::Silence);
        settings
    }

    #[test]
    fn an_apps_settings_read_back_as_written() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("settings.toml");
        assert_eq!(read_app(&path).unwrap(), AppSettings::default());
        let settings = every_kind_set();
        write_app(&path, &settings).unwrap();
        assert_eq!(read_app(&path).unwrap(), settings);
    }

    #[test]
    fn the_global_settings_read_back_as_written_and_start_as_the_defaults() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("settings.toml");
        assert_eq!(read_global(&path).unwrap(), GlobalSettings::default());
        let mut settings = GlobalSettings::default();
        settings.network.domains = DomainRule::Only(vec!["example.com".parse().unwrap()]);
        settings.clipboard_read = false;
        settings
            .answers
            .set(Question::Notifications, Some(Answer::Granted));
        write_global(&path, &settings).unwrap();
        assert_eq!(read_global(&path).unwrap(), settings);
    }

    #[test]
    fn an_app_that_set_no_answer_writes_no_table_of_them() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("settings.toml");
        write_app(&path, &AppSettings::default()).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("answer-with"));
        fs::write(&path, "[answer-with]\nsave-file = \"/tmp/out.pdf\"\n").unwrap();
        assert_eq!(
            read_app(&path).unwrap().answer_with.save_file,
            Some("/tmp/out.pdf".into())
        );
    }

    #[test]
    fn only_the_camera_and_the_microphone_are_simulated() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("settings.toml");
        fs::write(&path, "[answers]\ncamera = \"simulated\"\n").unwrap();
        let settings = read_global(&path).unwrap();
        assert_eq!(settings.answers.camera, Some(Answer::Simulated));
        assert_eq!(
            settings.simulated_microphone,
            SimulatedSound::Tone,
            "the default"
        );
        fs::write(&path, "[answers]\nnotifications = \"simulated\"\n").unwrap();
        let error = read_global(&path).unwrap_err();
        assert!(error.kind() == ErrorKind::InvalidData, "{error}");
    }

    #[test]
    fn an_app_with_an_empty_list_of_mounts_has_its_own() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("settings.toml");
        fs::write(&path, "mounts = []\n").unwrap();
        assert_eq!(
            read_app(&path).unwrap().file_system,
            Setting::Own(FileSystemGrant::default())
        );
    }

    #[test]
    fn a_file_kept_with_the_processes_switch_of_before_machines_reads_without_it() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("settings.toml");
        fs::write(
            &path,
            "local-network = true\n\n[processes]\non = true\ncommands = \"any\"\n",
        )
        .unwrap();
        let settings = read_app(&path).unwrap();
        assert_eq!(settings.local_network, Setting::Own(true));
        write_app(&path, &settings).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("processes"));
    }

    #[test]
    fn a_file_that_names_no_domain_is_unreadable() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("settings.toml");
        fs::write(
            &path,
            "[network]\non = true\ndomains = { only = [\"not a domain\"] }\n",
        )
        .unwrap();
        let error = read_app(&path).unwrap_err();
        assert!(error.kind() == ErrorKind::InvalidData, "{error}");
    }
}
