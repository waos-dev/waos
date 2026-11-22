//! The questions programs put to their user, on a desktop of the tests' own
//! (`state::library::test_desktop`): `examples/permissions`, which has a window, has its question
//! put over it, and the answer the user taps is kept in its settings; a question of a program with
//! no window, `examples/forever`, a console program started to hear a click on its notification,
//! has a window of its own, whose answer reaches the program and
//! which closes with it; a program's question about opening links of a scheme is put the same
//! way; a question whose program ends goes unanswered; and the picker of a program's outputs, a
//! question too, answers the output the user taps, or none when they cancel it, as the picker of a
//! machine for it does; and This computer picked where WaOS may not reach the phone's files offers
//! the system's page for them first, the pick answered as the user goes on without it, or comes
//! back to WaOS from it.

use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use waft::ui::{View as _, ViewId};
use waft_test::TestWindow;
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{
    Answer, AnswerFuture, AudioOutput, AudioOutputPickedFuture, MachineChoice, Prompts, Question,
    RunningProgram, ShellAudioOutputPicker, ShellMachinePicker,
};
use waos_package::ProgramId;
use waos_store::Setting;

use crate::logic::phone_files::TestPhone;
use crate::state::library::test_desktop::{DesktopKind, LibraryDesktop};

const PERMISSIONS: &str = "dev.example.permissions";
const FOREVER: &str = "dev.example.forever";

/// The desktop with no library window: only the programs' and the questions'.
fn desktop() -> LibraryDesktop {
    let (desktop, ()) =
        LibraryDesktop::new(|_app, _library, _recordings, _view_windows| (None, ()));
    desktop
}

/// Installs the program `id`, named `name`, made of the test program at `component`, and opens
/// it from the library.
fn open_program(desktop: &mut LibraryDesktop, id: &str, name: &str, component: &str) {
    let component = std::fs::read(component).unwrap();
    let package = desktop.package(id, name, "1.0.0", &component);
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    desktop.update(|library, cx| library.answer(true, cx));
    let id: ProgramId = id.parse().unwrap();
    desktop.update(|library, cx| library.open(&id, cx));
    let runs = Arc::clone(&desktop.runs);
    let named = id.to_string();
    desktop.settle_until("the program runs", move |_| {
        futures::executor::block_on(runs.names()).contains(&named)
    });
}

