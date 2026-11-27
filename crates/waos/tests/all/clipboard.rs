//! `examples/clipboard` under the test shell, with folders and switches of the test's own: the
//! program reads what another application copied once its window has the focus, copies text with
//! its HTML promised and its RTF promised but never made whole, and a file of its `/data` beside a
//! picture that exists only once pasted, and answers each paste of a promised form or file, a
//! paste of the RTF failing on its `done`, until another copy replaces its own. Whether it ends on
//! its own, flushing the clipboard as its window is asked to close, or the engine stops it, it is
//! asked for every promised form first, never the file, and what it gives is kept for later
//! pastes. With its switches off, it reads and writes nothing.
//!
//! `examples/copy-paste`, an Waft app over `wasdk-adapter-waft`, copies a line as it starts,
//! given as text and promised as HTML and as a file, each made on paste; it hears its copy as a
//! change once its window has the focus and reads it back, as it reads another application's
//! copy; and as its window is asked to close it flushes the clipboard, so the HTML outlives it
//! while the file ends with it. It is a debug build of Waft, slow to start, and draws,
//! so it runs where the engine makes hardware buffers, on Apple's systems.

use std::fs;
use std::future::pending;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tempfile::TempDir;
#[cfg(target_vendor = "apple")]
use waos::ProgramOutput;
use waos::{
    Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramView, Question, RunningProgram,
    WaOS,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant, ProgramFolders};

use crate::test_shell::clipboard::CopiedText;
use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_program_reads_a_copy_and_answers_the_pastes_of_its_own_until_another_replaces_it() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let folders = Folders::new();
    let output = Arc::new(RecordedOutput::default());
    let (running, window) = run_until_written(&engine, &shell, &folders, &output);

    let options = shell.clipboard.options();
    assert!(options.sensitive && !options.this_device_only);
    assert_eq!(options.expires_after, Some(Duration::from_secs(60)));
    assert_eq!(
        shell.clipboard.written_types(),
        [
            vec!["text/plain", "text/html", "text/rtf"],
            vec!["text/plain", "image/png"]
        ]
    );
    let note =
        waos::machine_paths::plain(fs::canonicalize(folders.data().join("note.txt")).unwrap());
    assert_eq!(shell.clipboard.file(1, "text/plain"), note);

    let html = shell.clipboard.paste(0, "text/html");
    assert_eq!(html.as_deref(), Some(&b"<b>copied text</b>"[..]));
    output.wait_for("asked for: 0 text/html");
    let picture = shell.clipboard.paste_promised_file(1, "image/png");
    assert_eq!(picture.unwrap(), b"PNG");
    output.wait_for("asked for: 1 image/png");

    shell
        .clipboard
        .another_application_copies(&engine, vec![CopiedText::item("another copy")]);
    output.wait_for("replaced");
    window.close_requested();
    output.wait_for("flushed");
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

#[test]
fn a_paste_fails_when_the_programs_done_says_the_form_was_not_made() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let folders = Folders::new();
    let output = Arc::new(RecordedOutput::default());
    let (running, _window) = run_until_written(&engine, &shell, &folders, &output);

    let rtf = shell.clipboard.paste(0, "text/rtf");
    assert_eq!(rtf, None, "a part is never the whole");
    output.wait_for("asked for: 0 text/rtf");
    running.stop();
    within(running.ended());
}

#[test]
fn a_program_that_ends_on_its_own_flushes_first_and_its_forms_outlive_it() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let folders = Folders::new();
    let output = Arc::new(RecordedOutput::default());
    let (running, window) = run_until_written(&engine, &shell, &folders, &output);

    window.close_requested();
    output.wait_for("flushed");
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert!(
        output.has("asked for: 0 text/html"),
        "{:#?}",
        output.lines()
    );
    assert!(output.has("asked for: 0 text/rtf"), "{:#?}", output.lines());
    assert!(
        !output.has("asked for: 1 image/png"),
        "a promised file ends with its program"
    );

    let html = shell.clipboard.paste(0, "text/html");
    assert_eq!(
        html.as_deref(),
        Some(&b"<b>copied text</b>"[..]),
        "kept, though the program has ended"
    );
    assert_eq!(shell.clipboard.paste(0, "text/rtf"), None, "never made");
    let picture = shell.clipboard.paste_promised_file(1, "image/png");
    assert!(picture.is_err(), "the file ended with the program");
}

#[test]
fn a_program_the_engine_stops_is_first_asked_for_the_forms_it_promised() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let folders = Folders::new();
    let output = Arc::new(RecordedOutput::default());
    let (running, _window) = run_until_written(&engine, &shell, &folders, &output);

    running.stop();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Stopped), "{end:?}");
    assert!(
        output.has("asked for: 0 text/html"),
        "{:#?}",
        output.lines()
    );
    assert!(
        !output.has("asked for: 1 image/png"),
        "a promised file ends with its program"
    );

    let html = shell.clipboard.paste(0, "text/html");
    assert_eq!(
        html.as_deref(),
        Some(&b"<b>copied text</b>"[..]),
        "kept, though the program has ended"
    );
}

