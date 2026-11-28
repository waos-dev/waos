//! The programs' openings, over Waft's opener: the engine asks from a program's thread once its
//! rules allow what is opened, a file by where it is on the machine; each opening is posted to the
//! App's thread (`app_thread_bound`), where a link is opened in the app the system picks for its
//! scheme, a file in the app it picks for its type or shown in its file manager, and answered with
//! whether the system handed it on. The opener is the engine's shell's only where Waft's platform
//! has one.
//!
//! The invariant: every opening asked for is answered, at once where it cannot reach the App's
//! thread, so no program waits for an answer that cannot come.

use std::path::Path;
use std::sync::Arc;

use futures::FutureExt;
use waft::App;
use waft::ui::{self, Platform, PlatformRef};
use waos::bindings::wasdk::platform::opener::OpenError;
use waos::{AppInstance, OpenFuture, ShellOpener};

use super::app_thread_bound::{AppThreadBound, AppThreadCalls};

/// Waft's opener, served to the engine as its shell's.
pub(crate) struct Opener {
    shell: OpenerOnRequest,
    calls: AppThreadCalls<PlatformRef>,
}

impl Opener {
    /// Waft's opener on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<Opener> {
        platform.opener()?;
        let (platform, calls) = AppThreadBound::new();
        let shell = OpenerOnRequest { platform };
        Some(Opener { shell, calls })
    }

    /// The opener as the engine's shell has it: each opening is posted to the App's thread, where
    /// [`serve`](Self::serve) runs it.
    pub(crate) fn shell(&self) -> Arc<dyn ShellOpener> {
        Arc::new(self.shell.clone())
    }

    /// From now on, each opening a program asks for is carried out through Waft's opener.
    pub(crate) fn serve(self, app: &mut App) {
        self.calls.serve(app, app.platform());
    }
}

/// The app's opener, as the engine reaches it: every opening is posted to the App's thread.
#[derive(Clone)]
struct OpenerOnRequest {
    platform: AppThreadBound<PlatformRef>,
}

impl OpenerOnRequest {
    /// Hands `opening` to Waft's opener on the App's thread, and answers whether the system handed
    /// it on.
    fn ask(
        &self,
        opening: impl FnOnce(&dyn ui::Opener) -> ui::OpenFuture + Send + 'static,
    ) -> OpenFuture {
        let opened = self.platform.ask(move |platform| {
            let opening = match platform.opener() {
                Some(opener) => opening(opener),
                None => Box::pin(std::future::ready(Err(ui::OpenError::Failed(
                    "the system has no opener".to_owned(),
                )))),
            };
            opening.map(|opened| opened.map_err(program_error))
        });
        Box::pin(opened.map(|opened| {
            opened.unwrap_or_else(|_| Err(OpenError::Failed("WaOS is quitting".to_owned())))
        }))
    }
}

impl ShellOpener for OpenerOnRequest {
    fn open_link(&self, program: &AppInstance, link: &str) -> OpenFuture {
        tracing::info!(program = program.name(), link, "a program opens a link");
        let link = link.to_owned();
        self.ask(move |opener| opener.open_link(&link))
    }

    fn open_file(&self, program: &AppInstance, path: &Path) -> OpenFuture {
        tracing::info!(program = program.name(), ?path, "a program opens a file");
        let path = path.to_owned();
        self.ask(move |opener| opener.open_file(&path))
    }

    fn reveal_file(&self, program: &AppInstance, path: &Path) -> OpenFuture {
        tracing::info!(program = program.name(), ?path, "a program reveals a file");
        let path = path.to_owned();
        self.ask(move |opener| opener.reveal_file(&path))
    }
}

/// Why nothing opened, as the program hears it.
fn program_error(error: ui::OpenError) -> OpenError {
    match error {
        ui::OpenError::NoApp => OpenError::NoApp,
        ui::OpenError::Failed(reason) => OpenError::Failed(reason),
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;

    use waft_test::{MockOpener, Opened, TestPlatform};

    use super::*;
    use crate::state::test_system::{Lines, TestSystem};

    const OPENER: &str = "dev.example.opener";

    /// Runs `examples/opener` over a platform whose opener is `opener`, until it listens for open
    /// requests, done opening; answers what it printed.
    fn run_opener(opener: &MockOpener) -> (TestSystem, Arc<Lines>) {
        let platform = TestPlatform::new().with_opener(Rc::new(opener.clone()));
        let system = TestSystem::new(platform);
        let lines = Arc::new(Lines::default());
        system.run_as(waos_test_programs::OPENER, OPENER, &lines);
        system.run_tasks_until("the program done opening", || {
            lines.has("listening for open requests")
        });
        (system, lines)
    }

    #[test]
    fn a_programs_links_and_files_open_through_wafts_opener_as_the_engines_rules_allow() {
        let opener = MockOpener::new();
        let (_system, lines) = run_opener(&opener);
        for line in [
            "open https://example.com/: ok",
            "open zoommtg://zoom.us/join: denied",
            "open file /data/note.txt: ok",
            "reveal file /data/note.txt: ok",
        ] {
            assert!(lines.has(line), "{line}");
        }
        let asked = opener.asked();
        assert_eq!(asked.len(), 3, "nothing the rules refuse: {asked:?}");
        assert_eq!(asked[0], Opened::Link("https://example.com/".to_owned()));
        let (Opened::File(opened), Opened::Revealed(revealed)) = (&asked[1], &asked[2]) else {
            panic!("the note opened, then shown: {asked:?}");
        };
        assert_eq!(opened, revealed);
        assert_eq!(
            std::fs::read_to_string(opened).unwrap(),
            "a note",
            "where it is"
        );
    }

    #[test]
    fn what_no_app_opens_answers_the_program_no_app() {
        let opener = MockOpener::new().failing(ui::OpenError::NoApp);
        let (_system, lines) = run_opener(&opener);
        assert!(lines.has("open https://example.com/: no app"));
        assert!(lines.has("open file /data/note.txt: no app"));
    }
}
