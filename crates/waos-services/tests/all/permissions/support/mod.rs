//! What Permissions' tests stand in with: the package manager's removals, played by the test.

#![allow(dead_code, reason = "each test crate uses its own part of the support")]

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::channel::oneshot;
use tokio::sync::{mpsc, watch};
use waos_capabilities::ChangedFuture;
use waos_package::ProgramId;
use waos_services::packages::InstalledApps;
use waos_services::permissions::{
    Answer, AnswerFuture, App, Prompts, Question, Role, SettingsClosedFuture, Switch,
};

pub fn triangle() -> ProgramId {
    "dev.example.triangle".parse().unwrap()
}

/// The package manager's removals, as a test begins and ends them.
#[derive(Default)]
pub struct Removals {
    removing: watch::Sender<BTreeSet<ProgramId>>,
    listeners: Mutex<Vec<mpsc::UnboundedSender<ProgramId>>>,
}

impl Removals {
    /// The removal of `app` begins.
    pub fn begin(&self, app: &ProgramId) {
        self.removing.send_modify(|removing| {
            removing.insert(app.clone());
        });
    }

    /// `app` is out of the record: its removal is told, and ends.
    pub fn end(&self, app: &ProgramId) {
        for listener in self.listeners.lock().unwrap().iter() {
            let _ = listener.send(app.clone());
        }
        self.removing.send_modify(|removing| {
            removing.remove(app);
        });
    }
}

impl InstalledApps for Removals {
    fn name(&self, _id: &ProgramId) -> Option<String> {
        None
    }

    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.removing.subscribe()
    }

    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        let (sender, removals) = mpsc::unbounded_channel();
        self.listeners.lock().unwrap().push(sender);
        removals
    }
}

/// Whether `changed` resolves within a moment.
pub fn heard(mut changed: ChangedFuture) -> bool {
    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        if changed.as_mut().now_or_never().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// The user, as a test plays them: what is asked of them, kept until the test answers it, and each
/// settings page opened, until the test leaves it.
#[derive(Default)]
pub struct TestPrompts {
    asked: Mutex<Vec<Put>>,
    pages: Mutex<Vec<Page>>,
}

/// One settings page opened: the app's name, its switch, and its leaving while it is open.
type Page = (String, Switch, Option<oneshot::Sender<()>>);

/// One thing asked of the user, and its answer while it waits.
struct Put {
    app: String,
    question: String,
    answer: Option<oneshot::Sender<Option<Answer>>>,
}

impl TestPrompts {
    /// What was asked so far, by the app's name, in order.
    pub fn asked(&self) -> Vec<(String, String)> {
        let asked = self.asked.lock().unwrap();
        asked
            .iter()
            .map(|put| (put.app.clone(), put.question.clone()))
            .collect()
    }

    /// The user answers everything waiting with `answer`.
    pub fn answer(&self, answer: Option<Answer>) {
        for put in self.asked.lock().unwrap().iter_mut() {
            if let Some(waiting) = put.answer.take() {
                let _ = waiting.send(answer);
            }
        }
    }

    /// Whether the `index`th thing asked was put away without its answer.
    pub fn put_away(&self, index: usize) -> bool {
        let asked = self.asked.lock().unwrap();
        asked[index]
            .answer
            .as_ref()
            .is_some_and(oneshot::Sender::is_canceled)
    }

    /// The settings pages opened so far, by the app's name.
    pub fn pages(&self) -> Vec<(String, Switch)> {
        let pages = self.pages.lock().unwrap();
        pages
            .iter()
            .map(|(app, switch, _)| (app.clone(), *switch))
            .collect()
    }

    /// The user leaves every page open.
    pub fn leave_pages(&self) {
        for (_, _, leave) in self.pages.lock().unwrap().iter_mut() {
            if let Some(leave) = leave.take() {
                let _ = leave.send(());
            }
        }
    }

    fn put(&self, app: &App, question: String) -> AnswerFuture {
        let (answer, answered) = oneshot::channel();
        self.asked.lock().unwrap().push(Put {
            app: app.name.clone(),
            question,
            answer: Some(answer),
        });
        Box::pin(answered.map(|answer| answer.unwrap_or(None)))
    }
}

impl Prompts for TestPrompts {
    fn ask(&self, app: &App, question: Question) -> AnswerFuture {
        self.put(app, format!("{question:?}"))
    }

    fn ask_to_open_links(&self, app: &App, scheme: &str) -> AnswerFuture {
        self.put(app, format!("links of {scheme}"))
    }

    fn ask_for_role(&self, app: &App, role: Role) -> AnswerFuture {
        self.put(app, format!("the {role:?} role"))
    }

    fn open_settings(&self, app: &App, switch: Switch) -> SettingsClosedFuture {
        let (leave, left) = oneshot::channel();
        let page = (app.name.clone(), switch, Some(leave));
        self.pages.lock().unwrap().push(page);
        Box::pin(left.map(drop))
    }
}

/// Apps installed by name, none removed.
pub struct Named(pub &'static [(&'static str, &'static str)]);

impl InstalledApps for Named {
    fn name(&self, id: &ProgramId) -> Option<String> {
        let named = self.0.iter().find(|(named, _)| *named == id.as_str());
        named.map(|(_, name)| (*name).to_owned())
    }

    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        watch::channel(BTreeSet::new()).1
    }

    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        mpsc::unbounded_channel().1
    }
}
