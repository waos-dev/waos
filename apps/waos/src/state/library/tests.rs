//! The library over a store of the test's own, with packages written to the test's folder and
//! sources served by an HTTP server on this machine: a package opened with WaOS is installed once
//! the user says yes, the system's request at launch waiting for WaOS to take it, and a program
//! removed once they say yes; a source's listing offers to install a program, then nothing, then
//! to update it once a newer version is listed; a source that cannot be read says why; an
//! installed program opens in WaOS, once however often it is opened; an installed program is
//! compiling until its code is ready, an open meanwhile waits for that compile, an open where the
//! cache holds nothing current compiles, a removed program leaves no code, and an update leaves
//! the new version's alone while a run of the old one goes on; and files opened
//! with an installed program as WaOS starts open it and reach it, while files opened with one not
//! installed are the library's problem. A package handed without a path, through a handle or a
//! stream of the test's own, is copied and asked about, its copy removed once answered; one that
//! cannot be read, is no package, or is named as no file is, is the library's problem.

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use waft::{AppCell, Subscription};
use waos_package::SigningKey;
use waos_store::SourceProxy;

use super::test_server::Server;
use super::*;
use crate::logic::environments::ProgramEnvironments;
use crate::logic::installed_code::HeldCompiles;
use crate::logic::opened_package::tests::write_package;
use crate::logic::package_copies::PackageCopies;
use crate::logic::program_places::{ProgramPlace, ProgramPlaces};
use crate::logic::program_shortcuts::{
    KeptShortcut, ProgramShortcut, ShortcutOpens, SystemShortcuts,
};
use crate::logic::runs::Runs;
use crate::logic::test_engine::{self, NoWindows};

/// How long a test waits for the library's work.
const WITHIN: Duration = Duration::from_secs(60);

const TRIANGLE: &str = "dev.example.triangle";
const SURVEY: &str = "dev.example.survey";

/// The library, on an app of an inert platform whose tasks the test runs, over a store and an
/// engine in a folder of the test's own.
struct TestLibrary {
    cell: Rc<AppCell>,
    library: Entity<Library>,
    runs: Arc<Runs>,
    folder: TempDir,
    key: SigningKey,
    /// How many times the library asked to be seen.
    asked_to_be_seen: Rc<Mutex<usize>>,
    _seen: Subscription,
}

impl TestLibrary {
    fn new() -> TestLibrary {
        TestLibrary::opening(ProgramsOpen::InWindows, |_| None)
    }

    /// The library, opening programs as `programs_open` says, with the preferences file
    /// `preferences` names in the test's folder, if any.
    fn opening(
        programs_open: ProgramsOpen,
        preferences: impl FnOnce(&std::path::Path) -> Option<PreferencesFile>,
    ) -> TestLibrary {
        let folder = tempfile::tempdir().unwrap();
        let preferences = preferences(folder.path());
        let copies = PackageCopies::clear(folder.path().join("package-copies")).unwrap();
        let runs = Arc::new(Runs::default());
        let store = Store::open(&folder.path().join("store"), SourceProxy::None).unwrap();
        let installed_runs = InstalledRuns::new(
            Arc::new(test_engine::engine(
                Arc::new(NoWindows),
                &folder.path().join("code"),
            )),
            ProgramEnvironments::new(waos_local::LocalCapabilities::new(), store.clone()),
            Arc::clone(&runs),
            ProgramPlaces::new(ProgramPlace::Windows),
        );
        let cell = AppCell::new();
        let library = open_library(
            &mut cell.borrow_mut(),
            store,
            installed_runs,
            copies,
            programs_open,
            preferences,
        )
        .unwrap();
        let asked_to_be_seen = Rc::new(Mutex::new(0));
        let counted = Rc::clone(&asked_to_be_seen);
        let seen = cell
            .borrow_mut()
            .subscribe(&library, move |_, _: &WantsToBeSeen| {
                *counted.lock().unwrap() += 1;
            });
        cell.checkpoint();
        TestLibrary {
            cell,
            library,
            runs,
            folder,
            key: SigningKey::generate(),
            asked_to_be_seen,
            _seen: seen,
        }
    }

    /// A package of `id` at `version`, made of `component`, signed with the test's key and
    /// written to the test's folder.
    fn package(&self, id: &str, version: &str, component: &[u8]) -> PathBuf {
        let packages = self.folder.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        write_package(&packages, id, "Triangle", version, component, &self.key)
    }

    fn update(&self, f: impl FnOnce(&mut Library, &mut Context<Library>)) {
        self.library.update(&mut self.cell.borrow_mut(), f);
        self.cell.checkpoint();
    }

    fn read<R>(&self, f: impl FnOnce(&Library) -> R) -> R {
        f(self.library.read(&self.cell.borrow()))
    }

