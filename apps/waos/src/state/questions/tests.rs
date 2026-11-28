//! The questions apps put to their user. Their rules, on a desktop of the tests' own for the apps'
//! windows over the one startup kept in memory (`state::test_windows`): an app's questions are
//! asked one at a time, the next once the first is replied to; the question of an app with a
//! window is put over that window, and one of an app with none has a window of its own, closed
//! once it is replied to; and a question whose asker stops waiting goes, its prompt with it.
//!
//! The question sheet's words and buttons, until the sheet takes values and callbacks, on the
//! test desktop with the library (`state::library::test_desktop`): a question about opening links
//! of a scheme or about a role says what it asks, and the pickers of outputs and machines answer
//! what is tapped, or none when cancelled.

use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use waft::ui::{View as _, ViewId};
use waft_test::TestWindow;
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{
    Answer, AppInstance, AudioOutput, AudioOutputPickedFuture, MachineChoice, Question,
    ShellAudioOutputPicker, ShellMachinePicker,
};
use waos_package::ProgramId;
use waos_services::permissions::{App, Prompts};

use crate::logic::prompts::{Asking, ProgramQuestion, Reply};
use crate::state::library::test_desktop::LibraryDesktop;
use crate::state::test_system::Lines;
use crate::state::test_windows::Desktop;

const PERMISSIONS: &str = "dev.example.permissions";
const FOREVER: &str = "dev.example.forever";

/// Forever, an app with no window, as its permissions ask about it.
fn forever() -> App {
    App {
        id: FOREVER.parse().unwrap(),
        name: "Forever".to_owned(),
    }
}

/// The permission `app`'s user is asked about now, if a permission is what they are asked.
fn asked_now(desktop: &Desktop, app: &ProgramId) -> Option<Question> {
    let app_state = desktop.shell.app();
    let questions = desktop.questions.read(&app_state);
    match &questions.asked_of(app)?.asking {
        Asking::Question {
            question: ProgramQuestion::Permission(question),
            ..
        } => Some(*question),
        _ => None,
    }
}

/// `app`'s user replies `answer` to the question they are asked now.
fn reply(desktop: &mut Desktop, app: &ProgramId, answer: Answer) {
    let reply = Some(Reply::Answer(answer));
    desktop
        .questions
        .update(&mut desktop.shell.app(), |questions, cx| {
            questions.reply(app, reply, cx);
        });
}

#[test]
fn an_apps_questions_are_asked_one_at_a_time() {
    let mut desktop = Desktop::new();
    let id = forever().id;
    let mut camera = desktop.prompts.ask(&forever(), Question::Camera);
    let mut microphone = desktop.prompts.ask(&forever(), Question::Microphone);
    desktop.pump_until("the first question asked", |desktop| {
        asked_now(desktop, &id) == Some(Question::Camera)
    });

    reply(&mut desktop, &id, Answer::Granted);

    desktop.pump_until("the next question asked", |desktop| {
        asked_now(desktop, &id) == Some(Question::Microphone)
    });
    assert_eq!(camera.as_mut().now_or_never(), Some(Some(Answer::Granted)));
    assert_eq!(microphone.as_mut().now_or_never(), None, "not answered yet");
    assert_eq!(desktop.windows_made(), 1, "in the one prompt");
}

#[test]
fn the_question_of_an_app_with_a_window_is_put_over_that_window() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    let component = waos_test_programs::PERMISSIONS;
    desktop.run_as(component, PERMISSIONS, "Permissions", &output);
    let id: ProgramId = PERMISSIONS.parse().unwrap();
    desktop.pump_until("its window, and its question", |desktop| {
        desktop.windows_made() == 1 && asked_now(desktop, &id) == Some(Question::Notifications)
    });
    assert_eq!(desktop.windows_made(), 1, "no window of the question's own");
}

#[test]
fn the_question_of_an_app_with_no_window_has_a_window_of_its_own_closed_once_replied_to() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    desktop.run_as(waos_test_programs::FOREVER, FOREVER, "Forever", &output);
    let id = forever().id;
    let mut answered = desktop.prompts.ask(&forever(), Question::Camera);
    desktop.pump_until("the question's window", |desktop| {
        desktop.windows_made() == 1 && asked_now(desktop, &id).is_some()
    });
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    assert_eq!(window.config().title, "Forever");

    reply(&mut desktop, &id, Answer::Denied);

    desktop.pump_until("the question's window closes", |_| window.closes() == 1);
    assert_eq!(answered.as_mut().now_or_never(), Some(Some(Answer::Denied)));
}

#[test]
fn a_question_whose_asker_stops_waiting_goes_with_its_prompt() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    desktop.run_as(waos_test_programs::FOREVER, FOREVER, "Forever", &output);
    let id = forever().id;
    let answered = desktop.prompts.ask(&forever(), Question::Camera);
    desktop.pump_until("the question's window", |desktop| {
        desktop.windows_made() == 1 && asked_now(desktop, &id).is_some()
    });
    let window = Rc::clone(&desktop.platform.windows.made()[0]);

    drop(answered);

    desktop.pump_until("the question's window closes", |_| window.closes() == 1);
    assert_eq!(asked_now(&desktop, &id), None, "the question went");
}

/// The desktop with no library window: only the programs' and the questions'.
fn library_desktop() -> LibraryDesktop {
    let (desktop, ()) =
        LibraryDesktop::new(|_app, _library, _recordings, _view_windows| (None, ()));
    desktop
}

