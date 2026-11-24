//! What a program asks its user, as a plain sheet in Fluent's controls: the question on Fluent's
//! solid background, filling what its placement gives it, with its replies: a question's two
//! answers, Allow and Don't allow; or the outputs its sound may play on, or the machines it may act
//! on as the user, a button each, scrolling when there are many, and Cancel; or that WaOS still
//! may not reach the phone's files, which something the user gave the program needs, and OK
//! (`logic::phone_files`). It is the program's
//! prompt, one widget wherever its placement shows it (`state::questions`), which dims what it is
//! shown over, if anything; nothing while the program asks nothing.

use std::fmt;
use waft::{
    App, BoxConstraints, BuildContext, ColoredBox, Column, ConstrainedBox, CrossAxisAlignment,
    Directionality, EdgeInsetsGeometry, Entity, IntoWidget, Listener, MainAxisAlignment,
    MainAxisSize, Padding, Row, SingleChildScrollView, SizedBox, StatelessWidget, Text,
    TextDirection, WidgetRef,
};
use waft_winui::{Button, ButtonStyle, TextBlockStyle, ThemeResources};
use waos::{Answer, AudioOutput, MachineChoice, Question, Role, RunningProgram};

use crate::logic::prompts::{Asking, ProgramQuestion, Reply};
use crate::state::questions::Questions;

/// The question's margins.
const MARGIN: f64 = 20.0;
const GAP: f64 = 8.0;

/// How tall the list of outputs or machines grows before it scrolls.
const CHOICES_HEIGHT: f64 = 200.0;

/// The question `program` asks its user now, if any.
pub(crate) struct QuestionSheet {
    pub(crate) questions: Entity<Questions>,
    pub(crate) program: RunningProgram,
}

impl fmt::Debug for QuestionSheet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuestionSheet")
            .field("program", &self.program.name())
            .finish_non_exhaustive()
    }
}

impl StatelessWidget for QuestionSheet {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let Some(pending) = self.questions.read(app).asked_of(&self.program) else {
            return SizedBox::shrink().into_widget();
        };
        let parts = match &pending.asking {
            Asking::Question { question, .. } => self.question(&pending.name, question),
            Asking::Output { outputs, .. } => self.output_picker(&pending.name, outputs),
            Asking::Machine { machines, .. } => self.machine_picker(&pending.name, machines),
            Asking::PhoneFilesOff { .. } => self.phone_files_off(),
        };
        let resources = ThemeResources::of(app, context);
        let mut children = vec![
            Text::new(parts.title)
                .style(
                    TextBlockStyle::Subtitle
                        .text_style(&resources, resources.text_fill_color_primary),
                )
                .into_widget(),
            Text::new(parts.detail)
                .style(
                    TextBlockStyle::Body
                        .text_style(&resources, resources.text_fill_color_secondary),
                )
                .into_widget(),
        ];
        children.extend(parts.choices);
        children.push(
            Row::new()
                .main_axis_alignment(MainAxisAlignment::End)
                .spacing(GAP)
                .children(parts.answers)
                .into_widget(),
        );
        let sheet = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(GAP * 2.0)
            .children(children);
        let card = ColoredBox::new(resources.solid_background_fill_color_base)
            .child(Padding::new(EdgeInsetsGeometry::all(MARGIN)).child(sheet));
        Directionality::new(TextDirection::Ltr, card).into_widget()
    }
}

/// A question as the sheet shows it: what it asks, what it says besides, what to choose from, and
/// the buttons along its foot.
struct SheetParts {
    title: String,
    detail: &'static str,
    choices: Option<WidgetRef>,
    answers: Vec<WidgetRef>,
}

impl QuestionSheet {
    /// Whether `name` may do what `question` names: Don't allow, or Allow.
    fn question(&self, name: &str, question: &ProgramQuestion) -> SheetParts {
        SheetParts {
            title: asked(name, question),
            detail: detail(question),
            choices: None,
            answers: vec![
                Button::text(DONT_ALLOW, self.answer(Answer::Denied)).into_widget(),
                Button::text(ALLOW, self.answer(Answer::Granted))
                    .style(ButtonStyle::Accent)
                    .into_widget(),
            ],
        }
    }

