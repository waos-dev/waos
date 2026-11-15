//! Every guarded use a program makes, decided and told ([`ProgramAccess`]): the one place a
//! capability asks whether the program may, the one place that says it did, and the one record of
//! what the program holds now ([`holdings`]). A capability holds its program's access rather than
//! its grants, and hands [`ProgramAccess::decide`] what the program asked ([`Asked`]) and its own
//! rule, which reads the grants, or what was compiled from them, as the mount table is, and
//! answers what the use resolved to ([`Resolved`]); the rules stay with the capabilities that
//! know them. An operation nothing decides anew, a stat or a datagram on a checked address, is
//! [`ProgramAccess::note`]d, and a use that lasts is [`ProgramAccess::hold`]en by whatever owns
//! the real thing, from its start until it really ends.
//!
//! What is told goes nowhere yet: the audit summary and the live trace that hear it are later
//! steps of `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md` and
//! `docs/proposals/2026-11-15-access-layer-design.md`. A test reads it from the access it made.
//!
//! The invariant: an answer that lets or refuses a use comes out of `decide`, its record made from
//! what the rule resolved, never from what the caller supposed before it ran, so a use is never
//! decided one way and told another. [`ProgramAccess::grants`] is for what is no use: a
//! permission's state, what a picker answers, an answer kept.

mod holdings;
#[cfg(test)]
mod test_answers;

use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use futures::FutureExt;
use waos_capabilities::{Answer, Command, ProgramGrants, Question, Switch};

use crate::files::{MountKind, OpenMode};
use crate::shell::AnswerFuture;
pub(crate) use holdings::{Held, Holding, Holdings, ShownRecordings};
#[cfg(test)]
pub(crate) use test_answers::Answers;

/// One program's access, shared by its main instance and its workers, and by the uses it holds
/// that outlive them. Every clone is the same.
#[derive(Clone)]
pub(crate) struct ProgramAccess {
    parts: Arc<AccessParts>,
}

struct AccessParts {
    grants: Arc<dyn ProgramGrants>,
    /// What the program holds now.
    holdings: holdings::Registry,
    /// What was told, for the test that made the access to read.
    #[cfg(test)]
    told: std::sync::Mutex<Vec<Told>>,
}

/// What the program asked for, in its own words, known before the rule runs. Borrowed from what
/// the capability holds.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "what a use names is read by the audit summary and the live trace, later steps"
)]
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
    Run {
        machine: &'a str,
        command: &'a Command,
        in_terminal: bool,
    },
    OpenLink {
        link: &'a str,
    },
    /// A notification posted.
    Post,
    /// A file or folder opened, by the path the program named from one of its folders.
    Open {
        path: &'a str,
        mode: OpenMode,
    },
    /// A change at the path the program named.
    Change {
        path: &'a str,
        change: FileChange<'a>,
    },
    /// A folder the program holds, watched, and everything under it where `recursive`.
    Watch {
        recursive: bool,
    },
    /// A name looked up, for the program itself or for what it caused.
    Lookup {
        name: &'a str,
        cause: Cause,
    },
    /// A TCP connection made to `address`.
    Connect {
        address: SocketAddr,
    },
    /// A TCP connection from `address` accepted.
    Accept {
        address: SocketAddr,
    },
    /// A TCP socket listening at `address`.
    Listen {
        address: SocketAddr,
    },
    /// An HTTP request the program sends.
    Request {
        method: &'a http::Method,
        uri: &'a http::Uri,
    },
}

/// A change the program asks for at a path.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "what a change names is read by the audit summary and the live trace, later steps"
)]
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
    Link {
        to: &'a str,
    },
    /// A symlink holding `contents` made at the path.
    Symlink {
        contents: &'a str,
    },
    SetTimes,
}

/// Who a lookup is for: the program's own, or one made for what it asked another to do.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Cause {
    Program,
    /// A request of the program's, whose host is looked up to send it.
    Request,
    /// A link a player or the reading of media is opened on, which the system fetches.
    Player,
}

/// What a rule's answer says of itself for the record, made only when the record is.
pub(crate) trait Resolved {
    fn resolution(&self) -> Resolution<'_>;
}

