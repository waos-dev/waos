//! What asks the user for what an app may use ([`Prompts`]), as Permissions is handed it: the
//! question at an app's first use, the question at its first link of a scheme of its own, the
//! question at its start for a role it asks for, and a switch's page in its settings, opened for
//! the user. Implemented by whatever shows WaOS's dialogs (the WaOS app, an app's own window
//! process on macOS); [`NoPrompts`] where nobody is there to ask.
//!
//! The invariant: a prompt only asks. Permissions keeps the answer, and puts each question once
//! however many wait for it, so an implementation keeps no answers and no list of its own.

use std::future::ready;

use waos_capabilities::{Answer, AnswerFuture, Question, Role, SettingsClosedFuture, Switch};
use waos_package::ProgramId;

/// An app, as its user is asked about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub id: ProgramId,
    /// What the user knows it by: its installed name, or its id where it has none.
    pub name: String,
}

/// What asks the user for what an app may use. Each future resolves to the user's answer, or to
/// none when nobody answered, as when the prompt could not be shown; dropping it puts the prompt
/// away, as when every wait for the answer is dropped.
pub trait Prompts: Send + Sync {
    /// Asks the user `question` for `app`, at its first use: Allow or Don't allow, and for the
    /// camera and the microphone, simulated.
    fn ask(&self, app: &App, question: Question) -> AnswerFuture;

    /// Asks the user whether `app` may open links of `scheme`, in lower case (`zoommtg`): any but
    /// `http`, `https` and `mailto`, which open freely.
    fn ask_to_open_links(&self, app: &App, scheme: &str) -> AnswerFuture;

    /// Asks the user whether `app` may hold `role`, which it asks for by importing the role's
    /// interfaces, saying what the role sees: at its start, before any of it runs.
    fn ask_for_role(&self, app: &App, role: Role) -> AnswerFuture;

    /// Opens `switch`'s page in `app`'s settings for the user, and resolves once the user leaves
    /// it.
    fn open_settings(&self, app: &App, switch: Switch) -> SettingsClosedFuture;
}

/// Prompts with nobody to ask, as a headless runner has: every question is refused, and no page
/// opens, so asking for a switch answers at once.
pub struct NoPrompts;

impl Prompts for NoPrompts {
    fn ask(&self, _app: &App, _question: Question) -> AnswerFuture {
        Box::pin(ready(Some(Answer::Denied)))
    }

    fn ask_to_open_links(&self, _app: &App, _scheme: &str) -> AnswerFuture {
        Box::pin(ready(Some(Answer::Denied)))
    }

    fn ask_for_role(&self, _app: &App, _role: Role) -> AnswerFuture {
        Box::pin(ready(Some(Answer::Denied)))
    }

    fn open_settings(&self, _app: &App, _switch: Switch) -> SettingsClosedFuture {
        Box::pin(ready(()))
    }
}
