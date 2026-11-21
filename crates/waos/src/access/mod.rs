//! Every guarded use a program makes, decided and told ([`ProgramAccess`]): the one place a
//! capability asks whether the program may, the one place that says it did, and the one record of
//! what the program uses now ([`active_uses`]). A capability holds its program's access rather than
//! its grants, and hands [`ProgramAccess::decide`] what the program asked ([`Asked`]) and its own
//! rule, which reads the grants, or what was compiled from them, as the mount table is, and
//! answers what the use resolved to ([`Resolved`]); the rules stay with the capabilities that
//! know them. An operation nothing decides anew, a stat or a datagram on a checked address, is
//! [`ProgramAccess::note`]d, and a use that lasts, an open file among them, is
//! [`ProgramAccess::hold`]en by whatever owns the real thing, from its start until it really ends.
//!
//! What is told is counted in the program's audit summary ([`summary`]), what it did today,
//! kept day by day through the engine's `AuditLog`, and sent as an event of `wasdk:inspect` to
//! whoever listens to its capability ([`trace`], [`events`]), one relaxed load while nobody does.
//! A test reads what was told from the access it made.
//!
//! The invariant: an answer that lets or refuses a use comes out of `decide`, its record made from
//! what the rule resolved, never from what the caller supposed before it ran, so a use is never
//! decided one way and told another. [`ProgramAccess::grants`] is for what is no use: a
//! permission's state, what a picker answers, an answer kept.

mod active_uses;
mod days;
pub(crate) mod events;
mod summary;
#[cfg(test)]
mod test_answers;
pub(crate) mod trace;

use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use futures::FutureExt;
use waos_capabilities::{Answer, Command, ProgramGrants, Question, Switch};

use crate::files::{MountKind, MountOf, OpenMode};
use crate::shell::AnswerFuture;
pub(crate) use active_uses::{
    ActiveUse, ActiveUseChange, ActiveUseChanges, ActiveUseKind, ActiveUseNow, ShownRecordings,
    TcpOrigin,
};
pub(crate) use days::Days;
pub(crate) use summary::KeptIn;
#[cfg(test)]
pub(crate) use test_answers::Answers;
pub use trace::{TraceEvent, TraceKind, TraceStream};

/// One program's access, shared by its main instance and its workers, and by the uses it holds
/// that outlive them. Every clone is the same.
#[derive(Clone)]
pub(crate) struct ProgramAccess {
    parts: Arc<AccessParts>,
    /// The instance this copy is in: the main one, 0, or a worker, from 1 as they started.
    instance: u32,
}

/// A program's access, held without keeping it: what the running program reaches it through, the
/// access holding the program in turn.
#[derive(Clone)]
pub(crate) struct WeakAccess(std::sync::Weak<AccessParts>);

struct AccessParts {
    grants: Arc<dyn ProgramGrants>,
    /// What the program uses now.
    active_uses: active_uses::ActiveUses,
    /// What it did today, kept day by day.
    summary: summary::Summary,
    /// Who listens to what it does now.
    taps: Arc<trace::TraceTaps>,
    /// What was told, for the test that made the access to read.
    #[cfg(test)]
    told: std::sync::Mutex<Vec<Told>>,
}

