//! The document as Typst set it: its pages' sizes and marks, the problems Typst found, and how
//! long the run took; and the runs themselves, which follow the text a moment after it changes.
//!
//! A run happens on the typesetting worker ([`typesetting_worker`](crate::typesetting_worker)),
//! so the window goes on answering while it does: it is asked for once typing pauses, for as long
//! as the last run took and no less than [`SHORTEST_PAUSE`], so a long document is not set
//! between two keys, and its pages arrive when the worker has set them. Typst redoes only what a
//! change reaches. Pages with an error in their text are not set: the pages of the last text
//! without one stay, said to be out of date.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use waft::{App, Listener, TextEditingControllerLeaf, Timer};

use crate::documents_app::DocumentsApp;
use crate::typesetting::messages::{PageOutline, Request, SetPages};
use crate::typesetting::{Place, Problem};

/// The shortest and the longest pause in typing before the text is set again.
const SHORTEST_PAUSE: Duration = Duration::from_millis(120);
const LONGEST_PAUSE: Duration = Duration::from_secs(1);

pub(crate) struct Pages {
    /// The pages of the last text without an error.
    set: Vec<PageOutline>,
    problems: Vec<Problem>,
    /// The document's folder, while the text names a file in it that the program cannot reach.
    closed_folder: Option<PathBuf>,
    took: Duration,
    /// The run waiting for typing to pause.
    waiting: Option<Timer>,
}

impl Pages {
    pub(crate) fn new() -> Pages {
        Pages {
            set: Vec::new(),
            problems: Vec::new(),
            closed_folder: None,
            took: Duration::ZERO,
            waiting: None,
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.set.len()
    }

    /// What the page at `index` shows, as a hash.
    pub(crate) fn mark(&self, index: usize) -> Option<u128> {
        self.set.get(index).map(|page| page.mark)
    }

    /// The width and height of a page, in points.
    pub(crate) fn size(&self, index: usize) -> Option<[f64; 2]> {
        self.set.get(index).map(|page| page.size)
    }

    /// Each page's height, in points.
    pub(crate) fn heights(&self) -> Rc<[f64]> {
        self.set.iter().map(|page| page.size[1]).collect()
    }

    /// The width of the widest page, in points.
    pub(crate) fn widest(&self) -> f64 {
        self.set.iter().map(|page| page.size[0]).fold(0.0, f64::max)
    }

    pub(crate) fn problems(&self) -> &[Problem] {
        &self.problems
    }

    pub(crate) fn error_count(&self) -> usize {
        self.problems
            .iter()
            .filter(|problem| problem.is_error)
            .count()
    }

    /// The document's folder, while the text names a file in it and the program has not been
    /// given the folder.
    pub(crate) fn closed_folder(&self) -> Option<&Path> {
        self.closed_folder.as_deref()
    }

    pub(crate) fn took(&self) -> Duration {
        self.took
    }

    /// Lets go of the pages and problems, as a document shown in place of another has none yet.
    pub(crate) fn forget(&mut self) {
        self.set.clear();
        self.problems.clear();
        self.closed_folder = None;
    }

    /// How long typing pauses before the next run: as long as the last run took, within bounds.
    fn pause(&self) -> Duration {
        self.took.clamp(SHORTEST_PAUSE, LONGEST_PAUSE)
    }

    /// Keeps what a run made: its pages where it set any, the last ones otherwise.
    fn keep(&mut self, set: SetPages) {
        if let Some(pages) = set.pages {
            self.set = pages;
        }
        self.problems = set.problems;
        self.closed_folder = set.closed_folder;
        self.took = set.took;
    }

    /// Where the problems in the text are, in UTF-16 code units.
    fn problem_places(&self) -> Vec<Range<usize>> {
        let places = self.problems.iter().map(|problem| &problem.place);
        places
            .filter_map(|place| match place {
                Place::Text { utf16, .. } => Some(utf16.clone()),
                Place::File(_) | Place::Unknown => None,
            })
            .collect()
    }
}

/// Sets the text as pages once typing has paused.
pub(crate) fn typeset_soon(app: &mut App, documents: &DocumentsApp) {
    let running = documents.clone();
    let pause = documents.pages.read(app).pause();
    let run = Timer::new(
        app,
        pause,
        Listener::new(move |app| typeset_now(app, &running)),
    );
    let waiting = documents
        .pages
        .update(app, |pages, _cx| pages.waiting.replace(run));
    if let Some(waiting) = waiting {
        waiting.cancel(app);
    }
}

/// Asks for the text to be set as pages now: the document's file names the folder its files are
/// found in. The pages arrive with [`pages_set`].
pub(crate) fn typeset_now(app: &mut App, documents: &DocumentsApp) {
    let text = documents.editor.text.text_value(app).to_owned();
    let path = documents.document.read(app).path().map(ToOwned::to_owned);
    let waiting = documents
        .pages
        .update(app, |pages, _cx| pages.waiting.take());
    if let Some(waiting) = waiting {
        waiting.cancel(app);
    }
    documents.typesetting.ask(Request::Typeset { path, text });
}

/// The worker set the text as `set` says: the pages, the problems in the editor, and the
/// pictures of pages no longer there let go.
pub(crate) fn pages_set(app: &mut App, documents: &DocumentsApp, set: SetPages) {
    let places = documents.pages.update(app, |pages, cx| {
        pages.keep(set);
        cx.notify();
        pages.problem_places()
    });
    documents.editor.text.set_problems(app, &places);
    let count = documents.pages.read(app).count();
    documents.page_images.update(app, |images, cx| {
        images.pages_set(count);
        cx.notify();
    });
}