    /// Runs the app's tasks until `done`, failing after [`WITHIN`].
    fn wait_until(&self, what: &str, done: impl Fn(&Library) -> bool) {
        let deadline = Instant::now() + WITHIN;
        while !self.read(&done) {
            assert!(Instant::now() < deadline, "{what} never happened");
            thread::sleep(Duration::from_millis(5));
            self.cell.checkpoint();
        }
    }

    /// The ids and versions installed, by id.
    fn installed(&self) -> Vec<(String, String)> {
        self.read(|library| {
            library
                .installed()
                .iter()
                .map(|program| {
                    (
                        program.manifest.id().to_string(),
                        program.manifest.version().to_string(),
                    )
                })
                .collect()
        })
    }

    /// What the one source added offers for each program it lists, once it has been read.
    fn offers(&self) -> Vec<(String, Offer)> {
        self.read(|library| match &library.sources()[0].listing {
            Listing::Read(offers) => offers
                .iter()
                .map(|(program, offer)| (program.id.to_string(), offer.clone()))
                .collect(),
            _ => Vec::new(),
        })
    }

    fn installs_after_yes(&self, package: PathBuf) {
        self.update(|library, cx| library.open_files(vec![package], cx));
        self.update(|library, cx| library.answer(true, cx));
    }

    fn asked_to_be_seen(&self) -> usize {
        *self.asked_to_be_seen.lock().unwrap()
    }

    /// Installs forever, as triangle, from a package opened with WaOS.
    fn installs_forever(&self) {
        let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
        self.installs_after_yes(self.package(TRIANGLE, "1.0.0", &forever));
    }

    /// Holds the library's compiles back until the answer is dropped.
    fn hold_compiles(&self) -> HeldCompiles {
        self.read(|library| library.runs.code.hold())
    }

    /// The files of code the engine compiled into the test's code cache that a load may map. On
    /// Windows the cache keeps a file a running program maps after it is removed, dated at the
    /// epoch so it is never mapped again, until nothing maps it.
    fn compiled_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.folder.path().join("code")) else {
            return Vec::new();
        };
        entries
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "cwasm")
            })
            .filter(|path| {
                std::fs::metadata(path).unwrap().modified().unwrap() != std::time::UNIX_EPOCH
            })
            .collect()
    }
}

/// What the library asks now, in words a test compares.
fn question(library: &Library) -> Option<String> {
    library.question().map(|question| match question {
        Question::Install(package) => format!("install {}", package.manifest().id()),
        Question::Remove { id, name } => format!("remove {id} ({name})"),
    })
}

#[test]
fn a_package_opened_with_waos_is_installed_once_the_user_says_yes() {
    let library = TestLibrary::new();
    let package = library.package(TRIANGLE, "1.0.0", b"\0asm");
    library.update(|library, cx| library.open_files(vec![package], cx));
    assert_eq!(
        library.read(question).as_deref(),
        Some("install dev.example.triangle")
    );
    assert_eq!(library.asked_to_be_seen(), 1);
    assert!(library.installed().is_empty(), "not before the answer");
    library.update(|library, cx| library.answer(true, cx));
    assert_eq!(
        library.installed(),
        [(TRIANGLE.to_owned(), "1.0.0".to_owned())]
    );
    assert_eq!(library.read(question), None);
    assert_eq!(
        library.read(|library| library.problem().map(str::to_owned)),
        None
    );
}

#[test]
fn packages_the_system_opens_with_waos_at_launch_and_later_are_asked_about() {
    let library = TestLibrary::new();
    let triangle = library.package(TRIANGLE, "1.0.0", b"\0asm");
    let survey = library.package(SURVEY, "2.0.0", b"\0asm");
    let opened = |package: PathBuf| waft::ui::OpenRequest {
        files: vec![waft::ui::PickedFile::Path(package)],
        links: vec!["https://example.com/".to_owned()],
    };
    library.cell.borrow_mut().open_requested(opened(triangle));
    take_files_opened_with_waos(&mut library.cell.borrow_mut(), &library.library, None);
    library.cell.checkpoint();
    assert_eq!(
        library.read(question).as_deref(),
        Some("install dev.example.triangle"),
        "the launch's request waited for WaOS"
    );
    library.cell.borrow_mut().open_requested(opened(survey));
    library.cell.checkpoint();
    library.update(|library, cx| library.answer(false, cx));
    assert_eq!(
        library.read(question).as_deref(),
        Some("install dev.example.survey")
    );
    assert_eq!(library.asked_to_be_seen(), 2);
}

#[test]
fn a_package_the_user_declines_is_not_installed() {
    let library = TestLibrary::new();
    let package = library.package(TRIANGLE, "1.0.0", b"\0asm");
    library.update(|library, cx| library.open_files(vec![package], cx));
    library.update(|library, cx| library.answer(false, cx));
    assert!(library.installed().is_empty());
    assert_eq!(library.read(question), None);
}