/// What the program asked for, in its own words, known before the rule runs. Borrowed from what
/// the capability holds.
#[derive(Debug)]
pub(crate) enum Asked<'a> {
    /// The clipboard read in `form`, by the user's paste or not.
    ClipboardRead {
        form: &'a str,
        pasted: bool,
    },
    ClipboardWrite {
        forms: &'a [&'a str],
    },
    /// The camera the program names by `id`, `simulated` when it is one of WaOS's own.
    OpenCamera {
        id: &'a str,
        simulated: bool,
    },
    /// The machine's default microphone, or WaOS's own where `simulated`.
    OpenMicrophone {
        simulated: bool,
    },
    /// A command run on the machine named `machine`.
    RunCommand {
        machine: &'a str,
        command: &'a Command,
        in_terminal: bool,
    },
    OpenLink {
        link: &'a str,
    },
    /// A notification posted.
    PostNotification,
    /// A file or folder opened, by the path the program named from one of its folders: by the
    /// program, or on its behalf by what `cause` names, read for all the program can say.
    OpenFile {
        path: &'a str,
        mode: OpenMode,
        cause: Cause,
    },
    /// A change at the path the program named.
    ChangeFile {
        path: &'a str,
        change: FileChange<'a>,
    },
    /// A folder the program holds, watched, and everything under it where `recursive`.
    WatchFolder {
        recursive: bool,
    },
    /// A name looked up, for the program itself or for what it caused.
    LookUpName {
        name: &'a str,
        cause: Cause,
    },
    /// A TCP connection made to `address`.
    ConnectTcp {
        address: SocketAddr,
    },
    /// A TCP connection from `address` accepted.
    AcceptTcp {
        address: SocketAddr,
    },
    /// A TCP socket listening at `address`.
    ListenTcp {
        address: SocketAddr,
    },
    /// An HTTP request the program sends, by an id of the program's requests.
    SendHttpRequest {
        id: u64,
        method: &'a http::Method,
        uri: &'a http::Uri,
    },
}

/// A change the program asks for at a path.
#[derive(Debug)]
pub(crate) enum FileChange<'a> {
    CreateDirectory,
    RemoveDirectory,
    /// A file or a symlink removed.
    Remove,
    /// Moved to `to`, as the program named it.
    Rename {
        to: &'a str,
    },
    /// A hard link to what the path names made at `to`.
    HardLink {
        to: &'a str,
    },
    /// A symlink holding `contents` made at the path.
    Symlink {
        contents: &'a str,
    },
    SetTimes,
}

/// Who a use is for: the program itself, or what it asked to act for it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Cause {
    Program,
    /// An HTTP request of the program's, whose host is looked up to send it.
    HttpRequest,
    /// A media player or the reading of media, which the system opens a link or a file for.
    MediaPlayer,
    /// The system's app a file is opened in, or its file manager, which shows it.
    Opener,
    /// The system's codecs, which read an image's file.
    ImageCodecs,
    /// A file dialog, which opens at a folder.
    FileDialog,
    /// Whatever the user pastes a file the program copied into.
    Clipboard,
}

/// What a rule's answer says of itself for the record, made only when the record is.
pub(crate) trait Resolved {
    fn resolution(&self) -> Resolution<'_>;
}

/// What a use resolved to.
#[derive(Debug)]
pub(crate) enum Resolution<'a> {
    Nothing,
    /// A path beneath a folder of a mount, the folder named by the system on demand: the mount's
    /// kind, and where it is on the machine.
    File {
        kind: MountKind,
        mount: &'a Path,
        folder: &'a fs::File,
        beneath: &'a Path,
    },
    /// A path of a machine the user gave the program whole.
    OnMachine(&'a Path),
    /// A lookup's answer.
    Addresses(&'a [IpAddr]),
    /// The names an address was looked up by.
    Names(&'a [String]),
    /// Two places, as a rename or a link names them: what is moved or linked, and where it goes.
    Both(Box<(Resolution<'a>, Resolution<'a>)>),
}

impl Resolved for () {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::Nothing
    }
}

impl Resolved for bool {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::Nothing
    }
}

impl Resolved for std::path::PathBuf {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::OnMachine(self)
    }
}

impl Resolved for Vec<String> {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::Names(self)
    }
}

impl Resolved for Vec<IpAddr> {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::Addresses(self)
    }
}

/// Why a rule did not let a use go on: the setting that refused it, or a failure no setting
/// decides, which the program is answered as the capability answers it.
pub(crate) trait Refused {
    /// The setting that refused, by the key `waos settings` names its row with; none for a
    /// failure.
    fn setting(&self) -> Option<&'static str>;
}

