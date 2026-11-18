//! Where the programs' prompts go: each to the main thread, a question to be shown to the user
//! (`state::questions`), a permission's, a link scheme's or a role's, which the engine's prompts ask, the
//! output a program's sound plays on, which the shell's picker of outputs asks, or a machine for a
//! program, which the shell's picker of machines asks; and a switch's
//! page to be opened in the library's settings (`state::library`). Each is noted as asked of the
//! program's user until it is answered (`asking_now`), for a run of `cargo waos run` to say. The engine asks from the
//! program's thread and is answered later, through the future it was given: a question with the
//! user's reply, or none when nobody replied, because the main thread stopped listening or dropped
//! it; a switch's page once the user left it, or at once when nobody can open it.
//!
//! The invariant: every prompt taken reaches the main thread or is answered at once, so no
//! program waits for an answer that cannot come.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

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

/// One event of the programs' questions on its way to the main thread.
pub(crate) enum QuestionEvent {
    Asked(Asked),
    /// The future waiting on question `id` was dropped, so its prompt goes if still waiting.
    Cancelled(u64),
}

/// One program's question, on its way to the main thread.
pub(crate) struct Asked {
    pub(crate) id: u64,
    pub(crate) program: RunningProgram,
    pub(crate) asking: Asking,
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

    /// Whether the caller waiting on this question has already dropped its future.
    pub(crate) fn is_canceled(&self) -> bool {
        match self {
            Asking::Question { answer, .. } => answer.is_canceled(),
            Asking::Output { picked, .. } | Asking::Machine { picked, .. } => picked.is_canceled(),
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
    pub(crate) questions: UnboundedReceiver<QuestionEvent>,
    pub(crate) settings: UnboundedReceiver<SettingsOpened>,
}

/// The app's prompts, as the engine reaches them: every one goes to the main thread, noted in
/// `asking` until it is answered.
pub(crate) struct PromptsOnRequest {
    questions: UnboundedSender<QuestionEvent>,
    settings: UnboundedSender<SettingsOpened>,
    asking: Arc<AskingNow>,
    next_id: Arc<AtomicU64>,
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
                next_id: Arc::new(AtomicU64::new(1)),
            },
            requests,
        )
    }

    /// The picker of outputs, whose questions arrive with these prompts' questions.
    pub(crate) fn output_picker(&self) -> OutputPickerOnRequest {
        OutputPickerOnRequest {
            questions: self.questions.clone(),
            asking: Arc::clone(&self.asking),
            next_id: Arc::clone(&self.next_id),
        }
    }

    /// The picker of machines, whose questions arrive with these prompts' questions.
    pub(crate) fn machine_picker(&self) -> MachinePickerOnRequest {
        MachinePickerOnRequest {
            questions: self.questions.clone(),
            asking: Arc::clone(&self.asking),
            next_id: Arc::clone(&self.next_id),
        }
    }
}

impl PromptsOnRequest {
    /// Sends `question` of `program`'s to the main thread, and resolves to the user's answer.
    fn put(&self, program: &RunningProgram, question: ProgramQuestion) -> AnswerFuture {
        let (answer, answered) = oneshot::channel();
        let noted = question.asked_now();
        let asking = Asking::Question { question, answer };
        let wait = send_and_wait(&self.questions, &self.next_id, program, asking, answered);
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
    questions: UnboundedSender<QuestionEvent>,
    asking: Arc<AskingNow>,
    next_id: Arc<AtomicU64>,
}

impl ShellAudioOutputPicker for OutputPickerOnRequest {
    fn pick(&self, program: &RunningProgram, outputs: Vec<AudioOutput>) -> AudioOutputPickedFuture {
        let (picked, picking) = oneshot::channel();
        let asking = Asking::Output { outputs, picked };
        let wait = send_and_wait(&self.questions, &self.next_id, program, asking, picking);
        Box::pin(
            self.asking
                .while_asked(program, AskedNow::AudioOutput, wait),
        )
    }
}

/// The app's picker of machines, as the engine reaches it: each pick is a question, shown as the
/// prompts' questions are.
pub(crate) struct MachinePickerOnRequest {
    questions: UnboundedSender<QuestionEvent>,
    asking: Arc<AskingNow>,
    next_id: Arc<AtomicU64>,
}

impl ShellMachinePicker for MachinePickerOnRequest {
    fn pick(&self, program: &RunningProgram, machines: Vec<MachineChoice>) -> MachinePickedFuture {
        let (picked, picking) = oneshot::channel();
        let asking = Asking::Machine { machines, picked };
        let wait = send_and_wait(&self.questions, &self.next_id, program, asking, picking);
        Box::pin(self.asking.while_asked(program, AskedNow::Machine, wait))
    }
}

impl ProgramQuestion {
    /// The question, as a run of `cargo waos run` is told it is asked.
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
/// `received`; dropping the returned future before it resolves sends `Cancelled(id)` so any
/// panel shown for it closes.
fn send_and_wait<T>(
    questions: &UnboundedSender<QuestionEvent>,
    next_id: &AtomicU64,
    program: &RunningProgram,
    asking: Asking,
    received: oneshot::Receiver<Option<T>>,
) -> WaitQuestion<T> {
    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let asked = Asked {
        id,
        program: program.clone(),
        asking,
    };
    let sent = if let Err(unsent) = questions.unbounded_send(QuestionEvent::Asked(asked)) {
        if let QuestionEvent::Asked(asked) = unsent.into_inner() {
            tracing::info!(
                program = program.name(),
                asking = ?asked.asking,
                "WaOS is quitting: unanswered"
            );
        }
        false
    } else {
        true
    };
    WaitQuestion {
        id,
        questions: questions.clone(),
        received,
        done: !sent,
    }
}

/// Waits for a question's `oneshot` reply, telling the main thread to close its prompt if
/// dropped before a reply arrives.
struct WaitQuestion<T> {
    id: u64,
    questions: UnboundedSender<QuestionEvent>,
    received: oneshot::Receiver<Option<T>>,
    done: bool,
}

impl<T> Future for WaitQuestion<T> {
    type Output = Option<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.received.poll_unpin(cx) {
            Poll::Ready(answer) => {
                self.done = true;
                // A question dropped unanswered, here or on the main thread, answers none.
                Poll::Ready(answer.unwrap_or(None))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T> Drop for WaitQuestion<T> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self
                .questions
                .unbounded_send(QuestionEvent::Cancelled(self.id));
        }
    }
}
