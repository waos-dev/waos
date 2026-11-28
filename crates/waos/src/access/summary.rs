//! A program's audit summary: what it did with what it was allowed today, counted as each use is
//! told, from what the use asked and what its rule resolved it to ([`Summary`]), into the
//! program's day in memory, which every run of it shares and the engine's one writer keeps
//! (`days`). Counting never writes: a guarded use only counts in memory.
//!
//! A use is counted under the day's lock, beside a check that already did far slower work: a
//! lookup, a connection, an open; where a file is on the machine is asked of the system before
//! the lock is taken. A datagram, which is checked one at a time, is counted alone on an atomic,
//! and a stat not at all (measured: neither an open nor a stat shows the counting beside it).
//!
//! The invariant: a day holds only that uses happened, never what they read or wrote, and grows
//! no list past its cap ([`LIST_CAPS`]): what falls beyond is counted in its overflow.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use futures::FutureExt;

use waos_capabilities::{
    Answer, AskedQuestion, AuditDay, ChangedFuture, DeviceSession, FolderUses, LIST_CAPS,
    MachineCommand, MountKind, NamedUses, ProgramGrants, Question, Uses,
};

use super::days::{Days, ProgramDay};
use super::{Asked, Cause, FileChange, Happened, Noted, Prompt, Resolution};
use crate::network::is_local;
use crate::shell::RecordingDevice;
use crate::sync::locked;

/// Where a program's days are kept, and under which name.
pub(crate) struct KeptIn {
    pub(crate) days: Days,
    pub(crate) program: String,
}

/// One run's summary, counting into its program's day.
pub(super) struct Summary {
    day: Arc<ProgramDay>,
    kept: Option<KeptIn>,
    /// Whether the user keeps a record of what the program does.
    grants: Arc<dyn ProgramGrants>,
    recording: Mutex<Recording>,
    /// Whether it records, as last read, for a datagram, which takes no lock.
    recorded_now: AtomicBool,
}

struct Recording {
    /// Whether the user keeps a record of what the program does, as the grants said last, and
    /// their next change, asked for before they were read.
    recorded: bool,
    grants_changed: ChangedFuture,
}

impl Summary {
    /// The summary of a run with `grants`, counting into its program's day where `kept` keeps
    /// its days, which every run of it shares; into a day of its own in memory otherwise. The run
    /// is counted there as a start of the program.
    pub(super) fn new(kept: Option<KeptIn>, grants: Arc<dyn ProgramGrants>) -> Summary {
        let grants_changed = grants.changed();
        let recorded = grants.keeps_record();
        let day = match &kept {
            Some(kept) => kept.days.of(&kept.program),
            None => ProgramDay::in_memory(),
        };
        day.follow(Arc::clone(&grants));
        let summary = Summary {
            day,
            kept,
            grants,
            recording: Mutex::new(Recording {
                recorded,
                grants_changed,
            }),
            recorded_now: AtomicBool::new(recorded),
        };
        // A run is a start of its program, which its day counts as it counts its uses.
        summary.change(|day, _| day.starts += 1);
        summary
    }

    /// Counts `asked`, allowed where `resolution` says what it resolved to, refused or failed
    /// otherwise, by `refused`, the setting that refused it.
    pub(super) fn decided(
        &self,
        asked: &Asked<'_>,
        resolution: Option<&Resolution<'_>>,
        refused: Option<&'static str>,
    ) {
        if resolution.is_none() && refused.is_none() {
            // A failure no setting decides is no use.
            return;
        }
        let allowed = resolution.is_some();
        // Asked of the system before the day's lock is taken.
        let changed = match (asked, resolution) {
            (Asked::ChangeFile { .. }, Some(resolution)) => changed_paths(resolution),
            _ => Vec::new(),
        };
        self.change(|day, at| count_decided(day, asked, resolution, &changed, (allowed, at)));
    }