#[test]
fn a_program_whose_switches_are_off_reads_and_writes_nothing() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let folders = Folders::new();
    let output = Arc::new(RecordedOutput::default());
    let running = start(&engine, &folders, &output, Switches::off());
    output.wait_for("a window");
    shell
        .clipboard
        .another_application_copies(&engine, vec![CopiedText::item("from another app")]);
    output.wait_for("read refused: denied");
    output.wait_for("write refused: denied");
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert!(
        shell.clipboard.written_types().is_empty(),
        "nothing written"
    );
}

#[cfg(target_vendor = "apple")]
#[test]
fn an_waft_app_promises_a_form_and_a_file_and_its_form_outlives_it() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = start_copy_paste(&engine, &output);
    let window = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("copy-paste asks for its window");

    output.wait_for("clipboard changed: copied by copy-paste");
    assert_eq!(
        shell.clipboard.written_types(),
        [vec!["text/plain", "text/html"], vec!["text/plain"]]
    );
    let html = shell.clipboard.paste(0, "text/html");
    assert_eq!(html.as_deref(), Some(&b"<b>copied by copy-paste</b>"[..]));
    let line = shell.clipboard.paste_promised_file(1, "text/plain");
    assert_eq!(line.unwrap(), b"copied by copy-paste");

    window.close_requested();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    let html = shell.clipboard.paste(0, "text/html");
    assert_eq!(
        html.as_deref(),
        Some(&b"<b>copied by copy-paste</b>"[..]),
        "flushed before the app ended"
    );
    let line = shell.clipboard.paste_promised_file(1, "text/plain");
    assert!(line.is_err(), "the file ended with the app");
}

#[cfg(target_vendor = "apple")]
#[test]
fn an_waft_app_hears_another_applications_copy_and_reads_it() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = start_copy_paste(&engine, &output);
    let window = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("copy-paste asks for its window");

    output.wait_for("clipboard changed: copied by copy-paste");
    shell
        .clipboard
        .another_application_copies(&engine, vec![CopiedText::item("another copy")]);
    output.wait_for("clipboard changed: another copy");
    window.close_requested();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

/// Starts the program with its switches on, gives it another application's copy once its window
/// has the focus, and waits until it has read it and written its own; answers the program and its
/// window, as the shell placed it.
fn run_until_written(
    engine: &WaOS,
    shell: &TestShell,
    folders: &Folders,
    output: &Arc<RecordedOutput>,
) -> (RunningProgram, ProgramView) {
    let running = start(engine, folders, output, Switches::on());
    let window = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("the program asks for its window");
    output.wait_for("a window");
    shell
        .clipboard
        .another_application_copies(engine, vec![CopiedText::item("from another app")]);
    output.wait_for("changed: text/plain");
    output.wait_for("read: from another app");
    output.wait_for("written");
    (running, window)
}

/// Starts `examples/copy-paste` with its switches on and this computer's fonts, as an app gives
/// them.
#[cfg(target_vendor = "apple")]
fn start_copy_paste(engine: &WaOS, output: &Arc<RecordedOutput>) -> RunningProgram {
    let program = engine
        .load(
            Path::new(waos_test_programs::COPY_PASTE),
            &waos_test_programs::code_of(waos_test_programs::COPY_PASTE),
        )
        .unwrap();
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(output) as Arc<dyn ProgramOutput>),
        grants: Some(Switches::on() as Arc<dyn ProgramGrants>),
        ..waos_local::LocalCapabilities::new().environment()
    };
    engine.run(&program, environment).unwrap()
}

fn start(
    engine: &WaOS,
    folders: &Folders,
    output: &Arc<RecordedOutput>,
    switches: Arc<Switches>,
) -> RunningProgram {
    let program = engine
        .load(
            Path::new(waos_test_programs::CLIPBOARD),
            &waos_test_programs::code_of(waos_test_programs::CLIPBOARD),
        )
        .unwrap();
    let environment = ProgramEnvironment {
        grants: Some(switches as Arc<dyn ProgramGrants>),
        folders: Some(folders.own()),
        ..printing_to(output)
    };
    engine.run(&program, environment).unwrap()
}

/// The program's own folders, made for the test.
struct Folders {
    root: TempDir,
}

impl Folders {
    fn new() -> Folders {
        let root = TempDir::new().unwrap();
        for folder in ["data", "cache"] {
            fs::create_dir(root.path().join(folder)).unwrap();
        }
        Folders { root }
    }

    fn data(&self) -> std::path::PathBuf {
        self.root.path().join("data")
    }

    fn own(&self) -> ProgramFolders {
        ProgramFolders {
            data: self.data(),
            cache: self.root.path().join("cache"),
        }
    }
}

/// The clipboard switches, on or off; nothing else granted.
struct Switches {
    on: AtomicBool,
}

impl Switches {
    fn on() -> Arc<Switches> {
        Arc::new(Switches {
            on: AtomicBool::new(true),
        })
    }

    fn off() -> Arc<Switches> {
        Arc::new(Switches {
            on: AtomicBool::new(false),
        })
    }
}

impl ProgramGrants for Switches {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Only(Vec::new()),
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    fn clipboard_write(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant { mounts: Vec::new() }
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}