#[test]
fn packages_opened_together_are_asked_about_one_at_a_time() {
    let library = TestLibrary::new();
    let triangle = library.package(TRIANGLE, "1.0.0", b"\0asm");
    let survey = library.package(SURVEY, "2.0.0", b"\0asm");
    library.update(|library, cx| library.open_files(vec![triangle, survey], cx));
    assert_eq!(
        library.read(question).as_deref(),
        Some("install dev.example.triangle")
    );
    library.update(|library, cx| library.answer(false, cx));
    assert_eq!(
        library.read(question).as_deref(),
        Some("install dev.example.survey")
    );
    library.update(|library, cx| library.answer(true, cx));
    assert_eq!(
        library.installed(),
        [(SURVEY.to_owned(), "2.0.0".to_owned())]
    );
}

#[test]
fn a_file_that_cannot_be_installed_is_the_librarys_problem() {
    let library = TestLibrary::new();
    let notes = library.folder.path().join("notes.txt");
    std::fs::write(&notes, "hello").unwrap();
    library.update(|library, cx| library.open_files(vec![notes], cx));
    let problem = library.read(|library| library.problem().map(str::to_owned));
    assert!(problem.is_some_and(|problem| problem.contains(".wapk")));
    assert_eq!(library.read(question), None);
    assert_eq!(library.asked_to_be_seen(), 1, "to say why");
    library.update(|library, cx| library.dismiss_problem(cx));
    assert_eq!(
        library.read(|library| library.problem().map(str::to_owned)),
        None
    );
}

#[test]
fn a_program_is_removed_once_the_user_says_yes() {
    let library = TestLibrary::new();
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.update(|library, cx| library.ask_to_remove(&triangle, cx));
    assert_eq!(
        library.read(question).as_deref(),
        Some("remove dev.example.triangle (Triangle)")
    );
    library.update(|library, cx| library.answer(false, cx));
    assert_eq!(library.installed().len(), 1, "kept after no");
    library.update(|library, cx| library.ask_to_remove(&triangle, cx));
    library.update(|library, cx| library.answer(true, cx));
    assert!(library.installed().is_empty());
}

#[test]
fn a_sources_listing_offers_to_install_then_nothing_then_to_update() {
    let library = TestLibrary::new();
    let server = Server::start();
    server.publish(
        TRIANGLE,
        &library.package(TRIANGLE, "1.0.0", b"\0asm"),
        "1.0.0",
    );
    let index = server.url("waos/index.json");
    library.update(|library, cx| library.add_source(&index, cx));
    library.wait_until("the source's listing", |library| {
        matches!(library.sources()[0].listing, Listing::Read(_))
    });
    assert_eq!(library.offers(), [(TRIANGLE.to_owned(), Offer::Install)]);

    install_what_the_source_lists(&library);
    assert_eq!(
        library.installed(),
        [(TRIANGLE.to_owned(), "1.0.0".to_owned())]
    );
    assert_eq!(library.offers(), [(TRIANGLE.to_owned(), Offer::Installed)]);

    server.publish(
        TRIANGLE,
        &library.package(TRIANGLE, "1.1.0", b"\0asm"),
        "1.1.0",
    );
    library.update(|library, cx| library.read_sources(cx));
    library.wait_until("the newer listing", |library| {
        matches!(&library.sources()[0].listing, Listing::Read(offers) if offers[0].0.version.to_string() == "1.1.0")
    });
    assert_eq!(
        library.offers(),
        [(
            TRIANGLE.to_owned(),
            Offer::Update {
                installed: "1.0.0".to_owned()
            }
        )]
    );
    install_what_the_source_lists(&library);
    assert_eq!(
        library.installed(),
        [(TRIANGLE.to_owned(), "1.1.0".to_owned())]
    );
    assert_eq!(library.offers(), [(TRIANGLE.to_owned(), Offer::Installed)]);
}

/// Installs the first program the one source lists, and waits until it is installed.
fn install_what_the_source_lists(library: &TestLibrary) {
    let (source, listed) = library.read(|library| {
        let read = &library.sources()[0];
        let Listing::Read(offers) = &read.listing else {
            panic!("the source is read");
        };
        (read.source.clone(), offers[0].0.clone())
    });
    library.update(|library, cx| library.install_from(&source, &listed, cx));
    assert!(library.read(|library| library.is_installing(&listed.id)));
    library.wait_until("the install", |library| !library.is_installing(&listed.id));
    assert_eq!(
        library.read(|library| library.problem().map(str::to_owned)),
        None
    );
}

#[test]
fn a_source_that_cannot_be_read_says_why() {
    let library = TestLibrary::new();
    let server = Server::start();
    let missing = server.url("missing/index.json");
    library.update(|library, cx| library.add_source(&missing, cx));
    library.wait_until("the source's answer", |library| {
        !matches!(library.sources()[0].listing, Listing::Reading)
    });
    library.read(|library| match &library.sources()[0].listing {
        Listing::Failed(why) => assert!(why.contains("404"), "{why}"),
        _ => panic!("the source is not read"),
    });
}

