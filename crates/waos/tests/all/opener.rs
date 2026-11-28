//! `examples/opener` under the test shell, with the permissions WaOS hands an app, which keep the
//! user's answers to link schemes and the files picked: a web link opens without
//! asking; a link of another scheme asks its user once, opens as the user allowed and is refused
//! with `denied` as the user refused, without asking again; a link no app opens fails with
//! `no-app`, and text that is no link fails; a file opens, and is shown, where it is on the
//! machine, and a path outside the program's files fails without reaching the shell. With no
//! grants, a link of a scheme of its own is refused without asking; and a shell without an opener
//! gives the program none.
//!
//! What the user opens with the program reaches it: a request made before it listens waits until
//! it does, each file mounted writable at its own path and kept among its picks; and a request
//! unfreezes a frozen program until it has read it, on time the test moves (`EngineTime::Manual`),
//! the program frozen again once it reads on (`RunningProgram::frozen`).

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use waos::{
    Answer, EngineTime, FreezePolicy, ManualTime, OpenRequest, ProgramEnd, ProgramEnvironment,
    RunningProgram, WaOS, WaOSConfig,
};
use waos_capabilities::{Mount, ProgramFolders, Setting};
use waos_services::permissions::{Permissions, ProgramId};

use crate::test_shell::opener::Opened;
use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

/// The engine's tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

#[test]
fn links_open_as_their_schemes_allow_and_files_where_they_are_on_the_machine() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.opener.without_app_for("nothing-opens:this");
    let scratch = tempfile::tempdir().unwrap();
    let own = own_folders(scratch.path());
    let permissions = shell.permissions();
    let grants = permissions.of(&app());
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(&grants)),
        folders: Some(own.clone()),
        ..printing_to(&output)
    };
    let program = engine
        .load(
            Path::new(waos_test_programs::OPENER),
            &waos_test_programs::code_of(waos_test_programs::OPENER),
        )
        .unwrap();
    let running = engine.run(&program, environment).unwrap();

    output.wait_for("open https://example.com/: ok");
    answer_scheme(&shell, 0, "zoommtg", Answer::Granted);
    output.wait_for("open zoommtg://zoom.us/join: ok");
    output.wait_for("open zoommtg://zoom.us/again: ok");
    answer_scheme(&shell, 1, "tel", Answer::Denied);
    output.wait_for("open tel:+15550100: denied");
    output.wait_for("open tel:+15550101: denied");
    answer_scheme(&shell, 2, "nothing-opens", Answer::Granted);
    output.wait_for("open nothing-opens:this: no app");
    output.wait_for("open not a link: failed (\"not a link\" is not a link)");
    output.wait_for("open file /data/note.txt: ok");
    output.wait_for("reveal file /data/note.txt: ok");
    output.wait_for(
        "open file /data/missing.txt: failed (no file at /data/missing.txt among the program's)",
    );
    output.wait_for("listening for open requests");
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));

    let asked: Vec<String> = shell
        .prompts
        .link_schemes_asked()
        .into_iter()
        .map(|(_program, scheme)| scheme)
        .collect();
    assert_eq!(asked, ["zoommtg", "tel", "nothing-opens"], "each once");
    assert_eq!(grants.link_scheme_answer("zoommtg"), Some(Answer::Granted));
    assert_eq!(grants.link_scheme_answer("tel"), Some(Answer::Denied));
    let note = waos::machine_paths::plain(fs::canonicalize(&own.data).unwrap()).join("note.txt");
    let link = |link: &str| Opened::Link {
        program: "opener".into(),
        link: link.into(),
    };
    assert_eq!(
        shell.opener.opened(),
        [
            link("https://example.com/"),
            link("zoommtg://zoom.us/join"),
            link("zoommtg://zoom.us/again"),
            link("nothing-opens:this"),
            Opened::File {
                program: "opener".into(),
                path: note.clone(),
            },
            Opened::Revealed {
                program: "opener".into(),
                path: note,
            },
        ],
        "nothing refused reaches the shell"
    );
}

#[test]
fn a_program_given_no_grants_is_refused_links_of_its_own_schemes_without_asking() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = printing_to(&output);
    let program = engine
        .load(
            Path::new(waos_test_programs::OPENER),
            &waos_test_programs::code_of(waos_test_programs::OPENER),
        )
        .unwrap();
    let running = engine.run(&program, environment).unwrap();
    output.wait_for("listening for open requests");
    running.stop();
    within(running.ended());
    for line in [
        "open https://example.com/: ok",
        "open zoommtg://zoom.us/join: denied",
        "open tel:+15550100: denied",
    ] {
        assert!(output.has(line), "no {line:?} in {:#?}", output.lines());
    }
    assert!(
        shell.prompts.link_schemes_asked().is_empty(),
        "nobody asked"
    );
}

#[test]
fn a_shell_without_an_opener_gives_the_program_none() {
    let shell = TestShell::new();
    let mut parts = shell.shell();
    parts.opener = None;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = printing_to(&output);
    let program = engine
        .load(
            Path::new(waos_test_programs::OPENER),
            &waos_test_programs::code_of(waos_test_programs::OPENER),
        )
        .unwrap();
    let running = engine.run(&program, environment).unwrap();
    output.wait_for("listening for open requests");
    running.stop();
    within(running.ended());
    assert_eq!(
        output.lines(),
        ["opener: none", "listening for open requests"]
    );
}