/// Installs the console program `id`, named `name`, made of the test program at `component`, and
/// starts it as a click on one of its notifications does, the one way the library starts a
/// program with no window.
fn start_console_program(desktop: &mut LibraryDesktop, id: &str, name: &str, component: &str) {
    let component = std::fs::read(component).unwrap();
    let package = desktop.package(id, name, "1.0.0", &component);
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    desktop.update(|library, cx| library.answer(true, cx));
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
fn the_program(desktop: &LibraryDesktop) -> RunningProgram {
    let programs = futures::executor::block_on(desktop.runs.programs());
    assert_eq!(programs.len(), 1);
    programs.into_iter().next().unwrap()
}

#[test]
fn a_programs_question_is_put_over_its_window_and_the_answer_is_kept() {
    let mut desktop = desktop();
    open_program(
        &mut desktop,
        PERMISSIONS,
        "Permissions",
        waos_test_programs::PERMISSIONS,
    );
    let asked = "Allow Permissions to show notifications?";
    desktop.settle_until("the question", |desktop| desktop.shows(asked));
    let window = window_titled(&desktop, "Permissions").expect("the program's window");
    assert_eq!(
        desktop.windows().len(),
        1,
        "no window of the question's own"
    );
    desktop.tap(view_of(&window), "Allow");
    let (store, id) = (
        desktop.store.clone(),
        PERMISSIONS.parse::<ProgramId>().unwrap(),
    );
    desktop.settle_until("the answer kept", move |_| {
        let settings = store.program_settings(&id).unwrap();
        *settings.answers.get(Question::Notifications) == Setting::Own(Answer::Granted)
    });
    assert!(!desktop.shows(asked));
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_question_of_a_program_with_no_window_has_a_window_of_its_own() {
    let mut desktop = desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut answered = ask(&desktop, Question::Camera);
    desktop.settle_until("the question's window", |desktop| {
        desktop.shows("Allow Forever to use the camera?")
    });
    let window = window_titled(&desktop, "Forever").expect("the question's window");
    desktop.tap(view_of(&window), "Don't allow");
    desktop.settle_until("the answer", |_| true);
    assert_eq!(answered.as_mut().now_or_never(), Some(Some(Answer::Denied)));
    assert_eq!(window.closes(), 1, "closed once answered");
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_question_whose_program_ends_goes_unanswered() {
    let mut desktop = desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut answered = ask(&desktop, Question::Microphone);
    desktop.settle_until("the question's window", |desktop| {
        window_titled(desktop, "Forever").is_some()
    });
    futures::executor::block_on(desktop.runs.stop_all());
    desktop.settle_until("the question's window closes", |desktop| {
        window_titled(desktop, "Forever").is_some_and(|window| window.closes() == 1)
    });
    assert_eq!(answered.as_mut().now_or_never(), Some(None));
}

#[test]
fn a_programs_question_about_a_link_scheme_is_put_as_the_others_are() {
    let mut desktop = desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut answered = desktop
        .prompts
        .ask_to_open_links(&the_program(&desktop), "zoommtg");
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
    let mut desktop = desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let mut answered = desktop
        .prompts
        .ask_for_role(&the_program(&desktop), waos::Role::Inspector);
    desktop.settle_until("the question's window", |desktop| {
        desktop.shows("Allow Forever to see what your other programs do?")
    });
    let window = window_titled(&desktop, "Forever").expect("the question's window");
    desktop.tap(view_of(&window), "Don't allow");
    desktop.settle_until("the answer", |_| true);
    assert_eq!(answered.as_mut().now_or_never(), Some(Some(Answer::Denied)));
    futures::executor::block_on(desktop.runs.stop_all());
}

/// The one program running asks its user `question`, as the engine asks for it.
fn ask(desktop: &LibraryDesktop, question: Question) -> AnswerFuture {
    desktop.prompts.ask(&the_program(desktop), question)
}

#[test]
fn a_waiting_question_is_answered_and_its_panel_closes_when_its_setting_changes() {
    let mut desktop = desktop();
    open_program(
        &mut desktop,
        PERMISSIONS,
        "Permissions",
        waos_test_programs::PERMISSIONS,
    );
    let asked = "Allow Permissions to show notifications?";
    desktop.settle_until("the question", |desktop| desktop.shows(asked));

    let id: ProgramId = PERMISSIONS.parse().unwrap();
    desktop
        .store
        .change_program_settings(&id, |own| {
            own.answers
                .set(Question::Notifications, Setting::Own(Answer::Granted));
        })
        .unwrap();

    desktop.settle_until("the panel closes", |desktop| !desktop.shows(asked));
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn dropping_a_questions_answer_closes_its_prompt_window() {
    let mut desktop = desktop();
    start_console_program(
        &mut desktop,
        FOREVER,
        "Forever",
        waos_test_programs::FOREVER,
    );
    let answered = ask(&desktop, Question::Camera);
    let asked = "Allow Forever to use the camera?";
    desktop.settle_until("the question's window", |desktop| desktop.shows(asked));
    let window = window_titled(&desktop, "Forever").expect("the question's window");
    assert_eq!(window.closes(), 0);

    drop(answered);
    desktop.settle_until("the prompt window closes", |_| window.closes() == 1);
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_picker_of_outputs_answers_the_output_tapped() {
    let mut desktop = desktop();
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
    let mut desktop = desktop();
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
    let mut desktop = desktop();
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

#[test]
fn this_computer_picked_out_of_reach_of_the_phone_s_files_offers_their_page_first() {
    let phone = Arc::new(TestPhone::reachable(false));
    let kind = DesktopKind {
        phone_files: Some(Arc::clone(&phone) as _),
        ..DesktopKind::default()
    };
    let (mut desktop, ()) = LibraryDesktop::of(
        kind,
        Box::new(|_app, _library, _recordings, _views| (None, ())),
    );
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
    for (tapped, page_opened) in [("Not now", false), ("Open settings", true)] {
        let mut answered = desktop
            .machine_picker
            .pick(&the_program(&desktop), vec![this_computer.clone()]);
        desktop.settle_until("the machines to pick from", |desktop| {
            desktop.shows("Pick a machine for Forever")
        });
        let window = window_titled(&desktop, "Forever").expect("the picker's window");
        desktop.tap(view_of(&window), "This computer");
        desktop.settle_until("the phone's files offered", |desktop| {
            desktop.shows("Let WaOS reach this phone's files?")
        });
        assert_eq!(
            answered.as_mut().now_or_never(),
            None,
            "answered before the offer"
        );
        desktop.tap(view_of(&window), tapped);
        desktop.settle_until("the tap", |_| true);
        if page_opened {
            assert_eq!(phone.pages_opened(), 1);
            assert_eq!(
                answered.as_mut().now_or_never(),
                None,
                "answered with the page open"
            );
            desktop.leave_and_come_back();
        }
        let this_computer = Some("this-computer".to_owned());
        assert_eq!(
            answered.as_mut().now_or_never(),
            Some(this_computer),
            "{tapped}"
        );
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
