//! Every guarded use a program makes, decided and told ([`ProgramAccess`]): the one place a
//! capability asks whether the program may, and the one place that says it did. A capability holds
//! its program's access rather than its grants, and hands [`ProgramAccess::decide`] the use and
//! its own rule, which reads the grants; the rules stay with the capabilities that know them.
//!
//! What is told goes nowhere yet: the audit summary and the live trace that hear it are later
//! steps of `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`. A test reads it from
//! the access it made.
//!
//! The invariant: a grant read that lets or refuses a use is made inside `decide`'s or `hold`'s
//! rule, never beside it, so no use is decided without being told. [`ProgramAccess::grants`] is
//! for what is no use: a permission's state, what a picker answers, an answer kept.

use std::path::Path;
use std::sync::Arc;

use futures::FutureExt;
use waos_capabilities::{Answer, Command, ProgramGrants, Question, Switch};

use crate::shell::AnswerFuture;

/// One program's access, shared by its main instance and its workers. Every clone is the same.
#[derive(Clone)]
pub(crate) struct ProgramAccess {
    parts: Arc<AccessParts>,
}

struct AccessParts {
    grants: Arc<dyn ProgramGrants>,
    /// What was told, for the test that made the access to read.
    #[cfg(test)]
    told: std::sync::Mutex<Vec<Told>>,
}

/// One guarded use, as the capability names it, borrowed from what the capability holds.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "what a use names is read by the audit summary and the live trace, later steps"
)]
pub(crate) enum Use<'a> {
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

/// What refused a use: the setting, by the key `cargo waos settings` names its row with
/// (`clipboard-read`, `camera`…, `link-scheme` for a scheme's row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) setting: &'static str,
}

impl Refusal {
    pub(crate) const fn by(setting: &'static str) -> Refusal {
        Refusal { setting }
    }
}

/// A use that lasts, from its decision until this is dropped, when its end is told.
pub(crate) struct Held {
    parts: Arc<AccessParts>,
}

impl ProgramAccess {
    /// The access of a program with `grants`.
    pub(crate) fn new(grants: Arc<dyn ProgramGrants>) -> ProgramAccess {
        ProgramAccess {
            parts: Arc::new(AccessParts {
                grants,
                #[cfg(test)]
                told: std::sync::Mutex::default(),
            }),
        }
    }

    /// Decides `used` by `rule`, which reads the grants, and tells it.
    pub(crate) fn decide<R>(
        &self,
        used: Use<'_>,
        rule: impl FnOnce(&dyn ProgramGrants) -> Result<R, Refusal>,
    ) -> Result<R, Refusal> {
        let decided = rule(self.parts.grants.as_ref());
        self.parts.tell_decided(&used, decided.as_ref().err());
        decided
    }

    /// As [`decide`](Self::decide), for a use that lasts (a camera, the microphone, a command):
    /// its end is told when the [`Held`] answered is dropped.
    pub(crate) fn hold<R>(
        &self,
        used: Use<'_>,
        rule: impl FnOnce(&dyn ProgramGrants) -> Result<R, Refusal>,
    ) -> Result<(R, Held), Refusal> {
        let decided = self.decide(used, rule)?;
        let held = Held {
            parts: Arc::clone(&self.parts),
        };
        Ok((decided, held))
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
}

impl Drop for Held {
    fn drop(&mut self) {
        self.parts.tell_ended();
    }
}

/// Where what is told goes: nowhere yet, the summary and the trace being later steps.
#[cfg(not(test))]
impl AccessParts {
    fn tell_decided(&self, _used: &Use<'_>, _refusal: Option<&Refusal>) {}

    fn tell_happened(&self, _happened: &Happened<'_>) {}

    fn tell_ended(&self) {}
}

/// Where what is told goes in a test: into the access's record.
#[cfg(test)]
impl AccessParts {
    fn tell_decided(&self, used: &Use<'_>, refusal: Option<&Refusal>) {
        self.record(match refusal {
            None => Told::Allowed(format!("{used:?}")),
            Some(refusal) => Told::Refused(format!("{used:?}"), refusal.setting),
        });
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

/// One thing told, as a test reads it: the use or what happened, as `Debug` writes it.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Told {
    Allowed(String),
    /// A use refused, and the setting that refused it.
    Refused(String, &'static str),
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

/// Grants for the tests of what is asked: the answers to the questions and to link schemes, as a
/// test sets them, and nothing else granted.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Answers {
    questions: std::sync::Mutex<std::collections::HashMap<Question, Answer>>,
    link_schemes: std::sync::Mutex<std::collections::HashMap<String, Answer>>,
}

#[cfg(test)]
impl Answers {
    /// The access of a program with these answers.
    pub(crate) fn access(self: &Arc<Self>) -> ProgramAccess {
        ProgramAccess::new(Arc::clone(self) as Arc<dyn ProgramGrants>)
    }
}

#[cfg(test)]
impl ProgramGrants for Answers {
    fn network(&self) -> waos_capabilities::NetworkGrant {
        crate::permissions::NothingGranted.network()
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> waos_capabilities::FileSystemGrant {
        waos_capabilities::FileSystemGrant::default()
    }

    fn answer(&self, question: Question) -> Option<Answer> {
        crate::sync::locked(&self.questions).get(&question).copied()
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        crate::sync::locked(&self.questions).insert(question, answer);
    }

    fn link_scheme_answer(&self, scheme: &str) -> Option<Answer> {
        crate::sync::locked(&self.link_schemes).get(scheme).copied()
    }

    fn set_link_scheme_answer(&self, scheme: &str, answer: Answer) {
        crate::sync::locked(&self.link_schemes).insert(scheme.to_owned(), answer);
    }

    fn changed(&self) -> waos_capabilities::ChangedFuture {
        Box::pin(std::future::pending())
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
        let allowed = access.decide(Use::Post, |_| Ok(1));
        let refused = access.decide(Use::Post, |_| Err::<(), _>(Refusal::by("notifications")));
        assert_eq!(allowed, Ok(1));
        assert_eq!(refused, Err(Refusal::by("notifications")));
        assert_eq!(
            access.told(),
            [
                Told::Allowed("Post".to_owned()),
                Told::Refused("Post".to_owned(), "notifications"),
            ]
        );
    }

    #[test]
    fn a_held_use_ends_when_its_hold_is_dropped_and_a_refused_one_holds_nothing() {
        let access = access();
        let (_, held) = access.hold(Use::Post, |_| Ok(())).unwrap();
        assert!(
            access
                .hold(Use::Post, |_| Err::<(), _>(Refusal::by("notifications")))
                .is_err()
        );
        assert!(!access.told().contains(&Told::Ended));
        drop(held);
        assert_eq!(access.told().last(), Some(&Told::Ended));
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
        let read = access.decide(Use::Post, |grants| Ok(grants.clipboard_read()));
        assert_eq!(read, Ok(false));
    }
}
