//! `examples/opener` under the test shell, with grants of the test's own that keep the user's
//! answers to link schemes and the files picked, as an app keeps them: a web link opens without
//! asking; a link of another scheme asks its user once, opens as the user allowed and is refused
//! with `denied` as the user refused, without asking again; a link no app opens fails with
//! `no-app`, and text that is no link fails; a file opens, and is shown, where it is on the
//! machine, and a path outside the program's files fails without reaching the shell. With no
//! grants, a link of a scheme of its own is refused without asking; and a shell without an opener
//! gives the program none.
//!
//! What the user opens with the program reaches it: a request made before it listens waits until
//! it does, each file mounted writable at its own path and kept among its picks; and a request
//! thaws a frozen program until it has read it, on time the test moves (`EngineTime::Manual`),
//! the program frozen again once it reads on (`RunningProgram::frozen`).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;
use waos::{
    Answer, FreezePolicy, ManualTime, OpenRequest, ProgramEnd, ProgramEnvironment, ProgramGrants,
    ProgramOutput, Question, RunningProgram, WaOS,
};
use waos_capabilities::{
    ChangedFuture, DomainRule, FileSystemGrant, Mount, NetworkGrant, ProgramFolders,
};

use crate::shell::opener::Opened;
use crate::shell::{self, Lines, TestShell, resolved, wait_until, wait_until_frozen};

/// The engine's tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

#[test]
fn links_open_as_their_schemes_allow_and_files_where_they_are_on_the_machine() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.opener().without_app_for("nothing-opens:this");
    let scratch = tempfile::tempdir().unwrap();
    let own = own_folders(scratch.path());
    let grants = Arc::new(LinkGrants::default());
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        folders: Some(own.clone()),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(&engine, waos_test_programs::OPENER);
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();

    lines.wait_for("open https://example.com/: ok");
    answer_scheme(&shell, 0, "zoommtg", Answer::Granted);
    lines.wait_for("open zoommtg://zoom.us/join: ok");
    lines.wait_for("open zoommtg://zoom.us/again: ok");
    answer_scheme(&shell, 1, "tel", Answer::Denied);
    lines.wait_for("open tel:+15550100: denied");
    lines.wait_for("open tel:+15550101: denied");
    answer_scheme(&shell, 2, "nothing-opens", Answer::Granted);
    lines.wait_for("open nothing-opens:this: no app");
    lines.wait_for("open not a link: failed (\"not a link\" is not a link)");
    lines.wait_for("open file /data/note.txt: ok");
    lines.wait_for("reveal file /data/note.txt: ok");
    lines.wait_for(
        "open file /data/missing.txt: failed (no file at /data/missing.txt among the program's)",
    );
    lines.wait_for("listening for open requests");
    running.stop();
    assert!(matches!(shell::wait_for_end(&running), ProgramEnd::Stopped));

    let asked: Vec<String> = shell
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
        shell.opener().opened(),
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
    let (engine, shell, _arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(&engine, waos_test_programs::OPENER);
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    lines.wait_for("listening for open requests");
    running.stop();
    shell::wait_for_end(&running);
    for line in [
        "open https://example.com/: ok",
        "open zoommtg://zoom.us/join: denied",
        "open tel:+15550100: denied",
    ] {
        assert!(lines.has(line), "no {line:?} in {:#?}", lines.all());
    }
    assert!(shell.link_schemes_asked().is_empty(), "nobody asked");
}

#[test]
fn a_shell_without_an_opener_gives_the_program_none() {
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.opener = None;
    });
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(&engine, waos_test_programs::OPENER);
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    lines.wait_for("listening for open requests");
    running.stop();
    shell::wait_for_end(&running);
    assert_eq!(lines.all(), ["opener: none", "listening for open requests"]);
}

#[test]
fn a_request_made_before_the_program_listens_is_heard_once_it_does_its_files_mounted_writable() {
    let (engine, shell, _arrivals) = shell::engine();
    let scratch = tempfile::tempdir().unwrap();
    let grants = Arc::new(LinkGrants::default());
    let lines = Arc::new(Lines::default());
    let (running, _own) = run(&engine, scratch.path(), &grants, &lines);
    let notes = fs::canonicalize(scratch.path()).unwrap().join("notes.md");
    fs::write(&notes, "my notes").unwrap();

    wait_until("the program asks about zoommtg, not listening yet", || {
        !shell.link_schemes_asked().is_empty()
    });
    running.open_requested(OpenRequest {
        files: vec![notes.clone()],
        links: vec!["notes://today".to_owned()],
    });
    answer_every_scheme(&shell);
    lines.wait_for("listening for open requests");
    let named = waos::machine_paths::in_program(&notes).unwrap();
    let shown = named.display();
    lines.wait_for(&format!("opened file {shown}: my notes"));
    lines.wait_for(&format!("opened file {shown} written to: yes"));
    lines.wait_for("opened link: notes://today");
    assert_eq!(fs::read_to_string(&notes).unwrap(), "my notes, seen");
    assert_eq!(
        grants.picked(),
        [Mount {
            host: notes.clone(),
            at: named,
            writable: true,
        }],
        "kept as a pick is, where the program names it"
    );
    running.stop();
    shell::wait_for_end(&running);
}