    /// Counts what `noted` says of the day: a datagram, and the first write of a file in one of
    /// the user's mounts, by where it is.
    pub(super) fn noted(&self, noted: &Noted<'_>) {
        match noted {
            Noted::UdpDatagram { .. } => {
                if self.recorded_now.load(Ordering::Relaxed) {
                    self.day.count_datagram();
                }
            }
            Noted::FirstWrite { mount, file } if mount.kind.is_the_users() => {
                let Ok(changed) = crate::files::path_of(file) else {
                    return;
                };
                self.change(|day, _at| match folder_of(day, mount.kind, &mount.place) {
                    Some(folder) => {
                        let mut beyond = 0;
                        list_changed(folder, changed, &mut beyond);
                        day.beyond_caps.changed += beyond;
                    }
                    None => day.beyond_caps.folders += 1,
                });
            }
            Noted::FirstWrite { .. } | Noted::MetadataRead { .. } => {}
        }
    }

    /// Counts a session of `device` that started at `start` and ran for `length`.
    pub(super) fn session_ended(
        &self,
        device: RecordingDevice,
        start: SystemTime,
        length: Duration,
    ) {
        let session = DeviceSession {
            start,
            seconds: u32::try_from(length.as_secs()).unwrap_or(u32::MAX),
            simulated: matches!(
                device,
                RecordingDevice::SimulatedCamera | RecordingDevice::SimulatedMicrophone
            ),
        };
        self.change(|day, _at| {
            let sessions = match device {
                RecordingDevice::Camera | RecordingDevice::SimulatedCamera => &mut day.camera,
                RecordingDevice::Microphone | RecordingDevice::SimulatedMicrophone => {
                    &mut day.microphone
                }
            };
            if sessions.len() < LIST_CAPS.device_sessions {
                sessions.push(session);
            } else {
                day.beyond_caps.device_sessions += 1;
            }
        });
    }

    /// Counts a question put to the user, and its answer.
    pub(super) fn happened(&self, happened: &Happened<'_>) {
        match happened {
            Happened::QuestionAsked(prompt) => self.change(|day, at| {
                if day.questions.len() < LIST_CAPS.questions {
                    day.questions.push(AskedQuestion {
                        question: prompt_key(prompt),
                        at,
                        answer: None,
                    });
                } else {
                    day.beyond_caps.questions += 1;
                }
            }),
            Happened::QuestionAnswered(prompt, answer) => self.change(|day, _at| {
                let question = prompt_key(prompt);
                let asked = day
                    .questions
                    .iter_mut()
                    .rev()
                    .find(|asked| asked.question == question && asked.answer.is_none());
                if let Some(asked) = asked {
                    asked.answer = answer.map(|answer| answer_key(answer).to_owned());
                }
            }),
            Happened::SettingsOpened(_)
            | Happened::PickMounted { .. }
            | Happened::MachinePicked { .. }
            | Happened::HttpResponded { .. } => {}
        }
    }

    /// Whether the user keeps a record of what the program does, read again where the grants
    /// changed; turned off, what was counted of today is let go, and what comes until it is
    /// turned on again is not counted.
    fn recorded(&self) -> bool {
        let mut recording = locked(&self.recording);
        if (&mut recording.grants_changed).now_or_never().is_some() {
            recording.grants_changed = self.grants.changed();
            recording.recorded = self.grants.keeps_record();
            self.recorded_now
                .store(recording.recorded, Ordering::Relaxed);
            if !recording.recorded {
                self.day.forget();
            }
        }
        recording.recorded
    }

    /// Today, as counted so far.
    #[cfg(test)]
    pub(super) fn today(&self) -> AuditDay {
        self.day.today()
    }

    /// Changes the day by `change`, given the time now, while the user keeps a record of the
    /// program.
    fn change(&self, change: impl FnOnce(&mut AuditDay, SystemTime)) {
        if self.recorded() {
            self.day.change(change);
        }
    }
}

