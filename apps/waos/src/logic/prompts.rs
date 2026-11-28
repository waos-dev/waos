//! Where the apps' prompts go: each to the main thread, a question to be shown to the user
//! (`state::questions`), a permission's, a link scheme's or a role's, which an app's permissions
//! ask (`waos_services::permissions`), the output a program's sound plays on, which the shell's picker of
//! outputs asks, a machine for a program, which the shell's picker of machines asks, or that WaOS
//! still may not reach the phone's files, which the user is told once they come back from the
//! system's page without them (`logic::phone_files`); and a switch's page to be opened in the
//! library's settings (`state::library`). A question about an app is shown over its run, which the
//! running apps name, once it is among them. A picker is noted as asked of the program's user
//! until it is answered (`asking_now`), for an attach to the program to say; Permissions lists its
//! own questions. Each is answered later, through the future it was given: a question with the
//! user's reply, or none when nobody replied, because the main thread stopped listening or dropped
//! it; a switch's page once the user left it, or at once when nobody can open it.
//!
//! The invariant: every prompt taken reaches the main thread or is answered at once, or waits
//! for its app's run, so no app waits for an answer that cannot come while it runs.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::{
    Answer, AnswerFuture, AudioOutput, AudioOutputPickedFuture, MachineChoice, MachinePickedFuture,
    Question, Role, RunningProgram, SettingsClosedFuture, ShellAudioOutputPicker,
    ShellMachinePicker, Switch,
};
use waos_dev_protocol::Asked as AskedNow;
use waos_package::ProgramId;
use waos_services::permissions::{App, Prompts};

use super::asking_now::AskingNow;
use super::phone_files::{PhoneFilesOnRequest, asks_for_phone_files};
use super::running_apps::RunningApps;

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
    /// Nothing: its user is told that WaOS still may not reach the phone's files, which something
    /// they gave the program needs, having come back from the system's page without them. Answered
    /// once they have read it.
    PhoneFilesOff { read: oneshot::Sender<Option<()>> },
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
            Asking::PhoneFilesOff { .. } => f.write_str("PhoneFilesOff"),
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
    /// kind; what only tells the user something is read whatever they reply. A program gone
    /// meanwhile hears nothing.
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
            (Asking::PhoneFilesOff { read }, _) => {
                let _ = read.send(Some(()));
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

/// The app's prompts, as Permissions reaches them: every one goes to the main thread, about the
/// run of its app among `running`.
pub(crate) struct PromptsOnRequest {
    questions: UnboundedSender<Asked>,
    settings: UnboundedSender<SettingsOpened>,
    running: Arc<RunningApps>,
}

impl PromptsOnRequest {
    /// The prompts, each shown over its app's run among `running`, and where they arrive on the
    /// main thread.
    pub(crate) fn new(running: Arc<RunningApps>) -> (PromptsOnRequest, PromptRequests) {
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
                running,
            },
            requests,
        )
    }

    /// The picker of outputs, whose questions arrive with these prompts' questions, each noted in
    /// `asking` while it waits.
    pub(crate) fn output_picker(&self, asking: Arc<AskingNow>) -> OutputPickerOnRequest {
        OutputPickerOnRequest {
            questions: self.questions.clone(),
            asking,
        }
    }

    /// The picker of machines, whose questions arrive with these prompts' questions, each noted in
    /// `asking` while it waits; a pick of This computer asks for the phone's files through
    /// `phone_files`, where WaOS reaches them only as the user allows.
    pub(crate) fn machine_picker(
        &self,
        asking: Arc<AskingNow>,
        phone_files: Option<PhoneFilesOnRequest>,
    ) -> MachinePickerOnRequest {
        MachinePickerOnRequest {
            questions: self.questions.clone(),
            asking,
            phone_files,
        }
    }

    /// What tells a program's user that WaOS still may not reach the phone's files, shown as these
    /// prompts' questions are.
    pub(crate) fn phone_files_off(&self) -> PhoneFilesOffNotice {
        PhoneFilesOffNotice {
            questions: self.questions.clone(),
        }
    }
}

/// What tells a program's user that WaOS still may not reach the phone's files, from any thread.
#[derive(Clone)]
pub(crate) struct PhoneFilesOffNotice {
    questions: UnboundedSender<Asked>,
}

