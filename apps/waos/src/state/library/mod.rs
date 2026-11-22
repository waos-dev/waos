//! The library: the programs installed, each with its icon, to open and remove, and which of them
//! run; the sources added, each with what it lists, to install or update; the packages opened with
//! WaOS or picked in the system's file picker (`package_picker`), each waiting for the user to say
//! whether to install it, a package handed without a path first copied into a file of WaOS's own
//! (`logic::package_copies`); and the settings, global and each program's, with the page the
//! library is on (`settings`), and what the user searches the installed programs for. Every layout
//! of the library shows this one state. A click on a program's notification reaches the program
//! through it, as a program opened from it does. A program opened from it opens where WaOS's
//! programs open (`programs_open`): in windows it asks for, on a desktop and on Android each a task
//! of its own, or in a tab of the deck (`state::deck`), the home's tab where the home is open,
//! which gives it its view as it starts; a program running opens where it runs, its windows coming
//! forward. On Android the user chooses between tasks and tabs, which holds for programs opened
//! after, and the installed programs have shortcuts on the home screen and on WaOS's icon's menu
//! (`shortcuts`), and the global settings say whether WaOS may reach the phone's files
//! (`phone_files`).
//!
//! What reads the network or the disk at length, a source's index, a download, a program's load,
//! runs on the library's own tokio runtime, and its answer comes back to the main thread, where
//! the library changes and notifies. A program installed or updated has its code compiled, on a
//! thread of its own (`logic::installed_code`), and is compiling until its code is ready; a
//! program removed, or the version an update replaces, has its code removed first. A program
//! removed or updated while it runs ends first, its tab closing and its windows with it, as a
//! system ends an app it uninstalls or updates: the removal waits for the end before it takes the
//! program's folders, and an update from a source ends it once the package is downloaded; so no
//! run outlives the package it came from. What was asked last and failed is the library's
//! problem, until the next thing asked succeeds or the user dismisses it.
//!
//! The invariant: the library changes the store only through the store's own rules, and reads
//! what is installed again after every change it makes, with each program's icon, so what it shows
//! is what is installed; a source's listing is read against the installed programs whenever either
//! changes; which programs run is the runs' own list, as it last changed; and which programs'
//! code is compiled is the compiles' own list, read as the library is shown.

mod package_picker;
mod phone_files;
mod programs_open;
mod settings;
mod shortcuts;
#[cfg(test)]
pub(crate) mod test_desktop;
#[cfg(test)]
pub(crate) mod test_server;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use tokio::runtime::Runtime;
use tokio::sync::watch;
use waft::ui::{self, NotificationActivation, PickedFile};
use waft::{App, Context, Entity, EventEmitter, OpenRequestedCallback, WeakEntity};
use waft_single_instance::{LaterLaunches, Launch};
use waos::OpenRequest;
use waos::bindings::wasdk::platform::notifications::Activation;
use waos_package::{ListedProgram, ProgramId, ProgramKind};
use waos_store::{InstalledProgram, Source, Store};

use crate::logic::command_line::{Command, command};
use crate::logic::installed_runs::{InstalledRuns, Opening};
use crate::logic::offers::{Offer, offer_of};
use crate::logic::opened_package::OpenedPackage;
use crate::logic::opening;
use crate::logic::package_copies::{PackageCopies, document_name};
use crate::logic::phone_files::PhoneFiles;
use crate::logic::preferences::{KeptPreferences, Preferences, PreferencesFile};
use crate::logic::program_faces::program_face;
use crate::logic::program_places::ProgramPlace;
use crate::logic::program_shortcuts::SystemShortcuts;
use crate::logic::setting_changes::Offered;
use crate::logic::system_notifications::program_activation;
use crate::state::deck::tabs::Tabs;
pub(crate) use package_picker::picks_packages;
pub(crate) use phone_files::show_phone_files;
pub(crate) use programs_open::ProgramsOpen;
pub(crate) use settings::{LeftSettings, LibraryPage, SwitchAsked, open_settings_when_asked};
use shortcuts::ShortcutsMatching;
use waos_program_window::ProgramFace;

pub(crate) struct Library {
    store: Store,
    /// Where opened programs run.
    runs: InstalledRuns,
    /// The deck's tabs a program opened from the library may open in, on a phone; none on a
    /// desktop, where a program asks for its windows.
    tabs: Option<Entity<Tabs>>,
    /// Where the programs opened from the library open.
    programs_open: ProgramsOpen,
    /// WaOS's own preferences, where it keeps them.
    preferences: Option<KeptPreferences>,
    /// Where the store reads sources and installs from them, and programs load: reqwest needs
    /// tokio.
    work: Arc<Runtime>,
    installed: Vec<InstalledProgram>,
    /// Each installed program's icon, a PNG, kept so a widget showing it shows the same bytes each
    /// build; none for one whose icon cannot be read.
    icons: HashMap<ProgramId, Arc<[u8]>>,
    /// The programs running now, by id, whoever started them.
    running: BTreeSet<ProgramId>,
    sources: Vec<SourceListing>,
    /// The programs being installed from a source now, by id.
    installing: Vec<ProgramId>,
    /// What the user is asked now, and then the packages opened with WaOS waiting their turn.
    questions: Vec<Question>,
    /// Why what was asked last failed.
    problem: Option<String>,
    /// Where the packages handed without a path are copied while they are asked about.
    package_copies: PackageCopies,
    /// Whether the system's file picker the library opened for a package is open, until it
    /// answers (`package_picker`).
    picking: bool,
    /// The page the library is on.
    page: LibraryPage,
    /// The switch a program asked its user to change, on the settings page open for it.
    switch_asked: Option<SwitchAsked>,
    /// What the programs' pickers offer, read as a program's settings page is shown, so a build
    /// of the page asks no device.
    offered: Offered,
    /// The programs waiting for the user to leave the settings page the library is on.
    waiting_to_leave: Vec<LeftSettings>,
    /// The programs the system refuses notifications, as their apps last said.
    notifications_refused: HashSet<ProgramId>,
    /// What the user searches the installed programs for, as typed; empty for nothing.
    search: String,
    /// Where the installed programs' shortcuts are kept, on Android; none elsewhere.
    shortcuts: Option<Rc<dyn SystemShortcuts>>,
    /// Whether they are being made to match what is installed.
    shortcuts_matching: ShortcutsMatching,
    /// Whether WaOS may reach the phone's files, where it reaches them only as the user allows.
    phone_files: Option<Arc<dyn PhoneFiles>>,
}