impl Drop for Summary {
    /// Asks the writer to keep the day as the run's access goes: the run and every use it held
    /// have ended.
    fn drop(&mut self) {
        if let Some(kept) = &self.kept {
            kept.days.keep_now(&kept.program);
        }
    }
}

/// Counts `asked` in `day`, allowed or refused at `at`.
fn count_decided(
    day: &mut AuditDay,
    asked: &Asked<'_>,
    resolution: Option<&Resolution<'_>>,
    changed: &[PathBuf],
    (allowed, at): (bool, SystemTime),
) {
    match asked {
        Asked::ClipboardRead { pasted: true, .. } => {
            if allowed {
                day.pastes += 1;
            }
        }
        Asked::ClipboardRead { pasted: false, .. } => day.clipboard_reads.count(allowed, at),
        Asked::ClipboardWrite { .. } => day.clipboard_writes.count(allowed, at),
        Asked::PostNotification => day.notifications.count(allowed, at),
        Asked::RunCommand {
            machine, command, ..
        } => DayCounter { day, allowed, at }.command(MachineCommand {
            machine: (*machine).to_owned(),
            command: command.program.clone(),
        }),
        Asked::OpenLink { link } => DayCounter { day, allowed, at }.link(link),
        Asked::LookUpName { name, cause } => {
            // A request's own lookups are counted as the request.
            if !matches!(cause, Cause::HttpRequest) {
                DayCounter { day, allowed, at }.host(name);
            }
        }
        Asked::SendHttpRequest { uri, .. } => {
            DayCounter { day, allowed, at }.host(uri.host().unwrap_or_default())
        }
        Asked::ConnectTcp { address } | Asked::AcceptTcp { address } => {
            let names = match resolution {
                Some(Resolution::Names(names)) => names,
                _ => &[][..],
            };
            DayCounter { day, allowed, at }.address(address.ip(), names);
        }
        Asked::ListenTcp { address } => DayCounter { day, allowed, at }.address(address.ip(), &[]),
        Asked::OpenFile { mode, .. } => match resolution {
            None => day.folder_refusals += 1,
            Some(resolution) => count_open(day, resolution, mode.write),
        },
        Asked::ChangeFile { change, .. } => match resolution {
            None => day.folder_refusals += 1,
            Some(resolution) => count_change(day, resolution, change, changed),
        },
        Asked::OpenCamera { .. } | Asked::OpenMicrophone { .. } | Asked::WatchFolder { .. } => {}
    }
}

/// A use being counted in a day's lists.
struct DayCounter<'a> {
    day: &'a mut AuditDay,
    allowed: bool,
    at: SystemTime,
}

impl DayCounter<'_> {
    fn command(self, command: MachineCommand) {
        let beyond = &mut self.day.beyond_caps.commands;
        count_in(
            &mut self.day.commands,
            command,
            LIST_CAPS.commands,
            (self.allowed, self.at),
            beyond,
        );
    }

    /// A link, by its scheme and host, or its scheme alone where it names no host.
    fn link(self, link: &str) {
        let name = match url::Url::parse(link) {
            Ok(link) => match link.host_str() {
                Some(host) => format!("{}://{host}", link.scheme()),
                None => link.scheme().to_owned(),
            },
            Err(_) => link.split(':').next().unwrap_or_default().to_owned(),
        };
        let beyond = &mut self.day.beyond_caps.links;
        count_in(
            &mut self.day.links,
            name,
            LIST_CAPS.links,
            (self.allowed, self.at),
            beyond,
        );
    }

    /// A host a program named: an address by itself, a name among the domains.
    fn host(self, host: &str) {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        match host.parse::<IpAddr>() {
            Ok(address) => self.address(address, &[]),
            Err(_) => self.domain(host.to_ascii_lowercase()),
        }
    }

    /// An address reached: a local one by itself, any other by the names it was looked up by, or
    /// by itself where it was looked up by none.
    fn address(self, address: IpAddr, names: &[String]) {
        if is_local(address) {
            let beyond = &mut self.day.beyond_caps.local_addresses;
            let list = &mut self.day.local_addresses;
            count_in(
                list,
                address,
                LIST_CAPS.local_addresses,
                (self.allowed, self.at),
                beyond,
            );
        } else if let Some(name) = names.first() {
            self.domain(name.clone());
        } else {
            self.domain(address.to_string());
        }
    }

    fn domain(self, name: String) {
        let beyond = &mut self.day.beyond_caps.domains;
        count_in(
            &mut self.day.domains,
            name,
            LIST_CAPS.domains,
            (self.allowed, self.at),
            beyond,
        );
    }
}

