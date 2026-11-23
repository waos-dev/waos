//! Where the programs' prompts go: each to the main thread, a question to be shown to the user
//! (`state::questions`), a permission's, a link scheme's or a role's, which the engine's prompts ask, the
//! output a program's sound plays on, which the shell's picker of outputs asks, or a machine for a
//! program, which the shell's picker of machines asks; and a switch's
//! page to be opened in the library's settings (`state::library`). Each is noted as asked of the
//! program's user until it is answered (`asking_now`), for an attach to the program to say. The engine asks from the
//! program's thread and is answered later, through the future it was given: a question with the
//! user's reply, or none when nobody replied, because the main thread stopped listening or dropped
//! it; a switch's page once the user left it, or at once when nobody can open it.
//!
//! The invariant: every prompt taken reaches the main thread or is answered at once, so no
//! program waits for an answer that cannot come.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::{
    Answer, AnswerFuture, AudioOutput, AudioOutputPickedFuture, MachineChoice, MachinePickedFuture,
    Prompts, Question, Role, RunningProgram, SettingsClosedFuture, ShellAudioOutputPicker,
    ShellMachinePicker, Switch,
};
use waos_dev_protocol::Asked as AskedNow;

use super::asking_now::AskingNow;

/// One program's question, on its way to the main thread.
pub(crate) struct Asked {
    pub(crate) program: RunningProgram,
    pub(crate) asking: Asking,
    /// Fires when the engine drops its wait on this question before an answer returns.
    pub(crate) cancelled: oneshot::Receiver<()>,
}

/// What a program asks its user, with where the reply goes.
pub(crate) enum Asking {
    /// A question it puts once, answered Allow or Don't allow, the answer kept in its settings.
    Question {
        question: ProgramQuestion,
        answer: oneshot::Sender<Option<Answer>>,
    },
    /// Which of `outputs` its sound plays on: the id of the one picked.
    Output {
        outputs: Vec<AudioOutput>,
        picked: oneshot::Sender<Option<String>>,
    },
    /// Which of `machines` it acts on as the user: the id of the one picked, its grant.
    Machine {
        machines: Vec<MachineChoice>,
        picked: oneshot::Sender<Option<String>>,
    },
}

/// What a program asks its user, once, the answer kept in its settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProgramQuestion {
    /// A question of its permissions, at its first use.
    Permission(Question),
    /// Whether it may open links of a scheme, any but `http`, `https` and `mailto`, which open
    /// freely: the scheme, in lower case.
    LinkScheme(String),
    /// Whether it may hold a role it asks for by importing the role's interfaces, at its start.
    Role(Role),
}

impl fmt::Debug for Asking {
    /// What is asked, without where the reply goes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Asking::Question { question, .. } => f.debug_tuple("Question").field(question).finish(),
            Asking::Output { outputs, .. } => {
                let ids = outputs.iter().map(|output| &output.id);
                f.debug_tuple("Output")
                    .field(&ids.collect::<Vec<_>>())
                    .finish()
            }
            Asking::Machine { machines, .. } => {
                let ids = machines.iter().map(|machine| &machine.id);
                f.debug_tuple("Machine")
                    .field(&ids.collect::<Vec<_>>())
                    .finish()
            }
        }
    }
}

/// The user's reply to what a program asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    /// Allow or Don't allow, to a question.
    Answer(Answer),
    /// The id of the output picked, to the picker of outputs.
    Output(String),
    /// The id of the machine picked, to the picker of machines.
    Machine(String),
}

