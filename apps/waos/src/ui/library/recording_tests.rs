//! The programs recording, in the library's two layouts, on a desktop of the tests' own
//! (`state::library::test_desktop`): told by the indicator of who records, and with what, as the
//! engine tells it, a program recording is named with the device it uses in the launcher, in
//! Fluent's layout, and in a window of a phone's size, in Cupertino's, and is gone once it stops;
//! a device of WaOS's own is said simulated.

use std::sync::Arc;

use waft::IntoWidget;
use waos::{RecordingDevice, RunningProgram, ShellRecordingIndicator};
use waos_package::ProgramId;

use super::LaidOutLibrary;
use crate::state::library::test_desktop::{DesktopKind, LibraryDesktop};
use crate::state::phone_window::in_phone_window;
use crate::ui::launcher::tests::{Start, launcher_desktop};

const FOREVER: &str = "dev.example.forever";

/// What the library says while `examples/forever`, installed as Forever, records sound.
const RECORDING: &str = "Forever is using the microphone.";

/// What the library says while Forever records sound and uses the camera.
const RECORDING_BOTH: &str = "Forever is using the microphone. Forever is using the camera.";

/// The desktop with the library in its launcher, shown.
fn in_launcher() -> LibraryDesktop {
    let (desktop, _launcher) = launcher_desktop(DesktopKind::default(), Start::Shown);
    desktop
}

/// The desktop with the library in a window of a phone's size.
fn on_phone() -> LibraryDesktop {
    let (desktop, ()) = LibraryDesktop::new(|app, library, recordings, _view_windows| {
        let page = LaidOutLibrary {
            library: library.clone(),
            recordings: recordings.clone(),
            questions_here: true,
        };
        (Some(in_phone_window(app, page.into_widget())), ())
    });
    desktop
}

/// Installs `examples/forever` as Forever and opens it, answering the program running.
fn open_forever(desktop: &mut LibraryDesktop) -> RunningProgram {
    let component = std::fs::read(waos_test_programs::FOREVER_SHOWN).unwrap();
    let package = desktop.package(FOREVER, "Forever", "1.0.0", &component);
    desktop.update(|library, cx| library.open_files(vec![package], cx));
    desktop.update(|library, cx| library.answer(true, cx));
    let id: ProgramId = FOREVER.parse().unwrap();
    desktop.update(|library, cx| library.open(&id, cx));
    let runs = Arc::clone(&desktop.runs);
    desktop.settle_until("the program runs", move |_| {
        !futures::executor::block_on(runs.programs()).is_empty()
    });
    let programs = futures::executor::block_on(desktop.runs.programs());
    programs.into_iter().next().expect("Forever runs")
}

/// The indicator says `program` records with the microphone, then with the camera too, then that
/// it stopped with each, and the library shows what it uses while it does.
fn shown_while_recording(desktop: &mut LibraryDesktop, program: &RunningProgram) {
    let indicator = &desktop.recording_indicator;
    assert!(!desktop.shows(RECORDING), "nothing records yet");
    indicator.recording_changed(program, RecordingDevice::Microphone, true);
    desktop.settle_until("the recording shown", |desktop| desktop.shows(RECORDING));
    let indicator = &desktop.recording_indicator;
    indicator.recording_changed(program, RecordingDevice::Camera, true);
    desktop.settle_until("the camera shown", |desktop| desktop.shows(RECORDING_BOTH));
    let indicator = &desktop.recording_indicator;
    indicator.recording_changed(program, RecordingDevice::Microphone, false);
    indicator.recording_changed(program, RecordingDevice::Camera, false);
    desktop.settle_until("the recording gone", |desktop| {
        !desktop.shows(RECORDING) && !desktop.shows(RECORDING_BOTH)
    });
}

#[test]
fn a_program_recording_is_named_in_the_launcher() {
    let mut desktop = in_launcher();
    let program = open_forever(&mut desktop);
    shown_while_recording(&mut desktop, &program);
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_program_recording_is_named_on_a_phone() {
    let mut desktop = on_phone();
    let program = open_forever(&mut desktop);
    shown_while_recording(&mut desktop, &program);
    futures::executor::block_on(desktop.runs.stop_all());
}

#[test]
fn a_program_recording_with_simulated_devices_is_said_to_on_a_phone() {
    let mut desktop = on_phone();
    let program = open_forever(&mut desktop);
    let indicator = &desktop.recording_indicator;
    indicator.recording_changed(&program, RecordingDevice::SimulatedCamera, true);
    indicator.recording_changed(&program, RecordingDevice::SimulatedMicrophone, true);
    let said =
        "Forever is using the microphone (simulated). Forever is using the camera (simulated).";
    desktop.settle_until("the simulated devices shown", |desktop| desktop.shows(said));
    assert!(!desktop.shows(RECORDING_BOTH), "not as the machine's");
    futures::executor::block_on(desktop.runs.stop_all());
}