/// Counts one use of `name` in `list`, allowed or refused at a time: a name not in the list
/// joins it while it holds fewer than `cap`, and counts in `beyond` otherwise.
fn count_in<T: PartialEq>(
    list: &mut Vec<NamedUses<T>>,
    name: T,
    cap: usize,
    (allowed, at): (bool, SystemTime),
    beyond: &mut u32,
) {
    if let Some(counted) = list.iter_mut().find(|counted| counted.name == name) {
        counted.uses.count(allowed, at);
    } else if list.len() < cap {
        let mut uses = Uses::default();
        uses.count(allowed, at);
        list.push(NamedUses { name, uses });
    } else {
        *beyond += 1;
    }
}

/// Counts an open in the mount it landed in.
fn count_open(day: &mut AuditDay, resolution: &Resolution<'_>, to_write: bool) {
    let Some((kind, place)) = mount_of(resolution) else {
        return;
    };
    let Some(folder) = folder_of(day, kind, place) else {
        day.beyond_caps.folders += 1;
        return;
    };
    if to_write {
        folder.opened_to_write += 1;
    } else {
        folder.opened_to_read += 1;
    }
}

/// Counts a change in the mount it is made in, and, in one of the user's, lists `changed`, where
/// the change is on the machine.
fn count_change(
    day: &mut AuditDay,
    resolution: &Resolution<'_>,
    change: &FileChange<'_>,
    changed: &[PathBuf],
) {
    let made = match resolution {
        Resolution::Both(both) => &both.0,
        made => made,
    };
    let Some((kind, place)) = mount_of(made) else {
        return;
    };
    let mut beyond = 0;
    let Some(folder) = folder_of(day, kind, place) else {
        day.beyond_caps.folders += 1;
        return;
    };
    match change {
        FileChange::CreateDirectory | FileChange::Symlink { .. } | FileChange::HardLink { .. } => {
            folder.created += 1;
        }
        FileChange::RemoveDirectory | FileChange::Remove => folder.removed += 1,
        FileChange::Rename { .. } => folder.renamed += 1,
        FileChange::SetTimes => {}
    }
    for path in changed {
        list_changed(folder, path.clone(), &mut beyond);
    }
    day.beyond_caps.changed += beyond;
}

/// Where a change `resolution` resolved to is on the machine, and where it moved to, where it is
/// made in one of the user's mounts: what the day lists of it. Asks the system, so it is called
/// before the day's lock is taken.
fn changed_paths(resolution: &Resolution<'_>) -> Vec<PathBuf> {
    let (made, moved_to) = match resolution {
        Resolution::Both(both) => (&both.0, Some(&both.1)),
        made => (made, None),
    };
    let Some((kind, _)) = mount_of(made) else {
        return Vec::new();
    };
    if !kind.is_the_users() {
        return Vec::new();
    }
    path_on_machine(made)
        .into_iter()
        .chain(moved_to.and_then(path_on_machine))
        .collect()
}

/// Lists `path` among `folder`'s changed files, once, while the day's cap allows.
fn list_changed(folder: &mut FolderUses, path: PathBuf, beyond: &mut u32) {
    if folder.changed.contains(&path) {
        return;
    }
    if folder.changed.len() < LIST_CAPS.changed {
        folder.changed.push(path);
    } else {
        *beyond += 1;
    }
}