/// Installs the console program `id`, named `name`, made of the test program at `component`, and
/// starts it as a click on one of its notifications does, the one way the library starts a
/// program with no window.
fn start_console_program(desktop: &mut LibraryDesktop, id: &str, name: &str, component: &str) {
    desktop.install_compiled(id, name, component);
    let id: ProgramId = id.parse().unwrap();
    let click = Activation {
        id: "greeting".into(),
        action: None,
        reply: None,
    };
    desktop.update(|library, cx| library.notification_activated(&id, click, cx));
    let runs = Arc::clone(&desktop.runs);
    let named = id.to_string();
    desktop.settle_until("the program runs", move |_| {
        futures::executor::block_on(runs.names()).contains(&named)
    });
}

/// The window titled `title`, the last made.
fn window_titled(desktop: &LibraryDesktop, title: &str) -> Option<Rc<TestWindow>> {
    desktop
        .windows()
        .into_iter()
        .rev()
        .find(|window| window.config().title == title)
}

fn view_of(window: &TestWindow) -> ViewId {
    window.test_view().id()
}

/// The one program running.
fn the_program(desktop: &LibraryDesktop) -> AppInstance {
    let programs = desktop.runs.instances();
    assert_eq!(programs.len(), 1);
    programs.into_iter().next().unwrap()
}

/// The one program running, as its permissions ask about it.
fn the_app(desktop: &LibraryDesktop, name: &str) -> App {
    App {
        id: the_program(desktop).name().parse().unwrap(),
        name: name.to_owned(),
    }
}

#[test]
fn a_programs_question_about_a_link_scheme_is_put_as_the_others_are() {
    let mut desktop = library_desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut answered = desktop
        .prompts
        .ask_to_open_links(&the_app(&desktop, "Forever"), "zoommtg");
    desktop.settle_until("the question's window", |desktop| {
        desktop.shows("Allow Forever to open zoommtg links?")
    });
    let window = window_titled(&desktop, "Forever").expect("the question's window");
    desktop.tap(view_of(&window), "Allow");
    desktop.settle_until("the answer", |_| true);
    assert_eq!(
        answered.as_mut().now_or_never(),
        Some(Some(Answer::Granted))
    );
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_programs_question_about_a_role_says_what_the_role_sees() {
    let mut desktop = library_desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut answered = desktop
        .prompts
        .ask_for_role(&the_app(&desktop, "Forever"), waos::Role::Inspector);
    desktop.settle_until("the question's window", |desktop| {
        desktop.shows("Allow Forever to see what your other programs do?")
    });
    let window = window_titled(&desktop, "Forever").expect("the question's window");
    desktop.tap(view_of(&window), "Don't allow");
    desktop.settle_until("the answer", |_| true);
    assert_eq!(answered.as_mut().now_or_never(), Some(Some(Answer::Denied)));
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_picker_of_outputs_answers_the_output_tapped() {
    let mut desktop = library_desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut picked = pick(&desktop);
    desktop.settle_until("the picker", |desktop| {
        desktop.shows("Where should Forever play its sound?")
    });
    assert!(desktop.shows("Speakers"), "{:?}", desktop.labels());
    let window = window_titled(&desktop, "Forever").expect("the picker's window");
    desktop.tap(view_of(&window), "Headphones");
    desktop.settle_until("the pick", |_| true);
    assert_eq!(
        picked.as_mut().now_or_never(),
        Some(Some("headphones".to_owned()))
    );
    assert_eq!(window.closes(), 1, "closed once picked");
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_picker_cancelled_picks_nothing() {
    let mut desktop = library_desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut picked = pick(&desktop);
    desktop.settle_until("the picker", |desktop| desktop.shows("Headphones"));
    let window = window_titled(&desktop, "Forever").expect("the picker's window");
    desktop.tap(view_of(&window), "Cancel");
    desktop.settle_until("the cancel", |_| true);
    assert_eq!(picked.as_mut().now_or_never(), Some(None));
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_picker_of_machines_answers_the_machine_tapped_or_none_when_cancelled() {
    let mut desktop = library_desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let this_computer = MachineChoice {
        id: "this-computer".to_owned(),
        name: "This computer".to_owned(),
    };
    for (tapped, picked) in [
        ("This computer", Some("this-computer".to_owned())),
        ("Cancel", None),
    ] {
        let mut answered = desktop
            .machine_picker
            .pick(&the_program(&desktop), vec![this_computer.clone()]);
        desktop.settle_until("the machines to pick from", |desktop| {
            desktop.shows("Pick a machine for Forever")
        });
        let window = window_titled(&desktop, "Forever").expect("the picker's window");
        desktop.tap(view_of(&window), tapped);
        desktop.settle_until("the pick", |_| true);
        assert_eq!(answered.as_mut().now_or_never(), Some(picked), "{tapped}");
    }
    futures::executor::block_on(desktop.runs.stop_all());
}

/// The one program running asks its user to pick one of two outputs, as the engine asks for it.
fn pick(desktop: &LibraryDesktop) -> AudioOutputPickedFuture {
    let outputs = vec![
        AudioOutput {
            id: "speakers".to_owned(),
            group_id: None,
            label: "Speakers".to_owned(),
        },
        AudioOutput {
            id: "headphones".to_owned(),
            group_id: Some("jack".to_owned()),
            label: "Headphones".to_owned(),
        },
    ];
    desktop.output_picker.pick(&the_program(desktop), outputs)
}