impl PhoneFilesOffNotice {
    /// Tells `program`'s user, and resolves once they have read it, or nobody can.
    pub(crate) fn tell(&self, program: &RunningProgram) -> impl Future<Output = ()> + Send + use<> {
        let (read, reading) = oneshot::channel();
        let wait = send_and_wait(
            &self.questions,
            program,
            Asking::PhoneFilesOff { read },
            reading,
        );
        wait.map(drop)
    }
}

impl PromptsOnRequest {
    /// Sends `question` about `app` to the main thread, over its run, and resolves to the user's
    /// answer.
    fn put(&self, app: &App, question: ProgramQuestion) -> AnswerFuture {
        let questions = self.questions.clone();
        let send = move |program: RunningProgram| {
            let (answer, answered) = oneshot::channel();
            let asking = Asking::Question { question, answer };
            send_and_wait(&questions, &program, asking, answered)
        };
        // Sent at once where the app runs, so the question reaches the main thread as it is asked.
        match self.running.running(&app.id) {
            Some(program) => Box::pin(send(program)),
            None => {
                let run = run_of(&self.running, &app.id);
                Box::pin(async move { send(run.await).await })
            }
        }
    }
}

impl Prompts for PromptsOnRequest {
    fn ask(&self, app: &App, question: Question) -> AnswerFuture {
        self.put(app, ProgramQuestion::Permission(question))
    }

    fn ask_to_open_links(&self, app: &App, scheme: &str) -> AnswerFuture {
        self.put(app, ProgramQuestion::LinkScheme(scheme.to_owned()))
    }

    fn ask_for_role(&self, app: &App, role: Role) -> AnswerFuture {
        self.put(app, ProgramQuestion::Role(role))
    }

    fn open_settings(&self, app: &App, switch: Switch) -> SettingsClosedFuture {
        let (settings, run) = (self.settings.clone(), run_of(&self.running, &app.id));
        Box::pin(async move {
            let program = run.await;
            let (left, leaving) = oneshot::channel();
            let name = program.name().to_owned();
            let opened = SettingsOpened {
                program,
                switch,
                left,
            };
            if settings.unbounded_send(opened).is_err() {
                tracing::info!(program = name, ?switch, "WaOS is quitting: no settings");
            }
            // A page dropped, or never opened, counts as left.
            let _left = leaving.await;
        })
    }
}

/// The run of the app `id` among `running`, once it is there: a question about an app is shown
/// over its run, and one asked as it starts, its role's, comes before the run is listed.
fn run_of(
    running: &Arc<RunningApps>,
    id: &ProgramId,
) -> impl Future<Output = RunningProgram> + Send + use<> {
    let (running, id) = (Arc::clone(running), id.clone());
    let mut listed = running.running_ids();
    async move {
        loop {
            if let Some(program) = running.running(&id) {
                return program;
            }
            if listed.changed().await.is_err() {
                return std::future::pending().await;
            }
        }
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
/// prompts' questions are. This computer picked while WaOS may not reach the phone's files asks
/// the system for them before the pick is answered, and is answered whatever the user did there:
/// without them, This computer's home is WaOS's own folder.
pub(crate) struct MachinePickerOnRequest {
    questions: UnboundedSender<Asked>,
    asking: Arc<AskingNow>,
    /// What asks for the phone's files, where WaOS reaches them only as the user allows.
    phone_files: Option<PhoneFilesOnRequest>,
}

impl ShellMachinePicker for MachinePickerOnRequest {
    fn pick(&self, program: &RunningProgram, machines: Vec<MachineChoice>) -> MachinePickedFuture {
        let (picked, picking) = oneshot::channel();
        let asking = Asking::Machine { machines, picked };
        let wait = send_and_wait(&self.questions, program, asking, picking);
        let (phone_files, asked_for) = (self.phone_files.clone(), program.clone());
        let picked = async move {
            let picked = wait.await;
            if let Some(phone_files) = phone_files
                && asks_for_phone_files(picked.as_deref(), phone_files.reachable())
            {
                phone_files.request(&asked_for).await;
            }
            picked
        };
        Box::pin(self.asking.while_asked(program, AskedNow::Machine, picked))
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