/// The mount a resolution landed in: its kind and where it is on the machine.
fn mount_of<'a>(resolution: &'a Resolution<'_>) -> Option<(MountKind, &'a Path)> {
    match resolution {
        Resolution::File { kind, mount, .. } => Some((*kind, mount)),
        Resolution::OnMachine(_) => Some((MountKind::Machine, Path::new("/"))),
        _ => None,
    }
}

/// Where on the machine a resolution's file is, as the system names its folder now.
fn path_on_machine(resolution: &Resolution<'_>) -> Option<PathBuf> {
    match resolution {
        Resolution::File {
            folder, beneath, ..
        } => Some(crate::files::path_of(folder).ok()?.join(beneath)),
        Resolution::OnMachine(path) => Some(path.to_path_buf()),
        _ => None,
    }
}

/// The day's uses of the mount of `kind` at `place`, begun where the day has none yet; none past
/// the cap of folders.
fn folder_of<'a>(
    day: &'a mut AuditDay,
    kind: MountKind,
    place: &Path,
) -> Option<&'a mut FolderUses> {
    let at = day
        .folders
        .iter()
        .position(|folder| folder.kind == kind && folder.path == place);
    let at = match at {
        Some(at) => at,
        None if day.folders.len() < LIST_CAPS.folders => {
            day.folders.push(FolderUses {
                path: place.to_path_buf(),
                kind,
                opened_to_read: 0,
                opened_to_write: 0,
                created: 0,
                removed: 0,
                renamed: 0,
                changed: Vec::new(),
            });
            day.folders.len() - 1
        }
        None => return None,
    };
    day.folders.get_mut(at)
}

/// A switch's key, as its row is named: `network`, `clipboard-read`.
pub(super) fn switch_key(switch: waos_capabilities::Switch) -> &'static str {
    use waos_capabilities::Switch;
    match switch {
        Switch::Network => "network",
        Switch::LocalNetwork => "local-network",
        Switch::ClipboardRead => "clipboard-read",
        Switch::ClipboardWrite => "clipboard-write",
    }
}

/// A question's key, as the row a user answers it on is named: `camera`, `link-scheme zoommtg`.
pub(super) fn prompt_key(prompt: &Prompt) -> String {
    match prompt {
        Prompt::Question(Question::Notifications) => "notifications".to_owned(),
        Prompt::Question(Question::Camera) => "camera".to_owned(),
        Prompt::Question(Question::Microphone) => "microphone".to_owned(),
        Prompt::LinkScheme(scheme) => format!("link-scheme {scheme}"),
        Prompt::Role(waos_capabilities::Role::Inspector) => "role.inspector".to_owned(),
    }
}

