//! A program's audit summary: what it did with what it was allowed today, counted as each use is
//! told, from what the use asked and what its rule resolved it to ([`Summary`]), and kept through
//! the engine's [`AuditLog`] about once a minute, as the day ends, and as the program's access goes
//! with it. A crash loses at most what came since the last keep.
//!
//! A use is counted under the program's lock, beside a check that already did far slower work:
//! a lookup, a connection, an open. A datagram, which is checked one at a time, is counted alone
//! on an atomic, and a stat not at all (`../experiments/2026-11-15-audit-cost`).
//!
//! The invariant: a day holds only that uses happened, never what they read or wrote, and grows
//! no list past its cap ([`LIST_CAPS`]): what falls beyond is counted in its overflow.

use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use waos_capabilities::{
    Answer, AskedQuestion, AuditDay, AuditLog, DeviceSession, FolderUses, LIST_CAPS,
    MachineCommand, MountKind, NamedUses, Question, Uses,
};

use super::{Asked, Cause, FileChange, Happened, Noted, Prompt, Resolution};
use crate::network::is_local;
use crate::shell::RecordingDevice;
use crate::sync::locked;

/// How often a program's day is kept while it changes.
const KEEP_EVERY: Duration = Duration::from_secs(60);

/// A day, in seconds.
const DAY: u64 = 24 * 60 * 60;

/// Where a program's days are kept, and under which name.
pub(crate) struct KeptIn {
    pub(crate) log: Arc<dyn AuditLog>,
    pub(crate) program: String,
}

/// One program's summary of today.
pub(super) struct Summary {
    kept: Option<KeptIn>,
    today: Mutex<Today>,
    /// The UDP datagrams since the day was last kept, counted apart from the lock.
    udp_datagrams: AtomicU64,
}

struct Today {
    day: AuditDay,
    /// When the day was last kept, and whether it changed since.
    kept_at: Instant,
    changed: bool,
}

impl Summary {
    pub(super) fn new(kept: Option<KeptIn>) -> Summary {
        Summary {
            kept,
            today: Mutex::new(Today {
                day: AuditDay::starting(day_of(SystemTime::now())),
                kept_at: Instant::now(),
                changed: false,
            }),
            udp_datagrams: AtomicU64::new(0),
        }
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
        self.change(|day, at| count_decided(day, asked, resolution, (allowed, at)));
    }