#[test]
fn a_request_made_before_the_program_listens_is_heard_once_it_does_its_files_mounted_writable() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let permissions = shell.permissions();
    let output = Arc::new(RecordedOutput::default());
    let (running, _own) = run(&engine, scratch.path(), &permissions, &output);
    let notes = fs::canonicalize(scratch.path()).unwrap().join("notes.md");
    fs::write(&notes, "my notes").unwrap();

    shell
        .changes
        .wait_until("the program asks about zoommtg, not listening yet", || {
            !shell.prompts.link_schemes_asked().is_empty()
        });
    running.open_requested(OpenRequest {
        files: vec![notes.clone()],
        links: vec!["notes://today".to_owned()],
    });
    answer_every_scheme(&shell);
    output.wait_for("listening for open requests");
    let named = waos::machine_paths::in_program(&notes).unwrap();
    let shown = named.display();
    output.wait_for(&format!("opened file {shown}: my notes"));
    output.wait_for(&format!("opened file {shown} written to: yes"));
    output.wait_for("opened link: notes://today");
    assert_eq!(fs::read_to_string(&notes).unwrap(), "my notes, seen");
    let own = permissions.app_settings(&app()).unwrap().file_system;
    let Setting::Own(own) = own else {
        panic!("the pick is kept among its own mounts: {own:?}");
    };
    assert_eq!(
        own.mounts,
        [Mount {
            host: notes.clone(),
            at: named,
            writable: true,
        }],
        "kept as a pick is, where the program names it"
    );
    running.stop();
    within(running.ended());
}

#[test]
fn an_open_request_unfreezes_a_frozen_program_until_it_has_read_it() {
    let time = ManualTime::new();
    let only_when_suspended = FreezePolicy::default();
    let shell = TestShell::new();
    let config = WaOSConfig {
        time: EngineTime::Manual(time.clone()),
        freeze: only_when_suspended,
        ..config()
    };
    let engine = WaOS::new(config, shell.shell(), shell.platform()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let permissions = shell.permissions();
    let answered = permissions.change_app_settings(&app(), |own| {
        for (scheme, answer) in SCHEMES {
            own.link_schemes.insert(scheme.to_owned(), answer);
        }
    });
    answered.unwrap();
    let output = Arc::new(RecordedOutput::default());
    let (running, _own) = run(&engine, scratch.path(), &permissions, &output);
    output.wait_for("listening for open requests");

    within(engine.suspend());
    within(running.frozen());
    // A tick past the moment it last ran: woken without a unfreeze, it would stop at its next epoch
    // check, before it could say what it heard.
    time.elapse(TICK);
    running.open_requested(OpenRequest {
        files: Vec::new(),
        links: vec!["notes://while-frozen".to_owned()],
    });
    output.wait_for("opened link: notes://while-frozen");
    within(running.frozen());

    engine.resume();
    running.stop();
    within(running.ended());
}

/// Runs `examples/opener` with its own folders beneath `scratch`, `permissions` and its output
/// into `output`.
fn run(
    engine: &WaOS,
    scratch: &Path,
    permissions: &Permissions,
    output: &Arc<RecordedOutput>,
) -> (RunningProgram, ProgramFolders) {
    let own = own_folders(scratch);
    let environment = ProgramEnvironment {
        grants: Some(permissions.of(&app())),
        folders: Some(own.clone()),
        ..printing_to(output)
    };
    let program = engine
        .load(
            Path::new(waos_test_programs::OPENER),
            &waos_test_programs::code_of(waos_test_programs::OPENER),
        )
        .unwrap();
    let running = engine.run(&program, environment).unwrap();
    (running, own)
}

/// Answers the schemes the program asks about, as [`SCHEMES`] says, in order.
fn answer_every_scheme(shell: &TestShell) {
    for (before, (scheme, answer)) in SCHEMES.into_iter().enumerate() {
        answer_scheme(shell, before, scheme, answer);
    }
}

/// Waits until the program asks its user about links of `scheme`, the scheme it asks about after
/// `before` others, and answers `answer`.
fn answer_scheme(shell: &TestShell, before: usize, scheme: &str, answer: Answer) {
    shell
        .changes
        .wait_until(&format!("{scheme} is asked about"), || {
            shell.prompts.link_schemes_asked().len() > before
        });
    let asked = shell.prompts.link_schemes_asked();
    assert_eq!(asked[before], (app().to_string(), scheme.to_owned()));
    shell.prompts.answer_link_schemes(Some(answer));
}

/// The program's own `/data` and `/cache`, made beneath `scratch`.
fn own_folders(scratch: &Path) -> ProgramFolders {
    let own = ProgramFolders {
        data: scratch.join("data"),
        cache: scratch.join("cache"),
    };
    fs::create_dir_all(&own.data).unwrap();
    fs::create_dir_all(&own.cache).unwrap();
    own
}

/// The schemes of the program's links that ask, in order, and the user's answers to them.
const SCHEMES: [(&str, Answer); 3] = [
    ("zoommtg", Answer::Granted),
    ("tel", Answer::Denied),
    ("nothing-opens", Answer::Granted),
];

/// The id the program runs as, which its permissions are kept by.
fn app() -> ProgramId {
    "dev.waos.test.opener".parse().unwrap()
}