    /// Which of `outputs` `name`'s sound plays on: a button each, and Cancel.
    fn output_picker(&self, name: &str, outputs: &[AudioOutput]) -> SheetParts {
        let buttons = outputs
            .iter()
            .map(|output| {
                let reply = Reply::Output(output.id.clone());
                Button::text(output.label.clone(), self.reply(Some(reply))).into_widget()
            })
            .collect();
        SheetParts {
            title: picking(name),
            detail: PICKED_DETAIL,
            choices: Some(choices(buttons)),
            answers: vec![Button::text(CANCEL, self.reply(None)).into_widget()],
        }
    }

    /// Which of `machines` `name` acts on as the user: a button each, and Cancel.
    fn machine_picker(&self, name: &str, machines: &[MachineChoice]) -> SheetParts {
        let buttons = machines
            .iter()
            .map(|machine| {
                let reply = Reply::Machine(machine.id.clone());
                Button::text(machine.name.clone(), self.reply(Some(reply))).into_widget()
            })
            .collect();
        SheetParts {
            title: format!("Pick a machine for {name}"),
            detail: MACHINE_DETAIL,
            choices: Some(choices(buttons)),
            answers: vec![Button::text(CANCEL, self.reply(None)).into_widget()],
        }
    }

    /// The user came back from the system's page and WaOS still may not reach the phone's files:
    /// the sheet says so, and OK ends it.
    fn phone_files_off(&self) -> SheetParts {
        SheetParts {
            title: PHONE_FILES_OFF_TITLE.to_owned(),
            detail: PHONE_FILES_OFF_DETAIL,
            choices: None,
            answers: vec![
                Button::text(OK, self.reply(None))
                    .style(ButtonStyle::Accent)
                    .into_widget(),
            ],
        }
    }

    fn answer(&self, answer: Answer) -> Listener {
        self.reply(Some(Reply::Answer(answer)))
    }

    fn reply(&self, reply: Option<Reply>) -> Listener {
        let (questions, program) = (self.questions.clone(), self.program.clone());
        Listener::new(move |app| {
            let reply = reply.clone();
            questions.update(app, |questions, cx| questions.reply(&program, reply, cx));
        })
    }
}

/// `buttons`, one above the other, scrolling past the sheet's height for them.
fn choices(buttons: Vec<WidgetRef>) -> WidgetRef {
    let list = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(GAP)
        .children(buttons);
    ConstrainedBox::new(BoxConstraints::new().max_height(CHOICES_HEIGHT))
        .child(SingleChildScrollView::new().child(list))
        .into_widget()
}

const ALLOW: &str = "Allow";
const DONT_ALLOW: &str = "Don't allow";
const CANCEL: &str = "Cancel";
const OK: &str = "OK";
const PHONE_FILES_OFF_TITLE: &str = "All files access is off";
const PHONE_FILES_OFF_DETAIL: &str =
    "WaOS can't read the files on this phone until you turn it on for WaOS in Android's settings.";
const PICKED_DETAIL: &str = "It may play there until it quits.";
const INSPECTOR_DETAIL: &str = "It sees the sites they reach, the files they open and change, when they use the camera and the microphone, and what they run, and it can start and end them. You can change this in its settings in WaOS's library.";
const MACHINE_DETAIL: &str = "It acts on the machine you pick as you: it can run any command there and read and change any file you can, until you remove the machine in its settings in WaOS's library.";

/// What the user is asked, `name` being the program's.
fn asked(name: &str, question: &ProgramQuestion) -> String {
    let what = match question {
        ProgramQuestion::Permission(Question::Notifications) => "show notifications".to_owned(),
        ProgramQuestion::Permission(Question::Camera) => "use the camera".to_owned(),
        ProgramQuestion::Permission(Question::Microphone) => "use the microphone".to_owned(),
        ProgramQuestion::LinkScheme(scheme) => format!("open {scheme} links"),
        ProgramQuestion::Role(Role::Inspector) => "see what your other programs do".to_owned(),
    };
    format!("Allow {name} to {what}?")
}

/// What the user is told beneath the question.
fn detail(question: &ProgramQuestion) -> &'static str {
    match question {
        ProgramQuestion::Permission(_) => "You can change this in its settings in WaOS's library.",
        ProgramQuestion::LinkScheme(_) => "They open in another app. WaOS keeps your answer.",
        ProgramQuestion::Role(Role::Inspector) => INSPECTOR_DETAIL,
    }
}

/// What the picker of outputs asks, `name` being the program's.
fn picking(name: &str) -> String {
    format!("Where should {name} play its sound?")
}
