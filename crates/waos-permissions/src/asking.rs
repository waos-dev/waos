//! The questions being asked of the user now ([`AskedQuestion`]), each asked once however many wait
//! for it: the answer kept, or else the user's, asked through the prompts and kept, raced against a
//! change of the settings, which answers a question waiting and puts its prompt away. A switch's
//! page opened for the user is listed while it is open.
//!
//! The invariants: a question is listed exactly while it is asked, by a guard its asking holds;
//! a question asked again while it is asked waits for the one answer, and one asked once its
//! answer is kept reads it, so nothing is asked twice; and a question nobody waits for any longer
//! is put away, since only the waits hold it.

use std::future::ready;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use futures::future::{BoxFuture, Either, FutureExt, Shared as SharedFuture, WeakShared, select};
use waos_capabilities::{
    Answer, AnswerFuture, Question, Role, Setting, SettingsClosedFuture, Switch,
};
use waos_package::ProgramId;

use crate::permissions::Shared;
use crate::prompts::App;
use crate::settings::Holding;
use crate::settings_store::locked;

/// Something asked of the user for an app now.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AskedQuestion {
    pub app: ProgramId,
    pub question: AppQuestion,
}

/// What is asked of the user for an app.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AppQuestion {
    /// A permission's question, at the app's first use.
    Permission(Question),
    /// Whether it may open links of this scheme, in lower case.
    LinkScheme(String),
    /// Whether it may hold this role, at its start.
    Role(Role),
    /// To turn this switch on, on the app's settings page, opened for the user.
    Switch(Switch),
}

/// A question being asked: its answer, which every wait for it shares; none when nobody answered.
pub(crate) type Answering = SharedFuture<BoxFuture<'static, Option<Answer>>>;

/// A question being asked, as the list of them holds it: by its serial, and weakly, so the waits
/// alone keep it asked.
pub(crate) struct Asking {
    serial: u64,
    answering: WeakShared<BoxFuture<'static, Option<Answer>>>,
}

impl Shared {
    /// The answer to `question` for `app`: the one kept, or else the one being asked, or else the
    /// user's, asked now.
    pub(crate) fn ask(self: &Arc<Shared>, app: &ProgramId, question: AppQuestion) -> AnswerFuture {
        let asked = AskedQuestion {
            app: app.clone(),
            question,
        };
        if let Some(answer) = self.kept(&asked) {
            return Box::pin(ready(Some(answer)));
        }
        let mut asking = locked(&self.asking);
        if let Some(answering) = asking
            .get(&asked)
            .and_then(|asking| asking.answering.upgrade())
        {
            return Box::pin(answering);
        }
        let serial = self.serials.fetch_add(1, Ordering::Relaxed);
        let answering: Answering = Arc::clone(self)
            .answered(asked.clone(), serial)
            .boxed()
            .shared();
        let weak = answering
            .downgrade()
            .expect("a question not yet asked has no answer");
        asking.insert(
            asked,
            Asking {
                serial,
                answering: weak,
            },
        );
        Box::pin(answering)
    }

    /// Opens `switch`'s page in `app`'s settings for the user, listed while it is open.
    pub(crate) fn open_settings(
        self: &Arc<Shared>,
        app: &ProgramId,
        switch: Switch,
    ) -> SettingsClosedFuture {
        let asked = AskedQuestion {
            app: app.clone(),
            question: AppQuestion::Switch(switch),
        };
        let serial = self.serials.fetch_add(1, Ordering::Relaxed);
        let listed = Listed::new(self, asked, serial);
        let open = self.prompts.open_settings(&self.app(app), switch);
        Box::pin(async move {
            open.await;
            drop(listed);
        })
    }

    /// `asked`, asked of the user and listed until it is answered or put away; its answer kept
    /// when the user gives it, and read when the settings give it first.
    async fn answered(self: Arc<Shared>, asked: AskedQuestion, serial: u64) -> Option<Answer> {
        let _listed = Listed::new(&self, asked.clone(), serial);
        let shown = || self.show(&asked);
        match self.settled_or_shown(&asked, shown).await {
            Either::Left(kept) => Some(kept),
            Either::Right(Some(answer)) => {
                self.keep(&asked, answer);
                Some(answer)
            }
            Either::Right(None) => None,
        }
    }