/// A source the user added, and what it lists.
pub(crate) struct SourceListing {
    pub(crate) source: Source,
    pub(crate) listing: Listing,
}

/// What a source lists, as far as it has been read.
pub(crate) enum Listing {
    Reading,
    /// Each program it lists, with what the library offers to do with it.
    Read(Vec<(ListedProgram, Offer)>),
    /// Why it could not be read.
    Failed(String),
}

/// What the library asks the user.
pub(crate) enum Question {
    /// Whether to install a package opened with WaOS.
    Install(Box<OpenedPackage>),
    /// Whether to remove an installed program, which `name` names to the user.
    Remove { id: ProgramId, name: String },
}

/// The library has something to ask or to say, and wants to be seen.
pub(crate) struct WantsToBeSeen;

/// The program `id` was opened from the library, or by a launch of WaOS, and its windows come
/// forward, taking the focus with the launch's activation token where it brought one.
pub(crate) struct ProgramOpened {
    pub(crate) id: ProgramId,
    pub(crate) activation_token: Option<String>,
}

impl EventEmitter<WantsToBeSeen> for Library {}

/// The program `id`, about to start on Android as a task of its own, wants its first window opened
/// now, showing it as `face` says, which arrives as a program's first window the platform gives
/// and starts the program with it reserved (decision 10 of
/// `docs/proposals/2026-11-21-views-a-program-asks-for.md`).
pub(crate) struct FirstWindowWanted {
    pub(crate) id: ProgramId,
    pub(crate) face: ProgramFace,
}

impl EventEmitter<FirstWindowWanted> for Library {}

/// The user changed WaOS's own preferences, which hold from now on.
pub(crate) struct PreferencesChosen;

impl EventEmitter<PreferencesChosen> for Library {}

/// What is installed changed: a program was installed, updated or removed.
pub(crate) struct InstalledChanged;

impl EventEmitter<InstalledChanged> for Library {}
impl EventEmitter<ProgramOpened> for Library {}

/// How many threads the library's runtime reads, downloads and loads on.
const WORK_THREADS: usize = 2;

/// The library over `store`, opening programs with `runs` where `programs_open` says, copying the
/// packages handed without a path into `package_copies`, with WaOS's own preferences in
/// `preferences`, where it keeps them, on a runtime of its own.
pub(crate) fn open_library(
    app: &mut App,
    store: Store,
    runs: InstalledRuns,
    package_copies: PackageCopies,
    programs_open: ProgramsOpen,
    preferences: Option<PreferencesFile>,
) -> Result<Entity<Library>> {
    let work = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORK_THREADS)
        .thread_name("waos-library")
        .enable_all()
        .build()
        .context("the library's runtime")?;
    let work = Arc::new(work);
    let preferences = preferences.map(KeptPreferences::of);
    Ok(app.new_entity(|cx| {
        Library::new(
            store,
            runs,
            package_copies,
            programs_open,
            preferences,
            work,
            cx,
        )
    }))
}

impl Library {
    /// The library over `store`, opening programs with `runs` where `programs_open` says, with
    /// WaOS's own `preferences`, and reading and installing on `work`; what each source lists is
    /// read now.
    fn new(
        store: Store,
        runs: InstalledRuns,
        package_copies: PackageCopies,
        programs_open: ProgramsOpen,
        preferences: Option<KeptPreferences>,
        work: Arc<Runtime>,
        cx: &mut Context<Library>,
    ) -> Library {
        let installed = store.installed();
        let mut library = Library {
            icons: icons_of(&store, &installed),
            installed,
            running: BTreeSet::new(),
            sources: Vec::new(),
            store,
            runs,
            tabs: None,
            programs_open,
            preferences,
            work,
            installing: Vec::new(),
            questions: Vec::new(),
            problem: None,
            package_copies,
            picking: false,
            page: LibraryPage::Home,
            switch_asked: None,
            offered: Offered::default(),
            waiting_to_leave: Vec::new(),
            notifications_refused: HashSet::new(),
            search: String::new(),
            shortcuts: None,
            shortcuts_matching: ShortcutsMatching::Idle,
            phone_files: None,
        };
        library.read_sources(cx);
        library.follow_running(cx);
        library.follow_compiling(cx);
        library
    }