impl Asking {
    /// Sends `reply` to the program, or none when the user gave none, or a reply of the other
    /// kind. A program gone meanwhile hears nothing.
    pub(crate) fn reply(self, reply: Option<Reply>) {
        match (self, reply) {
            (Asking::Question { answer, .. }, Some(Reply::Answer(given))) => {
                let _ = answer.send(Some(given));
            }
            (Asking::Output { picked, .. }, Some(Reply::Output(id)))
            | (Asking::Machine { picked, .. }, Some(Reply::Machine(id))) => {
                let _ = picked.send(Some(id));
            }
            (Asking::Question { answer, .. }, _) => {
                let _ = answer.send(None);
            }
            (Asking::Output { picked, .. } | Asking::Machine { picked, .. }, _) => {
                let _ = picked.send(None);
            }
        }
    }
}

/// One program's request to open a switch's page in its settings, on its way to the main thread.
pub(crate) struct SettingsOpened {
    pub(crate) program: RunningProgram,
    pub(crate) switch: Switch,
    /// What tells the program the user left the page.
    pub(crate) left: oneshot::Sender<()>,
}

/// Where the prompts arrive on the main thread.
pub(crate) struct PromptRequests {
    pub(crate) questions: UnboundedReceiver<Asked>,
    pub(crate) settings: UnboundedReceiver<SettingsOpened>,
}

/// The app's prompts, as the engine reaches them: every one goes to the main thread, noted in
/// `asking` until it is answered.
pub(crate) struct PromptsOnRequest {
    questions: UnboundedSender<Asked>,
    settings: UnboundedSender<SettingsOpened>,
    asking: Arc<AskingNow>,
}

impl PromptsOnRequest {
    /// The prompts, each noted in `asking` while it waits, and where they arrive on the main
    /// thread.
    pub(crate) fn new(asking: Arc<AskingNow>) -> (PromptsOnRequest, PromptRequests) {
        let (questions, asked) = mpsc::unbounded();
        let (settings, opened) = mpsc::unbounded();
        let requests = PromptRequests {
            questions: asked,
            settings: opened,
        };
        (
            PromptsOnRequest {
                questions,
                settings,
                asking,
            },
            requests,
        )
    }

    /// The picker of outputs, whose questions arrive with these prompts' questions.
    pub(crate) fn output_picker(&self) -> OutputPickerOnRequest {
        OutputPickerOnRequest {
            questions: self.questions.clone(),
            asking: Arc::clone(&self.asking),
        }
    }

    /// The picker of machines, whose questions arrive with these prompts' questions.
    pub(crate) fn machine_picker(&self) -> MachinePickerOnRequest {
        MachinePickerOnRequest {
            questions: self.questions.clone(),
            asking: Arc::clone(&self.asking),
        }
    }
}

impl PromptsOnRequest {
    /// Sends `question` of `program`'s to the main thread, and resolves to the user's answer.
    fn put(&self, program: &RunningProgram, question: ProgramQuestion) -> AnswerFuture {
        let (answer, answered) = oneshot::channel();
        let noted = question.asked_now();
        let asking = Asking::Question { question, answer };
        let wait = send_and_wait(&self.questions, program, asking, answered);
        Box::pin(self.asking.while_asked(program, noted, wait))
    }
}

impl Prompts for PromptsOnRequest {
    fn ask(&self, program: &RunningProgram, question: Question) -> AnswerFuture {
        self.put(program, ProgramQuestion::Permission(question))
    }

    fn ask_to_open_links(&self, program: &RunningProgram, scheme: &str) -> AnswerFuture {
        self.put(program, ProgramQuestion::LinkScheme(scheme.to_owned()))
    }

    fn ask_for_role(&self, program: &RunningProgram, role: Role) -> AnswerFuture {
        self.put(program, ProgramQuestion::Role(role))
    }

    fn open_settings(&self, program: &RunningProgram, switch: Switch) -> SettingsClosedFuture {
        let (left, leaving) = oneshot::channel();
        let opened = SettingsOpened {
            program: program.clone(),
            switch,
            left,
        };
        if self.settings.unbounded_send(opened).is_err() {
            tracing::info!(
                program = program.name(),
                ?switch,
                "WaOS is quitting: no settings"
            );
        }
        // A page dropped, or never opened, counts as left.
        let left = leaving.map(|_left| ());
        let noted = AskedNow::Switch(protocol_switch(switch));
        Box::pin(self.asking.while_asked(program, noted, left))
    }
}

