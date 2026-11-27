//! A Waft app, `builtin-apps/notes` over `wasdk-adapter-waft`, under the test shell, given this
//! computer's fonts as an app gives them: given no view, it asks for its window, which the shell
//! answers with a view at once; it draws a non-blank picture; it serves Waft's service extensions,
//! being a debug build, answering one with its JSON and naming those it serves for one it does
//! not; it draws again at the new size when the view is resized; and closing its one window quits
//! it, its `main` returning.
//!
//! Hardware buffers are made on Apple's systems, where this test runs; on a machine with no fonts
//! installed it says so and passes.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::{ExtensionError, ProgramEnd, ProgramEnvironment, ProgramOutput, WaOS};
use waos_local::LocalCapabilities;

use crate::test_shell::{DISPLAY, RecordedOutput, TestShell, config, metrics, within};

/// The title Notes asks its window to have.
const TITLE: &str = "Notes";

#[test]
fn a_waft_app_draws_its_window_serves_its_extensions_follows_its_resize_and_quits_when_it_closes() {
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
        ..LocalCapabilities::new().environment()
    };
    let installed = environment
        .fonts
        .as_ref()
        .map_or(0, |fonts| fonts.lock().unwrap().face_count());
    if installed == 0 {
        eprintln!("SKIP: no fonts installed on this machine");
        return;
    }

    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let app = engine
        .load(
            Path::new(waos_test_programs::NOTES),
            &waos_test_programs::code_of(waos_test_programs::NOTES),
        )
        .unwrap();
    let running = engine.run(&app, environment).unwrap();

    let shown = within(shell.headless.view(0)).expect("the app asks for its window");
    assert_eq!(shown.window().expect("in a window").config().title, TITLE);
    let view = shown.view().expect("the app's view");
    assert!(within(view.picture_after_events()), "its first picture");
    let picture = view.picture().unwrap().expect("a picture");
    assert_eq!(
        (picture.width, picture.height),
        (DISPLAY.width, DISPLAY.height)
    );
    assert!(
        is_not_blank(&picture),
        "the app drew its UI and text onto the window"
    );

    let extensions = running.service_extensions();
    assert!(extensions.serves(), "a debug build serves its extensions");
    let dumped = within(extensions.call("ext.waft.debugDumpApp", Vec::new()))
        .expect("the app's tree, as JSON");
    let dumped: serde_json::Value = serde_json::from_str(&dumped).expect("JSON text");
    assert!(dumped.to_string().contains("WidgetsBinding"), "{dumped}");
    let unknown = within(extensions.call("ext.waft.nothing", Vec::new()));
    let Err(ExtensionError::Unknown(registered)) = unknown else {
        panic!("an extension it does not serve: {unknown:?}");
    };
    assert!(
        registered
            .iter()
            .any(|name| name == "ext.waft.debugDumpApp"),
        "{registered:?}"
    );

    let (half_width, half_height) = (DISPLAY.width / 2, DISPLAY.height / 2);
    view.metrics_changed(metrics(half_width, half_height));
    assert!(within(view.picture_after_events()), "its resized picture");
    let resized = view.picture().unwrap().expect("a resized picture");
    assert_eq!((resized.width, resized.height), (half_width, half_height));
    assert!(is_not_blank(&resized), "the resized window is not blank");

    view.close_requested();
    let end = within(running.ended());
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "closing its one window quits the app, not {end:?}; it printed {:#?}",
        output.lines()
    );
}

/// Whether `picture` holds more than one pixel colour.
fn is_not_blank(picture: &PixelBuffer) -> bool {
    let first = &picture.bytes[..4];
    (0..picture.height as usize).any(|y| {
        let row = &picture.bytes[y * picture.row_bytes as usize..];
        (0..picture.width as usize).any(|x| &row[x * 4..x * 4 + 4] != first)
    })
}