    /// Keeps which programs run as the runs' list changes, from now on.
    fn follow_running(&mut self, cx: &mut Context<Library>) {
        let mut ids = self.runs.runs.running_ids();
        self.running = ids.borrow_and_update().clone();
        follow(
            ids,
            |library, running, cx| {
                library.running = running;
                cx.notify();
            },
            cx,
        );
    }

    /// Shows which programs' code is being compiled each time that changes, from now on; the
    /// compiles' own list is read as the library is shown.
    fn follow_compiling(&mut self, cx: &mut Context<Library>) {
        let ids = self.runs.code.compiling_ids();
        follow(ids, |_library, _compiling, cx| cx.notify(), cx);
    }

    /// From now on a program opened from the library may open in a tab of `tabs`, as on a phone,
    /// where programs open in tabs.
    pub(crate) fn open_in(&mut self, tabs: Entity<Tabs>) {
        self.tabs = Some(tabs);
    }

    /// The programs installed, by id.
    pub(crate) fn installed(&self) -> &[InstalledProgram] {
        &self.installed
    }

    /// The icon of the installed program `id`, a PNG; none where it cannot be read.
    pub(crate) fn icon(&self, id: &ProgramId) -> Option<&Arc<[u8]>> {
        self.icons.get(id)
    }

    /// What the user searches the installed programs for, as typed; empty for nothing.
    pub(crate) fn search(&self) -> &str {
        &self.search
    }

    /// The user searches the installed programs for `typed` from now on.
    pub(crate) fn search_for(&mut self, typed: &str, cx: &mut Context<Library>) {
        if self.search != typed {
            typed.clone_into(&mut self.search);
            cx.notify();
        }
    }

    /// Whether the program `id` runs now.
    pub(crate) fn is_running(&self, id: &ProgramId) -> bool {
        self.running.contains(id)
    }

    /// Whether the installed program `id`'s code is being compiled now, as at its install, or at
    /// its first open once an update of WaOS changed the engine: until it is, it is not ready to
    /// open, and an open waits for it.
    pub(crate) fn is_compiling(&self, id: &ProgramId) -> bool {
        self.runs.code.is_compiling(id)
    }

    /// Whether a source lists the installed program `id` at a newer version than installed.
    pub(crate) fn update_offered(&self, id: &ProgramId) -> bool {
        self.sources.iter().any(|read| match &read.listing {
            Listing::Read(offers) => offers
                .iter()
                .any(|(program, offer)| &program.id == id && matches!(offer, Offer::Update { .. })),
            Listing::Reading | Listing::Failed(_) => false,
        })
    }

    /// Whether the library closes the programs it opened: where they open in tabs; not where they
    /// open in windows, which are a program's own to close.
    pub(crate) fn closes_programs(&self) -> bool {
        self.tabs.is_some() && self.programs_open_in_tabs()
    }

    /// Closes the program `id`'s tab, which stops it without asking, as a phone's switcher ends an
    /// app; nothing on a desktop.
    pub(crate) fn close(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        if let Some(tabs) = self.tabs.clone() {
            tabs.update(cx, |tabs, cx| tabs.close_program(id, cx));
        }
    }

    /// The sources added, in the order added, with what each lists.
    pub(crate) fn sources(&self) -> &[SourceListing] {
        &self.sources
    }

    /// Whether the program `id` is being installed from a source now.
    pub(crate) fn is_installing(&self, id: &ProgramId) -> bool {
        self.installing.contains(id)
    }

    /// What the user is asked now.
    pub(crate) fn question(&self) -> Option<&Question> {
        self.questions.first()
    }

    pub(crate) fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub(crate) fn dismiss_problem(&mut self, cx: &mut Context<Library>) {
        self.problem = None;
        cx.notify();
    }

    /// Opens the installed program `id`, as `waos run` runs a program, from its component in
    /// the store, where programs open: in a tab of its own, or in windows it asks for; one running
    /// already is left as it is, where it runs. Its windows come forward, or its tab, and one that
    /// cannot be opened is the library's problem, and asks to be seen.
    pub(crate) fn open(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        self.open_bringing_forward(id, None, cx);
    }

    /// Opens the installed program `id` as a launch of WaOS asked, as a pin of the program does, or
    /// brings it forward where it runs, its window taking the focus with the launch's
    /// `activation_token` where it brought one. A program not installed, as one whose pin outlived
    /// it, is the library's problem, and asks to be seen.
    pub(crate) fn show_program(
        &mut self,
        id: &ProgramId,
        activation_token: Option<String>,
        cx: &mut Context<Library>,
    ) {
        tracing::info!(
            %id,
            activation_token = activation_token.is_some(),
            "a launch asked for the program"
        );
        if self.store.program(id).is_none() {
            cx.emit(WantsToBeSeen);
        }
        self.open_bringing_forward(id, activation_token, cx);
    }