/// The app's picker of outputs, as the engine reaches it: each picker is a question, shown as the
/// prompts' questions are.
pub(crate) struct OutputPickerOnRequest {
    questions: UnboundedSender<Asked>,
    asking: Arc<AskingNow>,
}

impl ShellAudioOutputPicker for OutputPickerOnRequest {
    fn pick(&self, program: &RunningProgram, outputs: Vec<AudioOutput>) -> AudioOutputPickedFuture {
        let (picked, picking) = oneshot::channel();
        let asking = Asking::Output { outputs, picked };
        let wait = send_and_wait(&self.questions, program, asking, picking);
        Box::pin(
            self.asking
                .while_asked(program, AskedNow::AudioOutput, wait),
        )
    }
}

/// The app's picker of machines, as the engine reaches it: each pick is a question, shown as the
/// prompts' questions are.
pub(crate) struct MachinePickerOnRequest {
    questions: UnboundedSender<Asked>,
    asking: Arc<AskingNow>,
}

impl ShellMachinePicker for MachinePickerOnRequest {
    fn pick(&self, program: &RunningProgram, machines: Vec<MachineChoice>) -> MachinePickedFuture {
        let (picked, picking) = oneshot::channel();
        let asking = Asking::Machine { machines, picked };
        let wait = send_and_wait(&self.questions, program, asking, picking);
        Box::pin(self.asking.while_asked(program, AskedNow::Machine, wait))
    }
}

impl ProgramQuestion {
    /// The question, as an attach to the program is told it is asked.
    fn asked_now(&self) -> AskedNow {
        match self {
            ProgramQuestion::Permission(question) => {
                AskedNow::Question(protocol_question(*question))
            }
            ProgramQuestion::LinkScheme(scheme) => AskedNow::LinkScheme(scheme.clone()),
            ProgramQuestion::Role(Role::Inspector) => {
                AskedNow::Role(waos_dev_protocol::Role::Inspector)
            }
        }
    }
}

fn protocol_question(question: Question) -> waos_dev_protocol::Question {
    match question {
        Question::Notifications => waos_dev_protocol::Question::Notifications,
        Question::Camera => waos_dev_protocol::Question::Camera,
        Question::Microphone => waos_dev_protocol::Question::Microphone,
    }
}

fn protocol_switch(switch: Switch) -> waos_dev_protocol::Switch {
    match switch {
        Switch::Network => waos_dev_protocol::Switch::Network,
        Switch::LocalNetwork => waos_dev_protocol::Switch::LocalNetwork,
        Switch::ClipboardRead => waos_dev_protocol::Switch::ClipboardRead,
        Switch::ClipboardWrite => waos_dev_protocol::Switch::ClipboardWrite,
    }
}

/// Sends `asking` of `program`'s to the main thread, unless WaOS is quitting, and waits on
/// `received`; dropping the returned future before it resolves drops the sender of `cancelled`.
fn send_and_wait<T: Send>(
    questions: &UnboundedSender<Asked>,
    program: &RunningProgram,
    asking: Asking,
    received: oneshot::Receiver<Option<T>>,
) -> impl Future<Output = Option<T>> + Send + use<T> {
    let (cancel, cancelled) = oneshot::channel();
    let asked = Asked {
        program: program.clone(),
        asking,
        cancelled,
    };
    if let Err(unsent) = questions.unbounded_send(asked) {
        let asked = unsent.into_inner();
        tracing::info!(
            program = program.name(),
            asking = ?asked.asking,
            "WaOS is quitting: unanswered"
        );
    }
    async move {
        // A question dropped unanswered, here or on the main thread, answers none.
        let answer = received.await.unwrap_or(None);
        drop(cancel);
        answer
    }
}