#[test]
fn text_that_names_no_source_is_the_librarys_problem() {
    let library = TestLibrary::new();
    library.update(|library, cx| library.add_source("a triangle, please", cx));
    assert!(library.read(|library| library.problem().is_some()));
    assert!(library.read(|library| library.sources().is_empty()));
}

#[test]
fn a_removed_source_takes_its_listing_with_it() {
    let library = TestLibrary::new();
    let server = Server::start();
    server.publish(
        TRIANGLE,
        &library.package(TRIANGLE, "1.0.0", b"\0asm"),
        "1.0.0",
    );
    let index = server.url("waos/index.json");
    library.update(|library, cx| library.add_source(&index, cx));
    let source = library.read(|library| library.sources()[0].source.clone());
    library.update(|library, cx| library.remove_source(&source, cx));
    assert!(library.read(|library| library.sources().is_empty()));
    library.cell.checkpoint();
    thread::sleep(Duration::from_millis(50));
    library.cell.checkpoint();
    assert!(
        library.read(|library| library.sources().is_empty()),
        "a listing read after the source went is dropped"
    );
}

#[test]
fn an_installed_program_opens_in_waos_once_however_often_it_is_opened() {
    let library = TestLibrary::new();
    let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", &forever));
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.update(|library, cx| library.open(&triangle, cx));
    wait_for_runs(&library, &[TRIANGLE]);
    library.update(|library, cx| library.open(&triangle, cx));
    library.wait_until("the second open's answer", |library| {
        library.problem().is_none()
    });
    thread::sleep(Duration::from_millis(50));
    library.cell.checkpoint();
    assert_eq!(
        futures::executor::block_on(library.runs.programs()).len(),
        1
    );
    futures::executor::block_on(library.runs.stop_all());
    wait_for_runs(&library, &[]);
}

#[test]
fn a_launch_for_an_installed_program_opens_it_once() {
    let library = TestLibrary::new();
    let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", &forever));
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    let token = || Some("gnome-shell/Triangle/1".to_owned());
    let seen_before = library.asked_to_be_seen();
    library.update(|library, cx| library.show_program(&triangle, token(), cx));
    wait_for_runs(&library, &[TRIANGLE]);
    library.update(|library, cx| library.show_program(&triangle, token(), cx));
    library.wait_until("the second launch's answer", |library| {
        library.problem().is_none()
    });
    assert_eq!(
        futures::executor::block_on(library.runs.programs()).len(),
        1
    );
    assert_eq!(library.asked_to_be_seen(), seen_before, "nothing to say");
    futures::executor::block_on(library.runs.stop_all());
    wait_for_runs(&library, &[]);
}

#[test]
fn a_launch_for_a_program_no_longer_installed_says_so() {
    let library = TestLibrary::new();
    let gone: ProgramId = "dev.example.gone".parse().unwrap();
    library.update(|library, cx| library.show_program(&gone, None, cx));
    assert_eq!(
        library.read(|library| library.problem().map(str::to_owned)),
        Some("dev.example.gone is not installed, or its files cannot be read".to_owned())
    );
    assert_eq!(library.asked_to_be_seen(), 1);
}

#[test]
fn an_installed_program_is_compiling_until_its_code_is_ready() {
    let library = TestLibrary::new();
    let held = library.hold_compiles();
    library.installs_forever();
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    assert!(
        library.read(|library| library.is_compiling(&triangle)),
        "compiling from the moment it is installed"
    );
    drop(held);
    library.wait_until("its code", |library| !library.is_compiling(&triangle));
    assert_eq!(library.compiled_files().len(), 1, "its code, in the cache");
}

#[test]
fn an_open_while_the_install_compiles_waits_for_that_compile_then_opens() {
    let library = TestLibrary::new();
    let held = library.hold_compiles();
    library.installs_forever();
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.update(|library, cx| library.open(&triangle, cx));
    thread::sleep(Duration::from_millis(50));
    library.cell.checkpoint();
    assert!(library.read(|library| library.is_compiling(&triangle)));
    assert!(
        futures::executor::block_on(library.runs.names()).is_empty(),
        "the open waits"
    );
    drop(held);
    wait_for_runs(&library, &[TRIANGLE]);
    assert!(!library.read(|library| library.is_compiling(&triangle)));
    assert_eq!(library.compiled_files().len(), 1);
    futures::executor::block_on(library.runs.stop_all());
    wait_for_runs(&library, &[]);
}

