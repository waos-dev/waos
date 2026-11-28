//! The questions programs put to their user, on a desktop of the tests' own
//! (`state::library::test_desktop`): `examples/permissions`, which has a window, has its question
//! put over it, and the answer the user taps is kept in its settings; a question of a program with
//! no window, `examples/forever`, a console program started to hear a click on its notification,
//! has a window of its own, whose answer reaches the program and
//! which closes with it; a program's question about opening links of a scheme is put the same
//! way; a question an app with no window asks itself as it starts, for the inspector role it
//! imports, goes unanswered once the app is stopped, its window closing; and the picker of a program's outputs, a
//! question too, answers the output the user taps, or none when they cancel it, as the picker of a
//! machine for it does; and the phone's files, where WaOS may not reach them: This computer
//! picked, or a file or folder picked on the phone's storage, opens the system's page for them,
//! with no question first, and the user is told where they come back with them still off.

use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use waft::ui::{View as _, ViewId};
use waft_test::TestWindow;
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{
    Answer, AnswerFuture, AudioOutput, AudioOutputPickedFuture, MachineChoice, Question,
    RunningProgram, ShellAudioOutputPicker, ShellMachinePicker, ShellSystemPermissions,
    SystemPermission, SystemPermissionState,
};
use waos_package::ProgramId;
use waos_services::permissions::{App, Prompts, Setting};

use crate::logic::phone_files::TestPhone;
use crate::state::library::test_desktop::{DesktopKind, LibraryDesktop};

const PERMISSIONS: &str = "dev.example.permissions";
const FOREVER: &str = "dev.example.forever";
const INSPECTOR: &str = "dev.example.inspector";

/// The desktop with no library window: only the programs' and the questions'.
fn desktop() -> LibraryDesktop {
    let (desktop, ()) =
        LibraryDesktop::new(|_app, _library, _recordings, _view_windows| (None, ()));
    desktop
}