    /// The answer the settings give `asked` without calling `show`, or else `show`'s prompt raced
    /// against a change of the settings that answers it, which puts the prompt away.
    async fn settled_or_shown(
        &self,
        asked: &AskedQuestion,
        show: impl FnOnce() -> AnswerFuture,
    ) -> Either<Answer, Option<Answer>> {
        let mut settled = pin!(self.settled(asked));
        if let Some(kept) = settled.as_mut().now_or_never() {
            return Either::Left(kept);
        }
        match select(settled, show()).await {
            Either::Left((kept, _shown)) => Either::Left(kept),
            Either::Right((answer, _settled)) => Either::Right(answer),
        }
    }

    /// Resolves once the settings give `asked` an answer, the next change asked for before each
    /// read so none between the two is missed.
    async fn settled(&self, asked: &AskedQuestion) -> Answer {
        loop {
            let changed = self.changed();
            if let Some(kept) = self.kept(asked) {
                return kept;
            }
            changed.await;
        }
    }

    /// The prompt that asks `asked` of the user.
    fn show(&self, asked: &AskedQuestion) -> AnswerFuture {
        let app = self.app(&asked.app);
        match &asked.question {
            AppQuestion::Permission(question) => self.prompts.ask(&app, *question),
            AppQuestion::LinkScheme(scheme) => self.prompts.ask_to_open_links(&app, scheme),
            AppQuestion::Role(role) => self.prompts.ask_for_role(&app, *role),
            AppQuestion::Switch(_) => unreachable!("a switch's page is opened, not answered"),
        }
    }

    /// The answer the settings keep to `asked`; denied where they cannot be read, as nothing
    /// granted is the least.
    fn kept(&self, asked: &AskedQuestion) -> Option<Answer> {
        let (global, own) = match self.both(&asked.app) {
            Ok(both) => both,
            Err(error) => {
                tracing::warn!(app = %asked.app, "a question is denied: {error}");
                return Some(Answer::Denied);
            }
        };
        let holding = Holding {
            global: &global,
            own: &own,
        };
        match &asked.question {
            AppQuestion::Permission(question) => holding.answer(*question),
            AppQuestion::LinkScheme(scheme) => holding.link_scheme_answer(scheme),
            AppQuestion::Role(role) => holding.role(*role),
            AppQuestion::Switch(_) => None,
        }
    }

    /// Keeps `answer`, the user's to `asked`, among the app's own settings.
    fn keep(&self, asked: &AskedQuestion, answer: Answer) {
        let kept = self.change_app(&asked.app, |own| match &asked.question {
            AppQuestion::Permission(question) => own.answers.set(*question, Setting::Own(answer)),
            AppQuestion::LinkScheme(scheme) => {
                own.link_schemes.insert(scheme.clone(), answer);
            }
            AppQuestion::Role(role) => {
                own.roles.insert(*role, answer);
            }
            AppQuestion::Switch(_) => {}
        });
        if let Err(error) = kept {
            tracing::warn!(app = %asked.app, question = ?asked.question, "an answer is lost: {error}");
        }
    }

    /// `id`, as the prompts say it to its user.
    fn app(&self, id: &ProgramId) -> App {
        App {
            id: id.clone(),
            name: self.installed.name(id).unwrap_or_else(|| id.to_string()),
        }
    }
}

/// One question in the list of those asked now, until it is dropped.
struct Listed {
    shared: Arc<Shared>,
    asked: AskedQuestion,
    serial: u64,
}

impl Listed {
    fn new(shared: &Arc<Shared>, asked: AskedQuestion, serial: u64) -> Listed {
        shared.asked.send_modify(|list| list.push(asked.clone()));
        Listed {
            shared: Arc::clone(shared),
            asked,
            serial,
        }
    }
}

impl Drop for Listed {
    fn drop(&mut self) {
        let mut asking = locked(&self.shared.asking);
        if asking
            .get(&self.asked)
            .is_some_and(|asking| asking.serial == self.serial)
        {
            asking.remove(&self.asked);
        }
        drop(asking);
        self.shared.asked.send_modify(|list| {
            if let Some(index) = list.iter().position(|listed| *listed == self.asked) {
                list.remove(index);
            }
        });
    }
}