/// As after an update of WaOS that changed its engine, whose code cache then holds nothing
/// current: the first open compiles, and the program is compiling meanwhile.
#[test]
fn the_first_open_compiles_where_the_cache_holds_nothing_current() {
    let library = TestLibrary::new();
    library.installs_forever();
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.wait_until("its code", |library| !library.is_compiling(&triangle));
    for file in library.compiled_files() {
        std::fs::remove_file(file).unwrap();
    }
    let held = library.hold_compiles();
    library.update(|library, cx| library.open(&triangle, cx));
    library.wait_until("the open's compile", |library| {
        library.is_compiling(&triangle)
    });
    drop(held);
    wait_for_runs(&library, &[TRIANGLE]);
    assert!(!library.read(|library| library.is_compiling(&triangle)));
    assert_eq!(library.compiled_files().len(), 1);
    futures::executor::block_on(library.runs.stop_all());
    wait_for_runs(&library, &[]);
}

#[test]
fn a_removed_program_leaves_no_code() {
    let library = TestLibrary::new();
    library.installs_forever();
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.wait_until("its code", |library| !library.is_compiling(&triangle));
    assert_eq!(library.compiled_files().len(), 1);
    library.update(|library, cx| library.ask_to_remove(&triangle, cx));
    library.update(|library, cx| library.answer(true, cx));
    assert!(library.installed().is_empty());
    assert_eq!(library.compiled_files(), Vec::<PathBuf>::new());
}

#[test]
fn an_update_leaves_code_for_the_new_version_only_and_a_run_of_the_old_one_running() {
    let library = TestLibrary::new();
    library.installs_forever();
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.wait_until("its code", |library| !library.is_compiling(&triangle));
    library.update(|library, cx| library.open(&triangle, cx));
    wait_for_runs(&library, &[TRIANGLE]);
    let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
    library.installs_after_yes(library.package(TRIANGLE, "1.1.0", &forever));
    assert_eq!(
        library.installed(),
        [(TRIANGLE.to_owned(), "1.1.0".to_owned())]
    );
    library.wait_until("the new version's code", |library| {
        !library.is_compiling(&triangle)
    });
    assert_eq!(library.compiled_files().len(), 1, "one version's code");
    let updated = library.read(|library| library.store.program(&triangle).unwrap());
    assert!(
        library.read(|library| library.runs.engine.is_compiled(&updated).unwrap()),
        "the new version's"
    );
    thread::sleep(Duration::from_millis(50));
    library.cell.checkpoint();
    assert_eq!(
        futures::executor::block_on(library.runs.names()),
        [TRIANGLE],
        "the old version runs on"
    );
    futures::executor::block_on(library.runs.stop_all());
    wait_for_runs(&library, &[]);
}

#[test]
fn files_opened_with_an_installed_program_at_start_open_it_and_reach_it() {
    let library = TestLibrary::new();
    let opener = std::fs::read(waos_test_programs::OPENER).unwrap();
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", &opener));
    let notes = std::fs::canonicalize(library.folder.path())
        .unwrap()
        .join("notes.md");
    std::fs::write(&notes, "my notes").unwrap();
    let request = OpenRequest {
        files: vec![notes.clone()],
        links: Vec::new(),
    };
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.update(|library, cx| library.open_with(&triangle, request, cx));
    wait_for_runs(&library, &[TRIANGLE]);
    let deadline = Instant::now() + WITHIN;
    while std::fs::read_to_string(&notes).unwrap() != "my notes, seen" {
        assert!(Instant::now() < deadline, "the program never wrote to it");
        thread::sleep(Duration::from_millis(10));
    }
    futures::executor::block_on(library.runs.stop_all());
    wait_for_runs(&library, &[]);
}

#[test]
fn files_opened_with_a_program_not_installed_are_the_librarys_problem() {
    let library = TestLibrary::new();
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    let request = OpenRequest {
        files: vec![library.folder.path().join("notes.md")],
        links: Vec::new(),
    };
    library.update(|library, cx| library.open_with(&triangle, request, cx));
    library.wait_until("the open's answer", |library| library.problem().is_some());
    let problem = library
        .read(|library| library.problem().map(str::to_owned))
        .unwrap();
    assert!(
        problem.contains("neither installed nor running"),
        "{problem}"
    );
    assert_eq!(library.asked_to_be_seen(), 1, "to say why");
}

#[test]
fn a_program_that_cannot_be_loaded_is_the_librarys_problem() {
    let library = TestLibrary::new();
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.update(|library, cx| library.open(&triangle, cx));
    library.wait_until("the open's answer", |library| library.problem().is_some());
    let problem = library
        .read(|library| library.problem().map(str::to_owned))
        .unwrap();
    assert!(problem.contains("could not be opened"), "{problem}");
    assert_eq!(
        library.asked_to_be_seen(),
        2,
        "after the install, and to say why"
    );
    assert!(futures::executor::block_on(library.runs.names()).is_empty());
}