/// What a use resolved to.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "what a use resolved to is read by the audit summary and the live trace, later steps"
)]
pub(crate) enum Resolution<'a> {
    Nothing,
    /// A path beneath a folder of a mount, the folder named by the system on demand.
    File {
        kind: MountKind,
        folder: &'a fs::File,
        beneath: &'a Path,
    },
    /// A path of a machine the user gave the program whole.
    OnMachine(&'a Path),
    /// A lookup's answer.
    Addresses(&'a [IpAddr]),
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

impl Resolved for Vec<IpAddr> {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::Addresses(self)
    }
}

/// Why a rule did not let a use go on: the setting that refused it, or a failure no setting
/// decides, which the program is answered as the capability answers it.
pub(crate) trait Refused {
    /// The setting that refused, by the key `cargo waos settings` names its row with; none for a
    /// failure.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the setting is read by the audit summary and the live trace, later steps"
        )
    )]
    fn setting(&self) -> Option<&'static str>;
}

/// What refused a use: the setting, by the key `cargo waos settings` names its row with
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
#[expect(
    dead_code,
    reason = "what is noted is read by the live trace, a later step"
)]
pub(crate) enum Noted<'a> {
    /// A path a metadata call named, a stat, a symlink read or a hash, and where it led.
    Named {
        path: &'a str,
        resolution: Resolution<'a>,
    },
    /// The first write through a handle opened to write, in a mount of `kind`.
    FirstWrite { kind: MountKind },
    /// A datagram sent to `address`, or received from it, refused by `refusal` where it is none.
    Datagram {
        address: SocketAddr,
        sent: bool,
        refusal: Option<Refusal>,
    },
}

/// What happened that decides nothing.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "what happened is read by the audit summary and the live trace, later steps"
)]
pub(crate) enum Happened<'a> {
    /// A question put to the user.
    Asked(Prompt),
    /// The user's answer to it; none when nobody answered.
    Answered(Prompt, Option<Answer>),
    /// A switch's page opened in the user's settings.
    SettingsOpened(Switch),
    /// A file or folder the user picked, mounted for the program, as the machine names it.
    Mounted { on_machine: &'a Path },
    /// A machine the user picked, by its id.
    MachinePicked { machine: &'a str },
}

/// A question put to the user: a permission's, or whether the program may open links of a scheme.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "what was asked is read by the audit summary and the live trace, later steps"
)]
pub(crate) enum Prompt {
    Question(Question),
    LinkScheme(String),
}

impl ProgramAccess {
    /// The access of a program with `grants`, whose recordings nothing shows.
    pub(crate) fn new(grants: Arc<dyn ProgramGrants>) -> ProgramAccess {
        ProgramAccess::with_holdings(grants, holdings::Registry::new(None))
    }

    /// The access of a program with `grants`, whose recordings `shown` shows as they start and
    /// end.
    pub(crate) fn showing_recordings(
        grants: Arc<dyn ProgramGrants>,
        shown: ShownRecordings,
    ) -> ProgramAccess {
        ProgramAccess::with_holdings(grants, holdings::Registry::new(Some(shown)))
    }

    fn with_holdings(
        grants: Arc<dyn ProgramGrants>,
        holdings: holdings::Registry,
    ) -> ProgramAccess {
        ProgramAccess {
            parts: Arc::new(AccessParts {
                grants,
                holdings,
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
        self.parts.tell_decided(&asked, &decided);
        decided
    }

    /// Tells an operation nothing decides anew: a stat, a datagram on a checked address, the
    /// first write through an open file.
    pub(crate) fn note(&self, noted: Noted<'_>) {
        self.parts.tell_noted(&noted);
    }

    /// Registers `holding`, which the program holds from now until the [`Held`] answered is
    /// dropped by whatever owns the real thing, as it ends.
    pub(crate) fn hold(&self, holding: Holding) -> Held {
        self.parts.tell_held(&holding);
        holdings::Registry::hold(&self.parts, holding)
    }

    /// Registers `file`, open in a mount of `kind` as `mode` says, among the program's open files
    /// while anything holds it.
    pub(crate) fn opened(&self, file: &Arc<fs::File>, kind: MountKind, mode: OpenMode) {
        self.parts.holdings.opened(file, kind, mode);
    }

    /// Tells what happened that decides nothing: a question asked or answered, a pick kept.
    pub(crate) fn tell(&self, happened: Happened<'_>) {
        self.parts.tell_happened(&happened);
    }

    /// `answer`, the user's to `prompt`, put now: told asked now, and answered as it comes.
    pub(crate) fn put(&self, prompt: Prompt, answer: AnswerFuture) -> AnswerFuture {
        self.tell(Happened::Asked(prompt.clone()));
        let access = self.clone();
        let told = move |answer: &Option<Answer>| access.tell(Happened::Answered(prompt, *answer));
        Box::pin(answer.inspect(told))
    }

    /// The grants, for what is no use: a permission's state, what a picker answers, an answer
    /// kept.
    pub(crate) fn grants(&self) -> &dyn ProgramGrants {
        self.parts.grants.as_ref()
    }
    /// What the program holds now: each use held, since when, and each file open, where the
    /// system says it is. Read on demand; nothing is made for it while nobody reads it.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the live state is read by the inspection protocol, a later step"
        )
    )]
    pub(crate) fn holdings(&self) -> Holdings {
        self.parts.holdings.now()
    }
}