/// Installs the program `id`, named `name`, made of the test program at `component`, and opens
/// it from the library.
fn open_program(desktop: &mut LibraryDesktop, id: &str, name: &str, component: &str) {
    desktop.install_compiled(id, name, component);
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
fn the_program(desktop: &LibraryDesktop) -> RunningProgram {
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
    let (permissions, id) = (
        desktop.permissions.clone(),
        PERMISSIONS.parse::<ProgramId>().unwrap(),
    );
    desktop.settle_until("the answer kept", move |_| {
        let settings = permissions.app_settings(&id).unwrap();
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
fn a_question_whose_app_is_stopped_goes_unanswered() {
    let mut desktop = desktop();
    start_console_program(
        &mut desktop,
        INSPECTOR,
        "Inspector",
        waos_test_programs::IMPORTS_INSPECT,
    );
    desktop.settle_until(
        "the app's own question, in a window of its own",
        |desktop| desktop.shows("Allow Inspector to see what your other programs do?"),
    );
    let window = window_titled(&desktop, "Inspector").expect("the question's window");
    futures::executor::block_on(desktop.runs.stop_all());
    desktop.settle_until("the question's window closes", |_| window.closes() == 1);
    let id: ProgramId = INSPECTOR.parse().unwrap();
    let settings = desktop.permissions.app_settings(&id).unwrap();
    assert_eq!(
        settings.roles.get(&waos::Role::Inspector),
        None,
        "no answer kept"
    );
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
    let mut desktop = desktop();
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

/// The one program running asks its user `question`, as its permissions ask for it.
fn ask(desktop: &LibraryDesktop, question: Question) -> AnswerFuture {
    let app = the_app(desktop, "the one program");
    desktop.prompts.ask(&app, question)
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
        .permissions
        .change_app_settings(&id, |own| {
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

/// A desktop with a phone's files, out of WaOS's reach, and Forever running on it.
fn desktop_with_a_phone() -> (LibraryDesktop, Rc<TestPhone>) {
    let phone = Rc::new(TestPhone::reachable(false));
    let kind = DesktopKind {
        phone_files: Some(Rc::clone(&phone) as _),
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
    (desktop, phone)
}

const PHONE_FILES_OFF: &str = "All files access is off";

/// Taps `label` on the sheet Forever's question is put on.
fn tap_on_the_sheet(desktop: &mut LibraryDesktop, label: &str) {
    let window = window_titled(desktop, "Forever").expect("the question's window");
    desktop.tap(view_of(&window), label);
    desktop.settle_until("the tap", |_| true);
}

#[test]
fn this_computer_picked_out_of_reach_of_the_phone_s_files_opens_their_page_first() {
    let (mut desktop, phone) = desktop_with_a_phone();
    let this_computer = MachineChoice {
        id: "this-computer".to_owned(),
        name: "This computer".to_owned(),
    };
    for (turned_on, pages_opened) in [(false, 1), (true, 2)] {
        let mut answered = desktop
            .machine_picker
            .pick(&the_program(&desktop), vec![this_computer.clone()]);
        desktop.settle_until("the machines to pick from", |desktop| {
            desktop.shows("Pick a machine for Forever")
        });
        tap_on_the_sheet(&mut desktop, "This computer");
        assert_eq!(
            answered.as_mut().now_or_never(),
            None,
            "answered before the phone's files were asked for"
        );
        desktop.settle_until("the system's page opened", |_| {
            phone.pages_opened() == pages_opened
        });
        assert_eq!(
            answered.as_mut().now_or_never(),
            None,
            "answered with the page open"
        );
        phone.reachable.set(turned_on);
        desktop.leave_and_come_back();
        if !turned_on {
            desktop.settle_until("still off, said", |desktop| desktop.shows(PHONE_FILES_OFF));
            assert_eq!(answered.as_mut().now_or_never(), None, "before it was read");
            tap_on_the_sheet(&mut desktop, "OK");
        }
        let this_computer = Some("this-computer".to_owned());
        assert_eq!(
            answered.as_mut().now_or_never(),
            Some(this_computer),
            "This computer is picked with or without the phone's files, and with nothing to \
             read once they are on"
        );
        phone.reachable.set(false);
        desktop.leave_and_come_back();
    }
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_pick_on_the_phone_s_storage_is_denied_by_the_system_until_the_user_turns_its_files_on() {
    let (mut desktop, phone) = desktop_with_a_phone();
    let system = desktop
        .phone_files_on_request
        .clone()
        .expect("a phone's files");
    let program = the_program(&desktop);
    let music = SystemPermission::Path(std::path::Path::new("/storage/emulated/0/Music"));
    let own = SystemPermission::Path(std::path::Path::new(TestPhone::OWN_FOLDER));
    assert_eq!(system.state(music, &program), SystemPermissionState::Denied);
    assert_eq!(
        system.state(own, &program),
        SystemPermissionState::Allowed,
        "WaOS's own folder is its own"
    );

    let mut asked = ShellSystemPermissions::request(&system, music, &program);
    desktop.settle_until("the system's page opened", |_| phone.pages_opened() == 1);
    desktop.leave_and_come_back();
    desktop.settle_until("still off, said", |desktop| desktop.shows(PHONE_FILES_OFF));
    assert_eq!(asked.as_mut().now_or_never(), None, "before it was read");
    tap_on_the_sheet(&mut desktop, "OK");
    assert_eq!(
        asked.as_mut().now_or_never(),
        Some(SystemPermissionState::Denied),
        "left off"
    );

    let mut asked = ShellSystemPermissions::request(&system, music, &program);
    desktop.settle_until("the system's page opened again", |_| {
        phone.pages_opened() == 2
    });
    phone.reachable.set(true);
    desktop.leave_and_come_back();
    assert_eq!(
        asked.as_mut().now_or_never(),
        Some(SystemPermissionState::Allowed),
        "turned on in the system's settings: answered with nothing more to read"
    );
    assert_eq!(
        system.state(music, &program),
        SystemPermissionState::Allowed
    );
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
