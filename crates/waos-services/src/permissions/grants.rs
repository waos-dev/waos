//! One app's grants, as its run reads them ([`AppGrants`]): its own settings over the global ones,
//! read live; a question the run waits for asked of its user once (`asking`), the answer kept in
//! its settings.
//!
//! The invariant: settings that cannot be read grant nothing. An app whose own settings or the
//! global ones cannot be read has every switch off, nothing mounted, no machine, every question,
//! link scheme and role denied, and no answer set for a picker or a dialog, which then asks,
//! rather than more than the user gave it, and the failure is said to whoever subscribes.

use std::path::PathBuf;
use std::sync::Arc;

use waos_capabilities::{
    Answer, AnswerFuture, ChangedFuture, DomainRule, FileDialog, FileSystemGrant, Mount,
    NetworkGrant, ProgramGrants, Question, Role, Setting, SettingsClosedFuture, SimulatedSound,
    Switch,
};
use waos_package::ProgramId;

use crate::permissions::AppQuestion;
use crate::permissions::service::Shared;
use crate::permissions::settings::{AppSettings, GlobalSettings, Holding};

/// One app's grants: its own settings over the global ones, read live.
pub(crate) struct AppGrants {
    pub(crate) shared: Arc<Shared>,
    pub(crate) app: ProgramId,
}

impl AppGrants {
    /// What `read` reads of what holds for the app, or `nothing` when its settings cannot be
    /// read.
    fn read<T>(&self, read: impl FnOnce(&Holding<'_>) -> T, nothing: T) -> T {
        match self.shared.both(&self.app) {
            Ok((global, own)) => read(&Holding {
                global: &global,
                own: &own,
            }),
            Err(error) => {
                tracing::warn!(app = %self.app, "an app is granted nothing: {error}");
                nothing
            }
        }
    }

    /// Changes the app's own settings as `change` does, given the global ones; a change that
    /// cannot be kept is said, as `lost` puts what it loses.
    fn keep(&self, lost: &str, change: impl FnOnce(&GlobalSettings, &mut AppSettings)) {
        let kept = self.shared.change_app_with_global(&self.app, change);
        if let Err(error) = kept {
            tracing::warn!(app = %self.app, "{lost}: {error}");
        }
    }
}

impl ProgramGrants for AppGrants {
    fn network(&self) -> NetworkGrant {
        self.read(|holding| holding.network(), nothing_network())
    }

    fn local_network(&self) -> bool {
        self.read(|holding| holding.local_network(), false)
    }

    fn clipboard_read(&self) -> bool {
        self.read(|holding| holding.clipboard_read(), false)
    }

    fn clipboard_write(&self) -> bool {
        self.read(|holding| holding.clipboard_write(), false)
    }

    fn file_system(&self) -> FileSystemGrant {
        self.read(|holding| holding.file_system(), FileSystemGrant::default())
    }

    fn answer(&self, question: Question) -> Option<Answer> {
        self.read(|holding| holding.answer(question), Some(Answer::Denied))
    }

    fn ask(&self, question: Question) -> AnswerFuture {
        self.shared
            .ask(&self.app, AppQuestion::Permission(question))
    }

    fn link_scheme_answer(&self, scheme: &str) -> Option<Answer> {
        self.read(
            |holding| holding.link_scheme_answer(scheme),
            Some(Answer::Denied),
        )
    }

    fn ask_to_open_links(&self, scheme: &str) -> AnswerFuture {
        let question = AppQuestion::LinkScheme(scheme.to_owned());
        self.shared.ask(&self.app, question)
    }

    fn open_settings(&self, switch: Switch) -> SettingsClosedFuture {
        self.shared.open_settings(&self.app, switch)
    }

    /// Off where the settings cannot be read, as nothing granted keeps the least.
    fn keeps_record(&self) -> bool {
        self.read(|holding| holding.keeps_record(), false)
    }

    fn role(&self, role: Role) -> Option<Answer> {
        self.read(|holding| holding.role(role), Some(Answer::Denied))
    }

    fn ask_for_role(&self, role: Role) -> AnswerFuture {
        self.shared.ask(&self.app, AppQuestion::Role(role))
    }

    fn machines(&self) -> Vec<String> {
        self.read(|holding| holding.machines(), Vec::new())
    }

    /// The machine joins the app's own, unless it is among them.
    fn keep_machine(&self, id: &str) {
        let lost = format!("the machine {id} picked is kept for the run alone");
        self.keep(&lost, |_, own| {
            if !own.machines.iter().any(|kept| kept == id) {
                own.machines.push(id.to_owned());
            }
        });
    }

    /// The pick joins the app's own mounts, in place of one at its path: an app following the
    /// mounts for every app has them as its own from then on, as a change the user makes on its
    /// page does.
    fn keep_picked(&self, mount: Mount) {
        self.keep("a pick is kept for the run alone", |global, own| {
            let mut mounts = own.file_system.or(&global.file_system).clone();
            mounts.mounts.retain(|kept| kept.at != mount.at);
            mounts.mounts.push(mount);
            own.file_system = Setting::Own(mounts);
        });
    }

    fn machine_answer(&self) -> Option<String> {
        self.read(|holding| holding.answer_with().machine.clone(), None)
    }

    fn audio_output_answer(&self) -> Option<String> {
        self.read(|holding| holding.answer_with().audio_output.clone(), None)
    }

    fn file_dialog_answer(&self, dialog: FileDialog) -> Option<Vec<PathBuf>> {
        self.read(|holding| holding.answer_with().file_dialog(dialog), None)
    }

    /// Silence where the settings cannot be read, as nothing granted is quietest.
    fn simulated_microphone(&self) -> SimulatedSound {
        self.read(
            |holding| holding.simulated_microphone(),
            SimulatedSound::Silence,
        )
    }

    fn changed(&self) -> ChangedFuture {
        self.shared.changed()
    }
}

fn nothing_network() -> NetworkGrant {
    NetworkGrant {
        on: false,
        domains: DomainRule::Only(Vec::new()),
    }
}