/// What refused a use: the setting, by the key `waos settings` names its row with
/// (`clipboard-read`, `camera`…, `link-scheme` for a scheme's row, `folders` for the mounts,
/// `network`, `domains` and `local-network` for the network).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) setting: &'static str,
}

impl Refusal {
    pub(crate) const fn by(setting: &'static str) -> Refusal {
        Refusal { setting }
    }
}

impl Refused for Refusal {
    fn setting(&self) -> Option<&'static str> {
        Some(self.setting)
    }
}

/// An operation nothing decides anew, told so the trace can show it.
#[derive(Debug)]
pub(crate) enum Noted<'a> {
    /// A path a metadata call named, a stat, a symlink read or a hash, and where it led.
    MetadataRead {
        path: &'a str,
        resolution: Resolution<'a>,
    },
    /// The first write through a handle opened to write: the file, and the mount it is in.
    FirstWrite {
        mount: &'a MountOf,
        file: &'a fs::File,
    },
    /// A UDP datagram sent to `address`, or received from it, refused by `refusal` where it is
    /// none.
    UdpDatagram {
        address: SocketAddr,
        sent: bool,
        refusal: Option<Refusal>,
    },
}

/// What happened that decides nothing.
#[derive(Debug)]
pub(crate) enum Happened<'a> {
    /// A question put to the user.
    QuestionAsked(Prompt),
    /// The user's answer to it; none when nobody answered.
    QuestionAnswered(Prompt, Option<Answer>),
    /// A switch's page opened in the user's settings.
    SettingsOpened(Switch),
    /// A file or folder the user picked, mounted for the program, as the machine names it.
    PickMounted { on_machine: &'a Path },
    /// A machine the user picked, by its id.
    MachinePicked { machine: &'a str },
    /// The head of the response to the HTTP request `request_id` came, with `status`, after
    /// `time_to_first_byte`.
    HttpResponded {
        request_id: u64,
        status: u16,
        time_to_first_byte: std::time::Duration,
    },
}

/// A question put to the user: a permission's, or whether the program may open links of a scheme.
#[derive(Clone, Debug)]
pub(crate) enum Prompt {
    Question(Question),
    LinkScheme(String),
    /// Whether the program may hold a role it asks for, at its start.
    Role(waos_capabilities::Role),
}

impl ProgramAccess {
    /// The access of a program with `grants`, whose recordings nothing shows, and whose days are
    /// kept nowhere.
    #[cfg(test)]
    pub(crate) fn new(grants: Arc<dyn ProgramGrants>) -> ProgramAccess {
        ProgramAccess::of(grants, None, None)
    }

    /// The access of a program with `grants`, whose recordings `shown` shows as they start and
    /// end, where something shows them, and whose days `kept` keeps, where something does.
    pub(crate) fn of(
        grants: Arc<dyn ProgramGrants>,
        shown: Option<ShownRecordings>,
        kept: Option<KeptIn>,
    ) -> ProgramAccess {
        ProgramAccess {
            instance: 0,
            parts: Arc::new(AccessParts {
                summary: summary::Summary::new(kept, Arc::clone(&grants)),
                grants,
                active_uses: active_uses::ActiveUses::new(shown),
                taps: Arc::default(),
                #[cfg(test)]
                told: std::sync::Mutex::default(),
            }),
        }
    }

    /// Decides `asked` by `rule`, which reads the grants, and tells it with what the rule
    /// resolved, or what refused it.
    pub(crate) fn decide<R: Resolved, E: Refused>(
        &self,
        asked: Asked<'_>,
        rule: impl FnOnce(&dyn ProgramGrants) -> Result<R, E>,
    ) -> Result<R, E> {
        let decided = rule(self.parts.grants.as_ref());
        self.parts.tell_decided(&asked, &decided, self.instance);
        decided
    }

    /// Tells an operation nothing decides anew: a stat, a datagram on a checked address, the
    /// first write through an open file.
    pub(crate) fn note(&self, noted: Noted<'_>) {
        self.parts.tell_noted(&noted, self.instance);
    }

    /// Registers `kind`, which the program uses from now until the [`ActiveUse`] answered is
    /// dropped by whatever owns the real thing, as it ends.
    pub(crate) fn hold(&self, kind: ActiveUseKind) -> ActiveUse {
        self.parts.tell_held(&kind, self.instance);
        active_uses::ActiveUses::start(&self.parts, kind, self.instance)
    }

    /// Registers `file`, open in a mount of `mount` as `mode` says, among what the program uses
    /// until the [`ActiveUse`] answered goes with the last copy of its handle.
    pub(crate) fn opened(
        &self,
        file: &Arc<fs::File>,
        mount: MountKind,
        mode: OpenMode,
    ) -> ActiveUse {
        self.hold(ActiveUseKind::OpenFile {
            file: Arc::downgrade(file),
            mount,
            mode,
            at: None,
        })
    }

    /// Tells what happened that decides nothing: a question asked or answered, a pick kept.
    pub(crate) fn tell(&self, happened: Happened<'_>) {
        self.parts.tell_happened(&happened, self.instance);
    }

    /// `answer`, the user's to `prompt`, put now: told asked now, and answered as it comes.
    pub(crate) fn put_question(&self, prompt: Prompt, answer: AnswerFuture) -> AnswerFuture {
        self.tell(Happened::QuestionAsked(prompt.clone()));
        let access = self.clone();
        let told =
            move |answer: &Option<Answer>| access.tell(Happened::QuestionAnswered(prompt, *answer));
        Box::pin(answer.inspect(told))
    }

    /// The same access, for the program's instance `instance`: the main one, 0, or a worker,
    /// from 1 as they started.
    pub(crate) fn for_instance(&self, instance: u32) -> ProgramAccess {
        ProgramAccess {
            parts: Arc::clone(&self.parts),
            instance,
        }
    }

    /// Tells what the program wrote to its standard output, or its errors where `errors`.
    pub(crate) fn wrote_output(&self, errors: bool, bytes: &[u8]) {
        let instance = self.instance;
        self.parts.taps.send(TraceKind::Output, || {
            events::output(errors, bytes, instance)
        });
    }

    /// The id of the program's next request.
    pub(crate) fn next_http_request_id(&self) -> u64 {
        self.parts.taps.next_http_request_id()
    }

    /// A new listener of the program's uses of `kind`, from now on.
    pub(crate) fn subscribe(&self, kind: TraceKind) -> TraceStream {
        trace::TraceTaps::subscribe(&self.parts.taps, kind)
    }

    /// The access, held without keeping it.
    pub(crate) fn downgrade(&self) -> WeakAccess {
        WeakAccess(Arc::downgrade(&self.parts))
    }

    /// The grants, for what is no use: a permission's state, what a picker answers, an answer
    /// kept.
    pub(crate) fn grants(&self) -> &dyn ProgramGrants {
        self.parts.grants.as_ref()
    }

    /// A shared handle on the grants, for async waits that outlive a borrow.
    pub(crate) fn clone_grants(&self) -> Arc<dyn ProgramGrants> {
        Arc::clone(&self.parts.grants)
    }
    /// What the program uses now but its open files, oldest first, for a test to read.
    #[cfg(test)]
    pub(crate) fn uses_but_open_files(&self) -> Vec<ActiveUseKind> {
        let uses = self.parts.active_uses.now().into_iter().map(|now| now.kind);
        uses.filter(|kind| !matches!(kind, ActiveUseKind::OpenFile { .. }))
            .collect()
    }

    /// The files the program has open, oldest first, each where the system says it is, the kind
    /// of mount it was opened in and what for, for a test to read.
    #[cfg(test)]
    pub(crate) fn open_files(&self) -> Vec<(Option<std::path::PathBuf>, MountKind, OpenMode)> {
        let uses = self.parts.active_uses.now().into_iter();
        uses.filter_map(|now| match now.kind {
            ActiveUseKind::OpenFile {
                file, mount, mode, ..
            } => {
                let at = file
                    .upgrade()
                    .and_then(|file| crate::files::path_of(&file).ok());
                Some((at, mount, mode))
            }
            _ => None,
        })
        .collect()
    }

    /// What the program uses now, and each change from then on, until it and every use it made
    /// have ended.
    pub(crate) fn subscribe_active_uses(&self) -> (Vec<ActiveUseNow>, ActiveUseChanges) {
        active_uses::ActiveUses::subscribe(&self.parts)
    }
}

impl WeakAccess {
    /// The access, while the program's is there.
    pub(crate) fn upgrade(&self) -> Option<ProgramAccess> {
        Some(ProgramAccess {
            parts: self.0.upgrade()?,
            instance: 0,
        })
    }
}

/// Where what is told goes: into the program's summary, to the listeners of its capability, and,
/// in a test, into the access's record.
impl AccessParts {
    fn tell_decided<R: Resolved, E: Refused>(
        &self,
        asked: &Asked<'_>,
        decided: &Result<R, E>,
        instance: u32,
    ) {
        let resolution = decided.as_ref().ok().map(Resolved::resolution);
        let refused = decided.as_ref().err().and_then(Refused::setting);
        self.summary.decided(asked, resolution.as_ref(), refused);
        self.taps.send(events::kind_of(asked), || {
            events::decided(asked, resolution.as_ref(), refused, instance)
        });
        #[cfg(test)]
        self.record(match (resolution, refused) {
            (Some(resolution), _) => Told::Allowed(format!("{asked:?}"), format!("{resolution:?}")),
            (None, Some(setting)) => Told::Refused(format!("{asked:?}"), setting),
            (None, None) => Told::Failed(format!("{asked:?}")),
        });
    }

    fn tell_noted(&self, noted: &Noted<'_>, instance: u32) {
        self.summary.noted(noted);
        self.taps
            .send(events::noted_kind(noted), || events::noted(noted, instance));
        #[cfg(test)]
        self.record(Told::Noted(format!("{noted:?}")));
    }

    fn tell_held(&self, kind: &ActiveUseKind, instance: u32) {
        if let Some(traced) = events::held_kind(kind) {
            self.taps.send(traced, || events::held(traced, instance));
        }
        #[cfg(test)]
        if !matches!(kind, ActiveUseKind::OpenFile { .. }) {
            self.record(Told::Held(format!("{kind:?}")));
        }
    }

    fn tell_happened(&self, happened: &Happened<'_>, instance: u32) {
        self.summary.happened(happened);
        self.taps.send(events::happened_kind(happened), || {
            events::happened(happened, instance)
        });
        #[cfg(test)]
        self.record(Told::Happened(format!("{happened:?}")));
    }

    /// Tells a held use's end; a recording's counts as a session, from `since`.
    fn tell_ended(&self, kind: &ActiveUseKind, since: (SystemTime, Instant), instance: u32) {
        let length = since.1.elapsed();
        if let ActiveUseKind::Recording(device) = kind {
            self.summary.session_ended(*device, since.0, length);
        }
        if let Some(traced) = events::ended_kind(kind) {
            self.taps
                .send(traced, || events::ended(traced, length, instance));
        }
        #[cfg(test)]
        if !matches!(kind, ActiveUseKind::OpenFile { .. }) {
            self.record(Told::Ended);
        }
    }

    #[cfg(test)]
    fn record(&self, told: Told) {
        crate::sync::locked(&self.told).push(told);
    }
}

/// One thing told, as a test reads it: what was asked, resolved, held or happened, as `Debug`
/// writes it.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Told {
    /// A use allowed, and what it resolved to.
    Allowed(String, String),
    /// A use refused, and the setting that refused it.
    Refused(String, &'static str),
    /// A use that failed, no setting deciding it.
    Failed(String),
    Noted(String),
    /// A use held from now on.
    Held(String),
    /// A held use's end.
    Ended,
    Happened(String),
}

#[cfg(test)]
impl ProgramAccess {
    /// Everything told so far, in order.
    pub(crate) fn told(&self) -> Vec<Told> {
        crate::sync::locked(&self.parts.told).clone()
    }

    /// The settings that refused each use refused so far, in order.
    pub(crate) fn refusals(&self) -> Vec<&'static str> {
        self.told()
            .into_iter()
            .filter_map(|told| match told {
                Told::Refused(_, setting) => Some(setting),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::NothingGranted;

    fn access() -> ProgramAccess {
        ProgramAccess::new(Arc::new(NothingGranted))
    }

    #[test]
    fn a_use_is_told_with_its_verdict() {
        let access = access();
        let allowed = access.decide(Asked::PostNotification, |_| Ok::<_, Refusal>(()));
        let refused = access.decide(Asked::PostNotification, |_| {
            Err::<(), _>(Refusal::by("notifications"))
        });
        assert_eq!(allowed, Ok(()));
        assert_eq!(refused, Err(Refusal::by("notifications")));
        assert_eq!(
            access.told(),
            [
                Told::Allowed("PostNotification".to_owned(), "Nothing".to_owned()),
                Told::Refused("PostNotification".to_owned(), "notifications"),
            ]
        );
    }

    #[test]
    fn a_use_is_told_with_what_its_rule_resolved_it_to() {
        let access = access();
        let looked_up = access.decide(
            Asked::LookUpName {
                name: "example.com",
                cause: Cause::Program,
            },
            |_| Ok::<_, Refusal>(vec![IpAddr::from([93, 184, 215, 14])]),
        );
        assert!(looked_up.is_ok());
        assert_eq!(
            access.told(),
            [Told::Allowed(
                "LookUpName { name: \"example.com\", cause: Program }".to_owned(),
                "Addresses([93.184.215.14])".to_owned()
            )]
        );
    }

    #[test]
    fn a_question_put_is_told_asked_then_answered() {
        let access = access();
        let prompt = Prompt::LinkScheme("zoommtg".to_owned());
        let answer =
            access.put_question(prompt, Box::pin(std::future::ready(Some(Answer::Granted))));
        assert_eq!(
            access.told(),
            [Told::Happened(
                "QuestionAsked(LinkScheme(\"zoommtg\"))".to_owned()
            )]
        );
        assert_eq!(answer.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(
            access.told().last(),
            Some(&Told::Happened(
                "QuestionAnswered(LinkScheme(\"zoommtg\"), Some(Granted))".to_owned()
            ))
        );
    }

    #[test]
    fn the_rule_reads_the_programs_grants() {
        let access = access();
        let read = access.decide(Asked::PostNotification, |grants| {
            Ok::<_, Refusal>(grants.clipboard_read())
        });
        assert_eq!(read, Ok(false));
    }

    #[test]
    fn a_use_is_sent_to_a_listener_of_its_capability_from_the_instance_that_made_it() {
        let access = access();
        let mut clipboard = access.subscribe(TraceKind::Clipboard);
        let worker = access.for_instance(3);
        let read = Asked::ClipboardRead {
            form: "text/plain",
            pasted: false,
        };
        let refused = worker.decide(read, |_| Err::<(), _>(Refusal::by("clipboard-read")));
        assert!(refused.is_err());
        let event = futures::StreamExt::next(&mut clipboard)
            .now_or_never()
            .flatten();
        let json = serde_json::to_value(event.expect("the read is sent")).unwrap();
        assert_eq!(json["instance"], 3);
        assert_eq!(json["verdict"]["refused"], "clipboard-read");
        assert_eq!(json["kind"]["read"]["form"], "text/plain");
    }
}