    /// Counts what `noted` says of the day: a datagram, and the first write of a file in one of
    /// the user's mounts, by where it is.
    pub(super) fn noted(&self, noted: &Noted<'_>) {
        match noted {
            Noted::UdpDatagram { .. } => {
                self.udp_datagrams.fetch_add(1, Ordering::Relaxed);
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

    /// Keeps the day as it is now, where it changed since it was last kept.
    pub(super) fn keep(&self) {
        let kept = {
            let mut today = locked(&self.today);
            self.fold_datagrams(&mut today);
            today.take_changed()
        };
        self.keep_days(kept.into_iter().collect());
    }

    /// Today, as counted so far.
    #[cfg(test)]
    pub(super) fn today(&self) -> AuditDay {
        let mut today = locked(&self.today);
        self.fold_datagrams(&mut today);
        today.day.clone()
    }

    /// Changes the day by `change`, given the time now; a day that ended is kept and a new one
    /// begun first, and the day is kept once a minute has passed since it last was.
    fn change(&self, change: impl FnOnce(&mut AuditDay, SystemTime)) {
        let now = SystemTime::now();
        let mut to_keep = Vec::new();
        {
            let mut today = locked(&self.today);
            if day_of(now) != today.day.start {
                self.fold_datagrams(&mut today);
                to_keep.extend(today.take_changed());
                today.day = AuditDay::starting(day_of(now));
            }
            change(&mut today.day, now);
            today.changed = true;
            if today.kept_at.elapsed() >= KEEP_EVERY {
                self.fold_datagrams(&mut today);
                to_keep.extend(today.take_changed());
            }
        }
        self.keep_days(to_keep);
    }

    fn fold_datagrams(&self, today: &mut Today) {
        let udp_datagrams = self.udp_datagrams.swap(0, Ordering::Relaxed);
        if udp_datagrams > 0 {
            today.day.udp_datagrams += udp_datagrams;
            today.changed = true;
        }
    }

    /// Keeps `days` through the log, outside the program's lock: a keep writes a file.
    fn keep_days(&self, days: Vec<AuditDay>) {
        let Some(kept) = &self.kept else {
            return;
        };
        for day in &days {
            kept.log.keep(&kept.program, day);
        }
    }
}

impl Today {
    /// The day, to be kept, where it changed since it last was.
    fn take_changed(&mut self) -> Option<AuditDay> {
        self.kept_at = Instant::now();
        std::mem::take(&mut self.changed).then(|| self.day.clone())
    }
}

impl Drop for Summary {
    /// Keeps the day as the program's access goes: the program and every use it held have ended.
    fn drop(&mut self) {
        self.keep();
    }
}

/// The midnight, UTC, that begins the day `at` is in.
fn day_of(at: SystemTime) -> SystemTime {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    UNIX_EPOCH + Duration::from_secs(seconds - seconds % DAY)
}

/// Counts `asked` in `day`, allowed or refused at `at`.
fn count_decided(
    day: &mut AuditDay,
    asked: &Asked<'_>,
    resolution: Option<&Resolution<'_>>,
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
            Some(resolution) => count_change(day, resolution, change),
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

/// Counts a change in the mount it is made in, and, in one of the user's, lists where.
fn count_change(day: &mut AuditDay, resolution: &Resolution<'_>, change: &FileChange<'_>) {
    let (made, moved_to) = match resolution {
        Resolution::Both(both) => (&both.0, Some(&both.1)),
        made => (made, None),
    };
    let Some((kind, place)) = mount_of(made) else {
        return;
    };
    let changed = kind.is_the_users().then(|| path_on_machine(made)).flatten();
    let moved_to = moved_to
        .filter(|_| kind.is_the_users())
        .and_then(path_on_machine);
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
    for path in changed.into_iter().chain(moved_to) {
        list_changed(folder, path, &mut beyond);
    }
    day.beyond_caps.changed += beyond;
}

/// Lists `path` among `folder`'s changed files, once, while the day's cap allows.
fn list_changed(folder: &mut FolderUses, path: std::path::PathBuf, beyond: &mut u32) {
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
fn path_on_machine(resolution: &Resolution<'_>) -> Option<std::path::PathBuf> {
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

    use super::*;
    use crate::files::{MountOf, OpenMode};

    /// An audit log that keeps every day it is given, in order.
    #[derive(Default)]
    struct Kept(Mutex<Vec<(String, AuditDay)>>);

    impl AuditLog for Kept {
        fn keep(&self, program: &str, day: &AuditDay) {
            locked(&self.0).push((program.to_owned(), day.clone()));
        }

        fn days(&self, _program: &str) -> Vec<AuditDay> {
            Vec::new()
        }
    }

    fn summary() -> Summary {
        Summary::new(None)
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
        let folder = fs::File::open(scratch.path()).unwrap();
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
        assert_eq!(summary.today(), AuditDay::starting(summary.today().start));
    }

    #[test]
    fn the_day_is_kept_under_the_programs_name_as_its_access_goes() {
        let log = Arc::new(Kept::default());
        let summary = Summary::new(Some(KeptIn {
            log: Arc::clone(&log) as Arc<dyn AuditLog>,
            program: "dev.waos.notes".to_owned(),
        }));
        summary.decided(&Asked::PostNotification, Some(&Resolution::Nothing), None);
        assert!(
            locked(&log.0).is_empty(),
            "kept once a minute, not at each use"
        );
        drop(summary);
        let kept = locked(&log.0);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, "dev.waos.notes");
        assert_eq!(kept[0].1.notifications.allowed, 1);
    }

    #[test]
    fn a_day_begins_at_midnight_utc() {
        let noon = UNIX_EPOCH + Duration::from_secs(20_407 * DAY + 12 * 60 * 60);
        assert_eq!(day_of(noon), UNIX_EPOCH + Duration::from_secs(20_407 * DAY));
    }
}