/// Where what is told goes: nowhere yet, the summary and the trace being later steps.
#[cfg(not(test))]
impl AccessParts {
    fn tell_decided<R: Resolved, E: Refused>(&self, _asked: &Asked<'_>, _decided: &Result<R, E>) {}

    fn tell_noted(&self, _noted: &Noted<'_>) {}

    fn tell_held(&self, _holding: &Holding) {}

    fn tell_happened(&self, _happened: &Happened<'_>) {}

    fn tell_ended(&self) {}
}

/// Where what is told goes in a test: into the access's record.
#[cfg(test)]
impl AccessParts {
    fn tell_decided<R: Resolved, E: Refused>(&self, asked: &Asked<'_>, decided: &Result<R, E>) {
        let asked = format!("{asked:?}");
        self.record(match decided {
            Ok(resolved) => Told::Allowed(asked, format!("{:?}", resolved.resolution())),
            Err(refused) => match refused.setting() {
                Some(setting) => Told::Refused(asked, setting),
                None => Told::Failed(asked),
            },
        });
    }

    fn tell_noted(&self, noted: &Noted<'_>) {
        self.record(Told::Noted(format!("{noted:?}")));
    }

    fn tell_held(&self, holding: &Holding) {
        self.record(Told::Held(format!("{holding:?}")));
    }

    fn tell_happened(&self, happened: &Happened<'_>) {
        self.record(Told::Happened(format!("{happened:?}")));
    }

    fn tell_ended(&self) {
        self.record(Told::Ended);
    }

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
        let allowed = access.decide(Asked::Post, |_| Ok::<_, Refusal>(()));
        let refused = access.decide(Asked::Post, |_| Err::<(), _>(Refusal::by("notifications")));
        assert_eq!(allowed, Ok(()));
        assert_eq!(refused, Err(Refusal::by("notifications")));
        assert_eq!(
            access.told(),
            [
                Told::Allowed("Post".to_owned(), "Nothing".to_owned()),
                Told::Refused("Post".to_owned(), "notifications"),
            ]
        );
    }

    #[test]
    fn a_use_is_told_with_what_its_rule_resolved_it_to() {
        let access = access();
        let looked_up = access.decide(
            Asked::Lookup {
                name: "example.com",
                cause: Cause::Program,
            },
            |_| Ok::<_, Refusal>(vec![IpAddr::from([93, 184, 215, 14])]),
        );
        assert!(looked_up.is_ok());
        assert_eq!(
            access.told(),
            [Told::Allowed(
                "Lookup { name: \"example.com\", cause: Program }".to_owned(),
                "Addresses([93.184.215.14])".to_owned()
            )]
        );
    }

    #[test]
    fn a_question_put_is_told_asked_then_answered() {
        let access = access();
        let prompt = Prompt::LinkScheme("zoommtg".to_owned());
        let answer = access.put(prompt, Box::pin(std::future::ready(Some(Answer::Granted))));
        assert_eq!(
            access.told(),
            [Told::Happened("Asked(LinkScheme(\"zoommtg\"))".to_owned())]
        );
        assert_eq!(answer.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(
            access.told().last(),
            Some(&Told::Happened(
                "Answered(LinkScheme(\"zoommtg\"), Some(Granted))".to_owned()
            ))
        );
    }

    #[test]
    fn the_rule_reads_the_programs_grants() {
        let access = access();
        let read = access.decide(Asked::Post, |grants| {
            Ok::<_, Refusal>(grants.clipboard_read())
        });
        assert_eq!(read, Ok(false));
    }
}