/// Waits until the runs are exactly `names`.
fn wait_for_runs(library: &TestLibrary, names: &[&str]) {
    let deadline = Instant::now() + WITHIN;
    loop {
        let mut running = futures::executor::block_on(library.runs.names());
        running.sort();
        if running == names {
            return;
        }
        assert!(Instant::now() < deadline, "the runs stayed {running:?}");
        thread::sleep(Duration::from_millis(10));
        library.cell.checkpoint();
    }
}

/// A document handed without a path that seeks, as Android hands one from its Files app: `bytes`
/// under `name`, or an error at every read.
struct TestHandle {
    name: String,
    bytes: Option<Vec<u8>>,
}

impl ui::FileHandle for TestHandle {
    fn name(&self) -> &str {
        &self.name
    }

    fn url(&self) -> String {
        format!("content://test/{}", self.name)
    }

    fn size(&self) -> ui::FileSizeFuture {
        let size = self.bytes.as_ref().map(|bytes| bytes.len() as u64);
        Box::pin(async move { size.ok_or_else(|| std::io::Error::other("the provider is gone")) })
    }

    fn read_at<'a>(&'a self, offset: u64, buf: &'a mut [u8]) -> ui::FileReadFuture<'a> {
        Box::pin(async move {
            let bytes = self
                .bytes
                .as_ref()
                .ok_or_else(|| std::io::Error::other("the provider is gone"))?;
            let rest = &bytes[(offset as usize).min(bytes.len())..];
            let read = rest.len().min(buf.len());
            buf[..read].copy_from_slice(&rest[..read]);
            Ok(read)
        })
    }

    fn open_write(&self) -> Option<ui::FileOpenFuture<Box<dyn ui::FileWriter>>> {
        None
    }
}

/// A document handed without a path that is a pipe, read once through: `bytes` under `name`.
struct TestStream {
    name: String,
    bytes: std::io::Cursor<Vec<u8>>,
}

impl ui::FileStream for TestStream {
    fn name(&self) -> &str {
        &self.name
    }

    fn url(&self) -> String {
        format!("content://test/{}", self.name)
    }

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> ui::FileReadFuture<'a> {
        Box::pin(async move { std::io::Read::read(&mut self.bytes, buf) })
    }
}

