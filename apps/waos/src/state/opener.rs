//! The programs' openings on the main thread, over Waft's opener: each link opened in the app the
//! system picks for its scheme, each file in the app it picks for its type or shown in its file
//! manager, and answered with whether the system handed it on. The opener is the engine's shell's
//! only where Waft's platform has one.
//!
//! The invariant: every opening arriving is answered, so no program waits for an answer that
//! cannot come.

use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::App;
use waft::ui::{self, OpenFuture, Platform};
use waos::ShellOpener;
use waos::bindings::wasdk::platform::opener::OpenError;

use crate::logic::opener_calls::{OpenerOnRequest, Opening, OpeningCall};

/// Waft's opener, served to the engine as its shell's.
pub(crate) struct Opener {
    calls: OpenerOnRequest,
    arriving: UnboundedReceiver<OpeningCall>,
}

impl Opener {
    /// Waft's opener on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<Opener> {
        platform.opener()?;
        let (calls, arriving) = OpenerOnRequest::new();
        Some(Opener { calls, arriving })
    }

    /// The opener as the engine's shell has it: each opening goes to the main thread, where
    /// [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellOpener> {
        Arc::new(self.calls.clone())
    }

    /// From now on, each opening a program asks for is carried out through Waft's opener.
    pub(crate) fn serve(self, app: &mut App) {
        let (platform, mut arriving) = (app.platform(), self.arriving);
        app.spawn(async move |cx| {
            while let Some(OpeningCall { opening, opened }) = arriving.next().await {
                let opening = open(platform.as_ref(), opening);
                cx.update(|app| {
                    app.spawn(async move |_cx| {
                        // A program that ended meanwhile hears nothing.
                        let _ = opened.send(opening.await.map_err(program_error));
                    })
                });
            }
        });
    }
}

/// Hands `opening` to the system through `platform`'s opener.
fn open(platform: &dyn Platform, opening: Opening) -> OpenFuture {
    let Some(opener) = platform.opener() else {
        let failed = ui::OpenError::Failed("the system has no opener".to_owned());
        return Box::pin(std::future::ready(Err(failed)));
    };
    match opening {
        Opening::Link(link) => opener.open_link(&link),
        Opening::File(path) => opener.open_file(&path),
        Opening::Reveal(path) => opener.reveal_file(&path),
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