    /// [`open`](Self::open), its windows coming forward with `activation_token`.
    fn open_bringing_forward(
        &mut self,
        id: &ProgramId,
        activation_token: Option<String>,
        cx: &mut Context<Library>,
    ) {
        let Some(component) = self.store.program(id) else {
            self.failed(
                format!("{id} is not installed, or its files cannot be read"),
                cx,
            );
            return;
        };
        self.opened(id, activation_token, cx);
        if let Some(tabs) = self.tabs.clone()
            && self.place_to_open(id) == ProgramPlace::Tab
        {
            self.open_in_tab(tabs, id, component, cx);
            return;
        }
        let is_an_app = self
            .store
            .manifest(id)
            .is_none_or(|manifest| manifest.kind() == ProgramKind::App);
        if self.programs_open == ProgramsOpen::AsChosen && is_an_app && !self.is_running(id) {
            let face = program_face(&self.store, id.as_str());
            cx.emit(FirstWindowWanted {
                id: id.clone(),
                face,
            });
            return;
        }
        let (runs, id) = (self.runs.clone(), id.clone());
        self.work_then(
            async move {
                runs.open(&id, component, ProgramPlace::Windows, Opening::ByTheUser)
                    .await
                    .map(drop)
            },
            Library::program_opened,
            cx,
        );
    }

    /// Opens the program `id`, whose component is at `component`, in a tab of `tabs`, whose screen
    /// its first request takes; where it has a tab already, that tab comes into sight.
    fn open_in_tab(
        &mut self,
        tabs: Entity<Tabs>,
        id: &ProgramId,
        component: PathBuf,
        cx: &mut Context<Library>,
    ) {
        match tabs.update(cx, |tabs, cx| tabs.open(id, cx)) {
            Ok(true) => {}
            Ok(false) => return,
            Err(why) => return self.failed(format!("{id} could not be opened: {why}"), cx),
        }
        let (runs, id) = (self.runs.clone(), id.clone());
        let opening = id.clone();
        self.work_then(
            async move {
                runs.open(&opening, component, ProgramPlace::Tab, Opening::ByTheUser)
                    .await
            },
            move |library, opened, cx| {
                let (started, opened) = match opened {
                    Ok(started) => (Some(started), Ok(())),
                    Err(error) => (None, Err(error)),
                };
                tabs.update(cx, |tabs, cx| tabs.started(&id, started, cx));
                library.program_opened(opened, cx);
            },
            cx,
        );
    }

    /// Opens `request`'s files with the program `id`, as `waos open` asks when it starts WaOS:
    /// handed to the program once it runs (`logic::opening`), and the program opened first.
    pub(crate) fn open_with(
        &mut self,
        id: &ProgramId,
        request: OpenRequest,
        cx: &mut Context<Library>,
    ) {
        self.opened(id, None, cx);
        let (runs, store, id) = (self.runs.clone(), self.store.clone(), id.clone());
        self.work_then(
            async move { opening::open_with(&runs, &store, &id, request).await },
            Library::program_opened,
            cx,
        );
    }

    /// Hands the click `activation` on a notification of the program `id`'s to the program, the
    /// one running or else the installed one, opened first, which hears it once it listens
    /// (`logic::opening`). A click for a program no longer installed is the library's problem,
    /// which tells the user so and asks to be seen.
    pub(crate) fn notification_activated(
        &mut self,
        id: &ProgramId,
        activation: Activation,
        cx: &mut Context<Library>,
    ) {
        self.opened(id, None, cx);
        let (runs, store, id) = (self.runs.clone(), self.store.clone(), id.clone());
        self.work_then(
            async move { opening::notification_activated(&runs, &store, &id, activation).await },
            Library::program_opened,
            cx,
        );
    }

    /// The program `id` is opened by the user: its windows come forward, with `activation_token`
    /// where the launch brought one, and it goes first among the recent programs on WaOS's icon.
    fn opened(
        &mut self,
        id: &ProgramId,
        activation_token: Option<String>,
        cx: &mut Context<Library>,
    ) {
        cx.emit(ProgramOpened {
            id: id.clone(),
            activation_token,
        });
        self.add_recent(id, cx);
    }

    /// What opening a program came to: one that could not be opened is the library's problem,
    /// and asks to be seen.
    fn program_opened(&mut self, opened: Result<()>, cx: &mut Context<Library>) {
        match opened {
            Ok(()) => self.succeeded(cx),
            Err(error) => {
                self.failed(format!("{error:#}"), cx);
                cx.emit(WantsToBeSeen);
            }
        }
    }

    /// Asks whether to remove the installed program `id`.
    pub(crate) fn ask_to_remove(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        let Some(program) = self.installed_program(id) else {
            return;
        };
        let name = program.manifest.name().to_owned();
        self.ask(Question::Remove {
            id: id.clone(),
            name,
        });
        cx.notify();
    }

    /// Adds the source `text` names, a GitHub repository or an index's URL, and reads what it
    /// lists; text that names no source is the library's problem.
    pub(crate) fn add_source(&mut self, text: &str, cx: &mut Context<Library>) {
        let source = match text.trim().parse::<Source>() {
            Ok(source) => source,
            Err(error) => return self.failed(format!("{error}"), cx),
        };
        if let Err(error) = self.store.add_source(source.clone()) {
            return self.failed(format!("{error}"), cx);
        }
        self.succeeded(cx);
        self.read_sources(cx);
    }

    /// Removes `source`, and what it listed with it.
    pub(crate) fn remove_source(&mut self, source: &Source, cx: &mut Context<Library>) {
        match self.store.remove_source(source) {
            Ok(()) => {
                self.sources.retain(|listing| &listing.source != source);
                self.succeeded(cx);
            }
            Err(error) => self.failed(format!("{error}"), cx),
        }
    }