impl TestLibrary {
    /// What is in the folder the packages handed without a path are copied to.
    fn copies(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.folder.path().join("package-copies"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    /// Hands the library `document`, as the system opens a document with WaOS.
    fn open_document(&self, document: PickedFile) {
        self.update(|library, cx| library.open_documents(vec![document], cx));
    }

    /// A handle to `file`'s bytes under `name`.
    fn handle(name: &str, file: &std::path::Path) -> PickedFile {
        PickedFile::Handle(Box::new(TestHandle {
            name: name.to_owned(),
            bytes: Some(std::fs::read(file).unwrap()),
        }))
    }
}

#[test]
fn a_package_handed_as_a_handle_is_copied_asked_about_and_its_copy_removed_once_installed() {
    let library = TestLibrary::new();
    let package = library.package(TRIANGLE, "1.0.0", b"\0asm");
    library.open_document(TestLibrary::handle("triangle.wapk", &package));
    library.wait_until("the question", |library| library.question().is_some());
    assert_eq!(library.copies().len(), 1, "a copy while it is asked about");
    library.update(|library, cx| library.answer(true, cx));
    assert_eq!(
        library.installed(),
        [(TRIANGLE.to_owned(), "1.0.0".to_owned())]
    );
    assert!(library.copies().is_empty(), "the copy goes once answered");
}

#[test]
fn a_package_handed_as_a_stream_is_asked_about_and_its_copy_removed_once_refused() {
    let library = TestLibrary::new();
    let package = library.package(TRIANGLE, "1.0.0", b"\0asm");
    library.open_document(PickedFile::Stream(Box::new(TestStream {
        name: "triangle.wapk".to_owned(),
        bytes: std::io::Cursor::new(std::fs::read(&package).unwrap()),
    })));
    library.wait_until("the question", |library| library.question().is_some());
    library.update(|library, cx| library.answer(false, cx));
    assert!(library.installed().is_empty());
    assert!(library.copies().is_empty(), "the copy goes once answered");
}

#[test]
fn a_document_that_cannot_be_read_is_the_librarys_problem() {
    let library = TestLibrary::new();
    library.open_document(PickedFile::Handle(Box::new(TestHandle {
        name: "triangle.wapk".to_owned(),
        bytes: None,
    })));
    library.wait_until("the problem", |library| library.problem().is_some());
    let problem = library.read(|library| library.problem().unwrap().to_owned());
    assert!(
        problem.contains("triangle.wapk could not be read"),
        "{problem}"
    );
    assert!(
        *library.asked_to_be_seen.lock().unwrap() > 0,
        "it asks to be seen"
    );
    assert!(library.copies().is_empty());
}

#[test]
fn a_document_whose_name_is_not_a_file_name_is_refused() {
    let library = TestLibrary::new();
    let package = library.package(TRIANGLE, "1.0.0", b"\0asm");
    library.open_document(TestLibrary::handle("../triangle.wapk", &package));
    library.wait_until("the problem", |library| library.problem().is_some());
    assert!(library.read(|library| library.question().is_none()));
    assert!(library.copies().is_empty());
}

#[test]
fn a_document_that_is_no_package_is_the_problem_and_its_copy_goes() {
    let library = TestLibrary::new();
    let not_a_package = library.folder.path().join("notes.txt");
    std::fs::write(&not_a_package, b"hello").unwrap();
    library.open_document(TestLibrary::handle("notes.wapk", &not_a_package));
    library.wait_until("the problem", |library| library.problem().is_some());
    assert!(library.read(|library| library.question().is_none()));
    assert!(library.copies().is_empty());
}

#[test]
fn a_request_to_open_files_is_told_apart_into_paths_and_documents() {
    let request = ui::OpenRequest {
        files: vec![
            PickedFile::Path(PathBuf::from("/tmp/a.wapk")),
            PickedFile::Stream(Box::new(TestStream {
                name: "b.wapk".to_owned(),
                bytes: std::io::Cursor::new(Vec::new()),
            })),
        ],
        links: Vec::new(),
    };
    let opened = OpenedFiles::of(request);
    assert_eq!(opened.paths, [PathBuf::from("/tmp/a.wapk")]);
    assert_eq!(opened.documents.len(), 1);
}

/// What the library asked of a system's shortcuts, as a phone's launcher would keep them: each
/// shortcut by its id, pinned or recent, enabled or not.
#[derive(Default)]
struct TestShortcuts {
    pins: bool,
    kept: std::cell::RefCell<Vec<KeptShortcut>>,
    /// The ids the launcher was asked to pin, in order.
    pin_requests: std::cell::RefCell<Vec<String>>,
    /// The ids put first among the recent programs, in order, which the launcher keeps unpinned.
    recent: std::cell::RefCell<Vec<String>>,
    /// Each shortcut updated, in order.
    updates: std::cell::RefCell<Vec<ProgramShortcut>>,
}

impl TestShortcuts {
    /// Keeps the shortcut `id`, pinned where `pinned`, which a pin makes so for one kept already.
    fn keep(&self, id: &str, pinned: bool) {
        let mut kept = self.kept.borrow_mut();
        match kept.iter_mut().find(|shortcut| shortcut.id == id) {
            Some(shortcut) => shortcut.pinned |= pinned,
            None => kept.push(KeptShortcut {
                id: id.to_owned(),
                pinned,
                enabled: true,
                disabled_by_waos: false,
            }),
        }
    }

    /// Whether the shortcut `id` is kept, and enabled.
    fn state_of(&self, id: &str) -> Option<bool> {
        let kept = self.kept.borrow();
        kept.iter()
            .find(|shortcut| shortcut.id == id)
            .map(|shortcut| shortcut.enabled)
    }
}

impl SystemShortcuts for TestShortcuts {
    fn pins(&self) -> bool {
        self.pins
    }

    fn icon_size(&self) -> u32 {
        8
    }

    fn request_pin(
        &self,
        shortcut: &ProgramShortcut,
    ) -> futures::future::LocalBoxFuture<'static, Result<(), String>> {
        self.pin_requests.borrow_mut().push(shortcut.id.to_string());
        self.keep(shortcut.id.as_str(), true);
        Box::pin(std::future::ready(Ok(())))
    }

    fn add_recent(&self, shortcut: &ProgramShortcut) {
        self.recent.borrow_mut().push(shortcut.id.to_string());
        self.keep(shortcut.id.as_str(), false);
    }

    fn kept(&self) -> futures::future::LocalBoxFuture<'static, Result<Vec<KeptShortcut>, String>> {
        Box::pin(std::future::ready(Ok(self.kept.borrow().clone())))
    }

    fn update(
        &self,
        shortcuts: &[ProgramShortcut],
    ) -> futures::future::LocalBoxFuture<'static, Result<(), String>> {
        self.updates.borrow_mut().extend_from_slice(shortcuts);
        Box::pin(std::future::ready(Ok(())))
    }

    fn enable(&self, ids: &[ProgramId]) {
        for shortcut in self.kept.borrow_mut().iter_mut() {
            if ids.iter().any(|id| id.as_str() == shortcut.id) {
                shortcut.enabled = true;
                shortcut.disabled_by_waos = false;
            }
        }
    }

    fn disable(&self, ids: &[String]) {
        for shortcut in self.kept.borrow_mut().iter_mut() {
            if ids.contains(&shortcut.id) {
                shortcut.enabled = false;
                shortcut.disabled_by_waos = true;
            }
        }
    }

    fn hear_taps(&self, _tapped: Box<dyn Fn(ProgramId)>) {}
}