/// An answer's key, as a row's value names it.
pub(super) fn answer_key(answer: Answer) -> &'static str {
    match answer {
        Answer::Granted => "allow",
        Answer::Denied => "deny",
        Answer::Simulated => "simulated",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::SocketAddr;

    use std::time::UNIX_EPOCH;

    use waos_capabilities::AuditLog;

    use super::*;
    use crate::access::Answers;
    use crate::access::days::day_of;
    use crate::files::{MountOf, OpenMode};

    /// A day, in seconds.
    const DAY: u64 = 24 * 60 * 60;

    /// An audit log that keeps every day it is given, in order, telling its condvar of each.
    #[derive(Default)]
    struct Kept(Mutex<Vec<(String, AuditDay)>>, std::sync::Condvar);

    impl AuditLog for Kept {
        fn keep(&self, program: &str, day: &AuditDay) {
            locked(&self.0).push((program.to_owned(), day.clone()));
            self.1.notify_all();
        }

        /// The days kept of `program`, newest first, each as it was kept last.
        fn days(&self, program: &str) -> Vec<AuditDay> {
            let mut days: Vec<AuditDay> = Vec::new();
            for (kept, day) in locked(&self.0).iter().rev() {
                if kept == program && days.iter().all(|newer| newer.start != day.start) {
                    days.push(day.clone());
                }
            }
            days
        }
    }

    fn summary() -> Summary {
        Summary::new(None, Arc::new(Answers::default()))
    }

    const READ: OpenMode = OpenMode {
        read: true,
        write: false,
    };
    const WRITE: OpenMode = OpenMode {
        read: false,
        write: true,
    };

    #[test]
    fn the_network_is_counted_per_domain_and_the_local_network_per_address() {
        let summary = summary();
        let uri: http::Uri = "https://api.example.com/v1".parse().unwrap();
        let request = Asked::SendHttpRequest {
            id: 0,
            method: &http::Method::GET,
            uri: &uri,
        };
        summary.decided(&request, Some(&Resolution::Nothing), None);
        let lookup = |cause| Asked::LookUpName {
            name: "API.example.com",
            cause,
        };
        summary.decided(
            &lookup(Cause::HttpRequest),
            Some(&Resolution::Nothing),
            None,
        );
        summary.decided(&lookup(Cause::Program), None, Some("domains"));
        let printer: SocketAddr = "192.168.1.4:631".parse().unwrap();
        summary.decided(
            &Asked::ConnectTcp { address: printer },
            None,
            Some("local-network"),
        );
        let public: SocketAddr = "93.184.215.14:443".parse().unwrap();
        let names = vec!["example.org".to_owned()];
        summary.decided(
            &Asked::ConnectTcp { address: public },
            Some(&Resolution::Names(&names)),
            None,
        );
        for _ in 0..3 {
            summary.noted(&Noted::UdpDatagram {
                address: public,
                sent: true,
                refusal: None,
            });
        }

        let day = summary.today();
        let domains: Vec<(&str, u32, u32)> = day
            .domains
            .iter()
            .map(|counted| {
                (
                    counted.name.as_str(),
                    counted.uses.allowed,
                    counted.uses.refused,
                )
            })
            .collect();
        assert_eq!(
            domains,
            [("api.example.com", 1, 1), ("example.org", 1, 0)],
            "a request's own lookup is counted as the request"
        );
        assert_eq!(day.local_addresses[0].name, printer.ip());
        assert_eq!(day.local_addresses[0].uses.refused, 1);
        assert_eq!(day.udp_datagrams, 3);
    }

    #[test]
    fn opens_and_changes_are_counted_per_mount_and_a_change_listed_in_the_users_alone() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = crate::files::open_folder(scratch.path()).unwrap();
        let mount = |kind| MountOf {
            writable: true,
            kind,
            place: Arc::from(scratch.path()),
        };
        let (granted, data) = (mount(MountKind::Granted), mount(MountKind::OwnData));
        fn at<'a>(mount: &'a MountOf, folder: &'a fs::File, beneath: &'a str) -> Resolution<'a> {
            Resolution::File {
                kind: mount.kind,
                mount: &mount.place,
                folder,
                beneath: Path::new(beneath),
            }
        }
        let summary = summary();
        let open = |mode| Asked::OpenFile {
            path: "docs/a.txt",
            mode,
            cause: Cause::Program,
        };
        summary.decided(&open(READ), Some(&at(&granted, &folder, "a.txt")), None);
        summary.decided(&open(WRITE), Some(&at(&data, &folder, "a.txt")), None);
        summary.decided(&open(READ), None, Some("folders"));
        let remove = Asked::ChangeFile {
            path: "docs/a.txt",
            change: FileChange::Remove,
        };
        summary.decided(&remove, Some(&at(&granted, &folder, "a.txt")), None);
        summary.decided(&remove, Some(&at(&data, &folder, "b.txt")), None);

        let day = summary.today();
        assert_eq!(day.folder_refusals, 1);
        let granted = day
            .folders
            .iter()
            .find(|folder| folder.kind == MountKind::Granted)
            .unwrap();
        assert_eq!((granted.opened_to_read, granted.removed), (1, 1));
        assert!(
            granted.changed[0].ends_with("a.txt"),
            "{:?}",
            granted.changed
        );
        let data = day
            .folders
            .iter()
            .find(|folder| folder.kind == MountKind::OwnData)
            .unwrap();
        assert_eq!((data.opened_to_write, data.removed), (1, 1));
        assert!(
            data.changed.is_empty(),
            "the program's own folders list nothing"
        );
    }

    #[test]
    fn a_list_grows_to_its_cap_and_what_falls_beyond_is_counted() {
        let summary = summary();
        for n in 0..LIST_CAPS.links + 5 {
            let link = format!("zoommtg://meeting-{n}.example.com/join");
            summary.decided(
                &Asked::OpenLink { link: &link },
                Some(&Resolution::Nothing),
                None,
            );
        }
        let day = summary.today();
        assert_eq!(day.links.len(), LIST_CAPS.links);
        assert_eq!(day.links[0].name, "zoommtg://meeting-0.example.com");
        assert_eq!(day.beyond_caps.links, 5);
    }

    #[test]
    fn sessions_questions_and_the_clipboard_are_counted() {
        let summary = summary();
        let start = SystemTime::now();
        summary.session_ended(
            RecordingDevice::SimulatedCamera,
            start,
            Duration::from_secs(90),
        );
        let camera = Prompt::Question(Question::Camera);
        summary.happened(&Happened::QuestionAsked(camera.clone()));
        summary.happened(&Happened::QuestionAnswered(camera, Some(Answer::Simulated)));
        let read = |pasted| Asked::ClipboardRead {
            form: "text/plain",
            pasted,
        };
        summary.decided(&read(true), Some(&Resolution::Nothing), None);
        summary.decided(&read(false), None, Some("clipboard-read"));

        let day = summary.today();
        assert_eq!(
            day.camera,
            [DeviceSession {
                start,
                seconds: 90,
                simulated: true
            }]
        );
        assert_eq!(day.questions[0].question, "camera");
        assert_eq!(day.questions[0].answer.as_deref(), Some("simulated"));
        assert_eq!((day.pastes, day.clipboard_reads.refused), (1, 1));
    }

    #[test]
    fn a_failure_no_setting_decided_is_no_use() {
        let summary = summary();
        summary.decided(&Asked::PostNotification, None, None);
        let mut the_run_alone = AuditDay::starting(summary.today().start);
        the_run_alone.starts = 1;
        assert_eq!(summary.today(), the_run_alone);
    }

    /// The days of programs whose days `log` keeps, and a run's summary of `program` among them.
    fn kept_in(days: &Days, program: &str) -> Option<KeptIn> {
        Some(KeptIn {
            days: days.clone(),
            program: program.to_owned(),
        })
    }

    fn days_kept_in(log: &Arc<Kept>) -> Days {
        Days::new(Arc::clone(log) as Arc<dyn AuditLog>).unwrap()
    }

    fn post(summary: &Summary) {
        summary.decided(&Asked::PostNotification, Some(&Resolution::Nothing), None);
    }

    #[test]
    fn nothing_is_counted_while_the_record_is_off_and_what_was_is_let_go() {
        let log = Arc::new(Kept::default());
        let days = days_kept_in(&log);
        let answers = Arc::new(Answers::default());
        let summary = Summary::new(
            kept_in(&days, "dev.waos.notes"),
            Arc::clone(&answers) as Arc<dyn ProgramGrants>,
        );
        post(&summary);
        answers.keep_record(false);
        post(&summary);
        assert_eq!(
            summary.today().notifications.allowed,
            0,
            "let go, and not counted"
        );
        days.keep_all();
        assert!(locked(&log.0).is_empty(), "nothing kept while off");
        answers.keep_record(true);
        post(&summary);
        assert_eq!(
            summary.today().notifications.allowed,
            1,
            "counted again from then on"
        );
    }

    #[test]
    fn a_use_counts_in_memory_and_the_writer_keeps_the_day_under_the_programs_name() {
        let log = Arc::new(Kept::default());
        let days = days_kept_in(&log);
        let summary = Summary::new(
            kept_in(&days, "dev.waos.notes"),
            Arc::new(Answers::default()),
        );
        post(&summary);
        assert!(locked(&log.0).is_empty(), "a use writes nothing");
        assert_eq!(
            days.days_of("dev.waos.notes")[0].notifications.allowed,
            1,
            "today is read from memory"
        );
        days.keep_all();
        let kept = locked(&log.0);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, "dev.waos.notes");
        assert_eq!(kept[0].1.notifications.allowed, 1);
    }

    /// Each run of a program is a start its day counts, the launcher's often-used apps reading
    /// them; not while its user keeps no record of it.
    #[test]
    fn each_run_is_counted_as_a_start_of_its_program() {
        let log = Arc::new(Kept::default());
        let days = days_kept_in(&log);
        for _ in 0..2 {
            drop(Summary::new(
                kept_in(&days, "dev.waos.notes"),
                Arc::new(Answers::default()),
            ));
        }
        assert_eq!(days.days_of("dev.waos.notes")[0].starts, 2);
        let unrecorded = Arc::new(Answers::default());
        unrecorded.keep_record(false);
        drop(Summary::new(
            kept_in(&days, "dev.waos.camera"),
            Arc::clone(&unrecorded) as Arc<dyn ProgramGrants>,
        ));
        let starts_of_camera: u32 = days
            .days_of("dev.waos.camera")
            .iter()
            .map(|day| day.starts)
            .sum();
        assert_eq!(starts_of_camera, 0, "no record kept");
    }

    #[test]
    fn the_writer_keeps_a_runs_day_once_it_ends() {
        let log = Arc::new(Kept::default());
        let days = days_kept_in(&log);
        let summary = Summary::new(
            kept_in(&days, "dev.waos.notes"),
            Arc::new(Answers::default()),
        );
        post(&summary);
        drop(summary);
        let (kept, waited) = log
            .1
            .wait_timeout_while(locked(&log.0), Duration::from_secs(15), |kept| {
                kept.is_empty()
            })
            .unwrap();
        assert!(!waited.timed_out(), "kept by the writer, off the run");
        assert_eq!(kept[0].1.notifications.allowed, 1);
    }

    #[test]
    fn two_runs_on_one_day_add_up_in_one_day() {
        let log = Arc::new(Kept::default());
        let days = days_kept_in(&log);
        let run = || {
            Summary::new(
                kept_in(&days, "dev.waos.notes"),
                Arc::new(Answers::default()),
            )
        };
        let first = run();
        post(&first);
        post(&first);
        let second = run();
        post(&second);
        drop(first);
        drop(second);
        days.keep_all();
        let today = &log.days("dev.waos.notes")[0];
        assert_eq!(
            today.notifications.allowed, 3,
            "the first run's two and the second's one, though they overlapped"
        );
        assert!(
            log.days("dev.waos.other").is_empty(),
            "another program's day is its own"
        );
    }

    #[test]
    fn a_program_the_engine_runs_again_counts_on_from_the_day_kept_before() {
        let log = Arc::new(Kept::default());
        let earlier = days_kept_in(&log);
        post(&Summary::new(
            kept_in(&earlier, "dev.waos.notes"),
            Arc::new(Answers::default()),
        ));
        earlier.keep_all();
        let later = days_kept_in(&log);
        post(&Summary::new(
            kept_in(&later, "dev.waos.notes"),
            Arc::new(Answers::default()),
        ));
        later.keep_all();
        assert_eq!(log.days("dev.waos.notes")[0].notifications.allowed, 2);
    }

    #[test]
    fn a_day_begins_at_midnight_utc() {
        let noon = UNIX_EPOCH + Duration::from_secs(20_407 * DAY + 12 * 60 * 60);
        assert_eq!(day_of(noon), UNIX_EPOCH + Duration::from_secs(20_407 * DAY));
    }
}