    /// Reads again what every source lists: the sources the store keeps, each from its index.
    pub(crate) fn read_sources(&mut self, cx: &mut Context<Library>) {
        let sources = self.store.sources();
        self.sources = sources
            .iter()
            .map(|source| SourceListing {
                source: source.clone(),
                listing: Listing::Reading,
            })
            .collect();
        for source in sources {
            let store = self.store.clone();
            let read = source.clone();
            self.work_then(
                async move { store.read_source(&read).await },
                move |library, index, cx| {
                    let listing = match index {
                        Ok(index) => Listing::Read(offers_of(index.programs, &library.installed)),
                        Err(error) => Listing::Failed(format!("{error}")),
                    };
                    library.listing_read(&source, listing, cx);
                },
                cx,
            );
        }
        cx.notify();
    }

    /// Installs `program` as `source` lists it, a new program or an update of an installed one; a
    /// run of it ends once the package is downloaded, before the update replaces it.
    pub(crate) fn install_from(
        &mut self,
        source: &Source,
        program: &ListedProgram,
        cx: &mut Context<Library>,
    ) {
        if self.is_installing(&program.id) {
            return;
        }
        self.installing.push(program.id.clone());
        let first_install = self.installed_program(&program.id).is_none();
        self.remove_code(&program.id);
        let (store, source, listed) = (self.store.clone(), source.clone(), program.clone());
        let (runs, id) = (Arc::clone(&self.runs.runs), program.id.clone());
        self.work_then(
            async move {
                let download = store.download(&source, &listed).await?;
                // Once the package is here, so a failed download leaves the program running.
                runs.stop(&listed.id).await;
                store.install_download(download).await
            },
            move |library, installed, cx| {
                library.installing.retain(|installing| installing != &id);
                let succeeded = installed.is_ok();
                if succeeded {
                    // The tab of the run the update ended, where it had one.
                    library.close(&id, cx);
                }
                match installed {
                    Ok(_) => {
                        library.compile(&id);
                        library.succeeded(cx);
                    }
                    Err(error) => library.failed(format!("{error}"), cx),
                }
                library.installed_changed(cx);
                if succeeded && first_install {
                    library.offer_home_screen(&id, cx);
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Takes the files the system opened with WaOS: each package waits for the user to say
    /// whether to install it, and the library asks to be seen; a file that cannot be installed is
    /// the library's problem.
    pub(crate) fn open_files(&mut self, files: Vec<PathBuf>, cx: &mut Context<Library>) {
        tracing::info!(?files, "the system opened files with WaOS");
        for file in files {
            match OpenedPackage::open(&file) {
                Ok(package) => self.ask(Question::Install(Box::new(package))),
                Err(error) => {
                    self.package_copies.remove(&file);
                    self.problem = Some(format!("{error:#}"));
                }
            }
        }
        cx.emit(WantsToBeSeen);
        cx.notify();
    }

    /// Takes `documents`, files the system opened with WaOS without a path, as Android hands a
    /// document from its Files app: each is read whole, copied into a file of WaOS's own
    /// (`logic::package_copies`), and then asked about as a file opened with a path is; one that
    /// cannot be read or kept is the library's problem, and the library asks to be seen.
    pub(crate) fn open_documents(&mut self, documents: Vec<PickedFile>, cx: &mut Context<Library>) {
        for document in documents {
            let name = document_name(&document).unwrap_or_default();
            tracing::info!(name, "the system opened a document with WaOS");
            let reading = match document {
                // A document with a path is a file opened with WaOS like any other.
                PickedFile::Path(path) => {
                    self.open_files(vec![path], cx);
                    continue;
                }
                PickedFile::Handle(handle) => handle.read_all(),
                PickedFile::Stream(stream) => stream.read_all(),
            };
            let library = cx.weak_entity();
            // The task runs to its end whether or not its handle is kept.
            let _reading = cx.spawn(async move |cx| {
                let read = reading
                    .await
                    .with_context(|| format!("{name} could not be read"));
                cx.update(|app: &mut App| {
                    if let Some(library) = library.upgrade() {
                        library.update(app, |library, cx| library.document_read(name, read, cx));
                    }
                });
            });
        }
    }

    /// What reading the document `name` came to: its bytes are copied, off the main thread, and
    /// the copy asked about; why not, otherwise.
    fn document_read(&mut self, name: String, read: Result<Vec<u8>>, cx: &mut Context<Library>) {
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(error) => return self.document_failed(&error, cx),
        };
        let copies = self.package_copies.clone();
        self.work_then(
            async move { copies.write(&name, &bytes) },
            |library, copied, cx| match copied {
                Ok(copy) => library.open_files(vec![copy], cx),
                Err(error) => library.document_failed(&error, cx),
            },
            cx,
        );
    }

    fn document_failed(&mut self, error: &anyhow::Error, cx: &mut Context<Library>) {
        self.failed(format!("{error:#}"), cx);
        cx.emit(WantsToBeSeen);
    }

    /// The user's answer to the question asked now: yes installs the package, or removes the
    /// program; no leaves it. A package copied for the question is removed either way, once it is
    /// installed where yes. The next question, if any, is asked then.
    pub(crate) fn answer(&mut self, yes: bool, cx: &mut Context<Library>) {
        if self.questions.is_empty() {
            return;
        }
        match (self.questions.remove(0), yes) {
            (Question::Install(package), true) => self.install_opened(package, cx),
            (Question::Install(package), false) => self.package_copies.remove(package.file()),
            (Question::Remove { id, .. }, true) => self.remove(&id, cx),
            (Question::Remove { .. }, false) => {}
        }
        cx.notify();
    }

    /// Installs `package`, opened with WaOS, once no run of its program is left: a program that
    /// runs, updated so, ends first, its tab closing, as a system ends an app it updates.
    fn install_opened(&mut self, package: Box<OpenedPackage>, cx: &mut Context<Library>) {
        tracing::info!(file = %package.file().display(), "the user said to install");
        let id = package.manifest().id().clone();
        self.close(&id, cx);
        self.once_ended(
            &id,
            move |library, cx| library.put_in_place(package, cx),
            cx,
        );
    }

    /// Installs `package`, its copy removed after, and offers its program a place on the home
    /// screen where it is new.
    fn put_in_place(&mut self, package: Box<OpenedPackage>, cx: &mut Context<Library>) {
        let (id, copy) = (
            package.manifest().id().clone(),
            package.file().to_path_buf(),
        );
        let first_install = self.installed_program(&id).is_none();
        self.remove_code(&id);
        let installed = self
            .store
            .install(package.into_package())
            .map(|installed| self.compile(installed.manifest.id()));
        self.package_copies.remove(&copy);
        let succeeded = installed.is_ok();
        match installed {
            Ok(()) => self.succeeded(cx),
            Err(error) => self.failed(format!("{error}"), cx),
        }
        self.installed_changed(cx);
        if succeeded && first_install {
            self.offer_home_screen(&id, cx);
        }
    }

    /// Removes the installed program `id` once no run of it is left: one that runs ends first,
    /// its tab closing and its windows with it, as a system ends an app it uninstalls, so no run
    /// goes on over the folders the removal takes.
    fn remove(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        self.close(id, cx);
        let id = id.clone();
        self.once_ended(
            &id.clone(),
            move |library, cx| library.take_out(&id, cx),
            cx,
        );
    }

    /// Takes the program `id` out of the store, with its code and what the system kept for its
    /// app.
    fn take_out(&mut self, id: &ProgramId, cx: &mut Context<Library>) {
        self.remove_code(id);
        let removed = self.store.remove(id);
        self.notifications_refused.remove(id);
        match removed {
            Ok(()) => self.succeeded(cx),
            Err(error) => self.failed(format!("{error}"), cx),
        }
        self.installed_changed(cx);
    }

    /// Does `then` once no run of the program `id` is left: at once where none runs or is being
    /// started, and otherwise once its run is ended, on the library's runtime.
    fn once_ended(
        &mut self,
        id: &ProgramId,
        then: impl FnOnce(&mut Library, &mut Context<Library>) + 'static,
        cx: &mut Context<Library>,
    ) {
        if !self.runs.runs.runs_or_starts(id) {
            return then(self, cx);
        }
        let (runs, id) = (Arc::clone(&self.runs.runs), id.clone());
        self.work_then(
            async move { runs.stop(&id).await },
            move |library, (), cx| then(library, cx),
            cx,
        );
    }

    /// Starts compiling the code of the program `id`, installed or updated just now, off the main
    /// thread; until it is ready, the program says so (`is_compiling`). One whose code cannot be
    /// compiled is said to whoever subscribes, and its open compiles it again and says why not.
    fn compile(&self, id: &ProgramId) {
        let Some(component) = self.store.program(id) else {
            return;
        };
        if let Err(error) = self.runs.code.start(id, &component) {
            tracing::warn!(%id, "the program's code cannot be compiled: {error:#}");
        }
    }

    /// Removes the code compiled from the version of the program `id` installed now, which is
    /// about to be removed or replaced by an update, while its component is still there to find
    /// it by; a run of that version keeps the code it mapped. An update that then fails leaves the
    /// version installed to be compiled again at its next open. Code that cannot be removed
    /// stays, which is said to whoever subscribes.
    fn remove_code(&self, id: &ProgramId) {
        let Some(component) = self.store.program(id) else {
            return;
        };
        if let Err(error) = self.runs.code.remove(&component) {
            tracing::warn!(%id, "the program's code stays: {error:#}");
        }
    }

    fn ask(&mut self, question: Question) {
        self.questions.push(question);
    }

    /// What `source` lists is `listing` now, unless the source was removed meanwhile.
    fn listing_read(&mut self, source: &Source, listing: Listing, cx: &mut Context<Library>) {
        if let Some(read) = self.sources.iter_mut().find(|read| &read.source == source) {
            read.listing = listing;
            cx.notify();
        }
    }

    /// The store changed what is installed: read it again, and what each listing offers.
    fn installed_changed(&mut self, cx: &mut Context<Library>) {
        self.installed = self.store.installed();
        self.icons = icons_of(&self.store, &self.installed);
        cx.emit(InstalledChanged);
        self.match_shortcuts(cx);
        for read in &mut self.sources {
            if let Listing::Read(offers) = &mut read.listing {
                let listed = offers.drain(..).map(|(program, _)| program).collect();
                *offers = offers_of(listed, &self.installed);
            }
        }
        cx.notify();
    }

    fn installed_program(&self, id: &ProgramId) -> Option<&InstalledProgram> {
        self.installed
            .iter()
            .find(|program| program.manifest.id() == id)
    }

    fn succeeded(&mut self, cx: &mut Context<Library>) {
        self.problem = None;
        cx.notify();
    }

    /// Whether WaOS is kept in the Dock or the taskbar; none where WaOS is no desktop's launcher,
    /// as where programs do not open in windows of their own, or keeps no preferences.
    pub(crate) fn kept_in_dock(&self) -> Option<bool> {
        if self.programs_open != ProgramsOpen::InWindows {
            return None;
        }
        let preferences = self.preferences.as_ref()?;
        Some(preferences.chosen().kept_in_dock)
    }

    /// WaOS is kept in the Dock or the taskbar from now on, or else has only an icon in the menu
    /// bar or the tray, as `kept` says, kept in WaOS's preferences; where it cannot be kept,
    /// nothing changes, and that is the library's problem.
    pub(crate) fn keep_in_dock(&mut self, kept: bool, cx: &mut Context<Library>) {
        if self.kept_in_dock().is_some() {
            self.choose_preferences(|chosen| chosen.kept_in_dock = kept, cx);
        }
    }

    /// WaOS's own preferences as `change` makes them, once they are kept; where they cannot be,
    /// nothing changes, and that is the library's problem.
    fn choose_preferences(
        &mut self,
        change: impl FnOnce(&mut Preferences),
        cx: &mut Context<Library>,
    ) {
        let Some(preferences) = &mut self.preferences else {
            return;
        };
        match preferences.choose(change) {
            Ok(()) => {
                self.succeeded(cx);
                cx.emit(PreferencesChosen);
            }
            Err(error) => self.failed(format!("{error:#}"), cx),
        }
    }

    /// No first window opened for the program `id`, which is not started: the library's problem.
    pub(crate) fn first_window_refused(
        &mut self,
        id: &ProgramId,
        why: &str,
        cx: &mut Context<Library>,
    ) {
        self.failed(format!("{id} could not be opened: {why}"), cx);
    }

    fn failed(&mut self, why: String, cx: &mut Context<Library>) {
        tracing::info!(%why, "the library could not do what was asked");
        self.problem = Some(why);
        cx.notify();
    }

    /// The library's runtime, which reads, downloads and loads, for work of WaOS's that outlives
    /// whoever asked for it.
    pub(crate) fn runtime(&self) -> tokio::runtime::Handle {
        self.work.handle().clone()
    }

    /// Runs `work` on the library's runtime, and hands what it answers to `then` on the main
    /// thread once it is done; a library gone by then hears nothing.
    fn work_then<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
        then: impl FnOnce(&mut Library, T, &mut Context<Library>) + 'static,
        cx: &mut Context<Library>,
    ) {
        let working = self.work.spawn(work);
        let library = cx.weak_entity();
        // The task runs to its end whether or not its handle is kept.
        let _done = cx.spawn(async move |cx| {
            let answer = match working.await {
                Ok(answer) => answer,
                Err(error) => {
                    tracing::warn!(%error, "the library's work ended without an answer");
                    return;
                }
            };
            cx.update(|app: &mut App| {
                if let Some(library) = library.upgrade() {
                    library.update(app, |library, cx| then(library, answer, cx));
                }
            });
        });
    }
}

/// From now on, the files the system opens with WaOS go to `library`, and so does what each of
/// `later_launches` of WaOS asks, read by WaOS's command line; those opened before, as at launch,
/// go there now. WaOS opens no links of its own, and the system hands it every file by its path.
pub(crate) fn take_files_opened_with_waos(
    app: &mut App,
    library: &Entity<Library>,
    later_launches: Option<LaterLaunches>,
) {
    app.set_open_requested_handler(Some(opened_with_waos(library)));
    if let Some(later_launches) = later_launches {
        hear_later_launches(app, later_launches, library);
    }
}

/// What takes a request to open files with WaOS: `library`, which asks to be seen, and asks
/// whether to install each package; a launch with nothing to open only shows it.
fn opened_with_waos(library: &Entity<Library>) -> OpenRequestedCallback {
    let library = library.downgrade();
    Rc::new(move |app, request: ui::OpenRequest| {
        let Some(library) = library.upgrade() else {
            return;
        };
        if !request.links.is_empty() {
            tracing::info!(links = ?request.links, "links opened with WaOS, which opens none");
        }
        let opened = OpenedFiles::of(request.files);
        library.update(app, |library, cx| {
            // A launch with nothing to open still shows the library, as open_files asks.
            if !opened.paths.is_empty() || opened.documents.is_empty() {
                library.open_files(opened.paths, cx);
            }
            library.open_documents(opened.documents, cx);
        });
    })
}

/// Does what each later launch of WaOS asks, for as long as WaOS runs, which holds its claim to be
/// the one instance meanwhile.
fn hear_later_launches(
    app: &mut App,
    mut later_launches: LaterLaunches,
    library: &Entity<Library>,
) {
    let library = library.downgrade();
    app.spawn(async move |cx| {
        loop {
            let launch = later_launches.next_launch().await;
            cx.update(|app| carry_out_launch(app, launch, &library));
        }
    });
}

/// Does what `launch` asks, read by WaOS's command line in the folder it was started in: its files
/// opened with WaOS, as the system's own requests are, or with the program it names in `library`,
/// or that program opened or brought forward with the launch's activation token; any other launch
/// shows WaOS, as a launch with nothing to open does.
fn carry_out_launch(app: &mut App, launch: Launch, library: &WeakEntity<Library>) {
    let asked = command(launch.arguments).map(|asked| asked.in_folder(&launch.current_dir));
    match asked {
        Ok(Command::OpenWithWaos(files)) => app.open_requested(request_to_open_with_waos(files)),
        Ok(Command::Open { id, files }) => {
            let request = OpenRequest {
                files,
                links: Vec::new(),
            };
            if let Some(library) = library.upgrade() {
                library.update(app, |library, cx| library.open_with(&id, request, cx));
            }
        }
        Ok(Command::ShowProgram { id }) => {
            if let Some(library) = library.upgrade() {
                let token = launch.activation_token;
                library.update(app, |library, cx| library.show_program(&id, token, cx));
            }
        }
        Ok(Command::Show(_)) => app.open_requested(ui::OpenRequest::default()),
        Err(usage) => {
            tracing::warn!(usage, "a later launch of WaOS asked for nothing it takes");
            app.open_requested(ui::OpenRequest::default());
        }
    }
}

/// A request to open `files`, by their paths on the machine, with WaOS.
pub(crate) fn request_to_open_with_waos(files: Vec<PathBuf>) -> ui::OpenRequest {
    ui::OpenRequest {
        files: files.into_iter().map(PickedFile::Path).collect(),
        links: Vec::new(),
    }
}

/// The files the system hands WaOS, opened with it or picked: files by their paths on the
/// machine, as the desktop systems and iOS give them, and documents without one, as Android gives
/// a `content:` document.
struct OpenedFiles {
    paths: Vec<PathBuf>,
    documents: Vec<PickedFile>,
}

impl OpenedFiles {
    /// `files`, by whether the system gave each a path.
    fn of(files: Vec<PickedFile>) -> OpenedFiles {
        let (mut paths, mut documents) = (Vec::new(), Vec::new());
        for file in files {
            match file {
                PickedFile::Path(path) => paths.push(path),
                document @ (PickedFile::Handle(_) | PickedFile::Stream(_)) => {
                    documents.push(document);
                }
            }
        }
        OpenedFiles { paths, documents }
    }
}

/// From now on, a click on a program's notification goes to the program its id names, through
/// `library`: one on a notification WaOS posted as itself, the click that started WaOS among them,
/// waiting until now, and each arriving on `posted_as_programs`, those WaOS posted as the
/// programs themselves.
pub(crate) fn take_notification_clicks(
    app: &mut App,
    library: &Entity<Library>,
    posted_as_programs: Option<UnboundedReceiver<NotificationActivation>>,
) {
    let library = library.downgrade();
    let own = library.clone();
    app.set_notification_activated_handler(Some(Rc::new(move |app, activation| {
        hand_click_to_program(app, &own, activation);
    })));
    if let Some(mut clicks) = posted_as_programs {
        app.spawn(async move |cx| {
            while let Some(activation) = clicks.next().await {
                cx.update(|app| hand_click_to_program(app, &library, activation));
            }
        });
    }
}

/// Hands the click `activation` to the program its notification's id names, through `library`.
fn hand_click_to_program(
    app: &mut App,
    library: &WeakEntity<Library>,
    activation: NotificationActivation,
) {
    let Some((id, activation)) = program_activation(activation) else {
        tracing::info!("a click on a notification of no program's: left");
        return;
    };
    if let Some(library) = library.upgrade() {
        library.update(app, |library, cx| {
            library.notification_activated(&id, activation, cx)
        });
    }
}

/// Hands each change `changes` tells from now on to `changed`, on the main thread, for as long as
/// the library lives and `changes` has a sender.
fn follow<T: Clone + Send + Sync + 'static>(
    mut changes: watch::Receiver<T>,
    changed: impl Fn(&mut Library, T, &mut Context<Library>) + 'static,
    cx: &mut Context<Library>,
) {
    // The task runs to its end, with the library or the sender, whether or not its handle is
    // kept.
    let library = cx.weak_entity();
    let _following = cx.spawn(async move |cx| {
        while changes.changed().await.is_ok() {
            let now = changes.borrow_and_update().clone();
            let Some(library) = library.upgrade() else {
                return;
            };
            cx.update(|app: &mut App| library.update(app, |library, cx| changed(library, now, cx)));
        }
    });
}

/// The icon of each of `installed`, as `store` keeps it; one that cannot be read is left out, and
/// said.
fn icons_of(store: &Store, installed: &[InstalledProgram]) -> HashMap<ProgramId, Arc<[u8]>> {
    installed
        .iter()
        .filter_map(|program| {
            let id = program.manifest.id();
            let read = store.icon(id).map(std::fs::read);
            match read {
                Some(Ok(png)) => Some((id.clone(), Arc::from(png))),
                Some(Err(error)) => {
                    tracing::warn!(%id, %error, "its icon cannot be read");
                    None
                }
                None => None,
            }
        })
        .collect()
}

/// Each of `listed` with what it offers against `installed`.
fn offers_of(
    listed: Vec<ListedProgram>,
    installed: &[InstalledProgram],
) -> Vec<(ListedProgram, Offer)> {
    listed
        .into_iter()
        .map(|program| {
            let offer = offer_of(&program, installed);
            (program, offer)
        })
        .collect()
}