impl TestLibrary {
    /// Keeps the programs' shortcuts in a system of the test's own, whose launcher pins as `pins`
    /// says, already keeping shortcuts of `kept`.
    fn keeping_shortcuts(&self, pins: bool, kept: &[&str]) -> Rc<TestShortcuts> {
        let system = Rc::new(TestShortcuts {
            pins,
            ..TestShortcuts::default()
        });
        for id in kept {
            system.keep(id, true);
        }
        let kept_in = Rc::clone(&system) as Rc<dyn SystemShortcuts>;
        self.update(|library, cx| library.keep_shortcuts(kept_in, cx));
        system
    }
}

#[test]
fn a_program_installed_for_the_first_time_is_offered_the_home_screen_and_an_update_is_not() {
    let library = TestLibrary::new();
    let shortcuts = library.keeping_shortcuts(true, &[]);
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    library.wait_until("the pin's request", |_| {
        !shortcuts.pin_requests.borrow().is_empty()
    });
    library.installs_after_yes(library.package(TRIANGLE, "2.0.0", b"\0asm"));
    library.wait_until("the update's shortcut updated", |_| {
        !shortcuts.updates.borrow().is_empty()
    });
    assert_eq!(*shortcuts.pin_requests.borrow(), [TRIANGLE]);
    let updated = shortcuts.updates.borrow().last().cloned().unwrap();
    assert_eq!(updated.name, "Triangle");
    assert_eq!(updated.opens, ShortcutOpens::OwnTask);
}

#[test]
fn the_home_screen_is_offered_only_where_the_launcher_pins() {
    let library = TestLibrary::new();
    let shortcuts = library.keeping_shortcuts(false, &[]);
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    assert!(!library.read(|library| library.adds_to_home_screen(&triangle)));
    thread::sleep(Duration::from_millis(50));
    library.cell.checkpoint();
    assert!(shortcuts.pin_requests.borrow().is_empty());
}

#[test]
fn a_program_opened_goes_first_among_the_recent_programs() {
    let library = TestLibrary::new();
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    let shortcuts = library.keeping_shortcuts(true, &[]);
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.update(|library, cx| library.open(&triangle, cx));
    library.wait_until("the recent program", |_| {
        !shortcuts.recent.borrow().is_empty()
    });
    assert_eq!(*shortcuts.recent.borrow(), [TRIANGLE]);
}

#[test]
fn a_removed_programs_shortcut_is_disabled_and_enabled_once_it_is_installed_again() {
    let library = TestLibrary::new();
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    let shortcuts = library.keeping_shortcuts(true, &[TRIANGLE, SURVEY]);
    library.wait_until("the shortcut of a program not installed disabled", |_| {
        shortcuts.state_of(SURVEY) == Some(false)
    });
    assert_eq!(shortcuts.state_of(TRIANGLE), Some(true));
    let triangle: ProgramId = TRIANGLE.parse().unwrap();
    library.update(|library, cx| library.ask_to_remove(&triangle, cx));
    library.update(|library, cx| library.answer(true, cx));
    library.wait_until("the removed program's shortcut disabled", |_| {
        shortcuts.state_of(TRIANGLE) == Some(false)
    });
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    library.wait_until("the shortcut enabled again", |_| {
        shortcuts.state_of(TRIANGLE) == Some(true)
    });
    thread::sleep(Duration::from_millis(50));
    library.cell.checkpoint();
    assert!(
        shortcuts.pin_requests.borrow().is_empty(),
        "an icon on the home screen already is not asked for again"
    );
}

#[test]
fn a_program_only_among_the_recent_ones_is_offered_the_home_screen_when_installed_again() {
    let library = TestLibrary::new();
    let shortcuts = library.keeping_shortcuts(true, &[]);
    shortcuts.keep(TRIANGLE, false);
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    library.wait_until("the pin's request", |_| {
        !shortcuts.pin_requests.borrow().is_empty()
    });
}

#[test]
fn choosing_programs_inside_waos_points_the_shortcuts_at_waos() {
    let library = TestLibrary::opening(ProgramsOpen::AsChosen, |folder| {
        Some(PreferencesFile::at(folder.join("preferences.toml")))
    });
    library.installs_after_yes(library.package(TRIANGLE, "1.0.0", b"\0asm"));
    let shortcuts = library.keeping_shortcuts(true, &[TRIANGLE]);
    library.update(|library, cx| library.show_programs_inside_waos(true, cx));
    library.wait_until("the shortcut pointed at WaOS", |_| {
        shortcuts
            .updates
            .borrow()
            .last()
            .is_some_and(|updated| updated.opens == ShortcutOpens::InsideWaos)
    });
}