#[test]
fn an_open_request_thaws_a_frozen_program_until_it_has_read_it() {
    let time = ManualTime::new();
    let only_when_suspended = FreezePolicy { after_hidden: None };
    let (engine, _shell, _arrivals) = shell::engine_on(&time, only_when_suspended);
    let scratch = tempfile::tempdir().unwrap();
    let grants = Arc::new(LinkGrants::answered());
    let lines = Arc::new(Lines::default());
    let (running, _own) = run(&engine, scratch.path(), &grants, &lines);
    lines.wait_for("listening for open requests");

    resolved(engine.app_suspending());
    wait_until_frozen(&running);
    // A tick past the moment it last ran: woken without a thaw, it would stop at its next epoch
    // check, before it could say what it heard.
    time.elapse(TICK);
    running.open_requested(OpenRequest {
        files: Vec::new(),
        links: vec!["notes://while-frozen".to_owned()],
    });
    lines.wait_for("opened link: notes://while-frozen");
    wait_until_frozen(&running);

    engine.app_resumed();
    running.stop();
    shell::wait_for_end(&running);
}

#[test]
fn a_waiting_link_scheme_question_is_answered_by_setting_its_row_and_drops_its_prompt() {
    let (engine, shell, _arrivals) = shell::engine();
    let scratch = tempfile::tempdir().unwrap();
    let grants = Arc::new(LinkGrants::default());
    let lines = Arc::new(Lines::default());
    let (running, _own) = run(&engine, scratch.path(), &grants, &lines);

    wait_until("the program asks about zoommtg", || {
        !shell.link_schemes_asked().is_empty()
    });
    assert!(!shell.link_scheme_cancelled(0));

    grants.set_link_scheme_answer("tel", Answer::Denied);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!shell.link_scheme_cancelled(0));

    grants.set_link_scheme_answer("zoommtg", Answer::Granted);
    wait_until("the link scheme prompt future is dropped", || {
        shell.link_scheme_cancelled(0)
    });
    lines.wait_for("open zoommtg://zoom.us/join: ok");
    running.stop();
    shell::wait_for_end(&running);
}

/// Runs `examples/opener` with its own folders beneath `scratch`, `grants` and its output into
/// `lines`.
fn run(
    engine: &WaOS,
    scratch: &Path,
    grants: &Arc<LinkGrants>,
    lines: &Arc<Lines>,
) -> (RunningProgram, ProgramFolders) {
    let own = own_folders(scratch);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(lines) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::clone(grants) as Arc<dyn ProgramGrants>),
        folders: Some(own.clone()),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(engine, waos_test_programs::OPENER);
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
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
    wait_until(&format!("{scheme} is asked about"), || {
        shell.link_schemes_asked().len() > before
    });
    let asked = shell.link_schemes_asked();
    assert_eq!(asked[before], ("opener".to_owned(), scheme.to_owned()));
    shell.answer_link_schemes(Some(answer));
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

/// Grants that give nothing and keep the user's answers to link schemes and the files picked, as
/// an app keeps them.
#[derive(Default)]
struct LinkGrants {
    schemes: Mutex<HashMap<String, Answer>>,
    picked: Mutex<Vec<Mount>>,
    changed: Arc<Notify>,
}

impl LinkGrants {
    /// Grants with every scheme of [`SCHEMES`] answered already.
    fn answered() -> LinkGrants {
        let grants = LinkGrants::default();
        for (scheme, answer) in SCHEMES {
            grants.set_link_scheme_answer(scheme, answer);
        }
        grants
    }

    /// Each file picked, as it was kept.
    fn picked(&self) -> Vec<Mount> {
        self.picked.lock().unwrap().clone()
    }
}

impl ProgramGrants for LinkGrants {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Any,
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, scheme: &str) -> Option<Answer> {
        self.schemes.lock().unwrap().get(scheme).copied()
    }

    fn set_link_scheme_answer(&self, scheme: &str, answer: Answer) {
        self.schemes
            .lock()
            .unwrap()
            .insert(scheme.to_owned(), answer);
        self.changed.notify_waiters();
    }

    fn keep_picked(&self, mount: Mount) {
        self.picked.lock().unwrap().push(mount);
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(Arc::clone(&self.changed).notified_owned())
    }
}
