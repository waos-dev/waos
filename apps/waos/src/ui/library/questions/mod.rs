//! What the library asks or says, as a dialog over whatever shows: the question asked now, whether
//! to install a package opened with WaOS, with its icon, or whether to remove a program, with its
//! two answers; or, while nothing is asked, what went wrong last, with OK. On a phone it is
//! Cupertino's alert over the deck, the home or a program's tab alike (`cupertino`); on a desktop
//! Fluent's content dialog over the panel (`fluent`). What is shared between them is here: what is
//! said now, in words, read from the library.
//!
//! The invariant: a dialog shows exactly while the library has something to ask or say, and says
//! what it has now, so answering one question shows the next in its place, and the dialog closes
//! once nothing is left; its buttons only answer through the library and never close it
//! themselves.

pub(crate) mod cupertino;
pub(crate) mod fluent;

use std::sync::Arc;

use waft::{App, Entity};

use super::words;
use crate::state::library::{Library, Question};

/// What the library says now, as a dialog shows it.
pub(crate) struct Said {
    pub(crate) title: String,
    pub(crate) message: String,
    /// The program's icon, over a question to install it.
    pub(crate) icon: Option<Arc<[u8]>>,
    /// What answers no, or dismisses.
    pub(crate) dismiss: &'static str,
    /// What answers yes; none for what only tells.
    pub(crate) confirm: Option<Confirm>,
}

/// The answer that does what the question asks.
pub(crate) struct Confirm {
    pub(crate) label: &'static str,
    /// Whether it destroys something, as removing a program does.
    pub(crate) destructive: bool,
}

/// What `library` asks or says now: the question asked, else the problem; none for nothing.
pub(crate) fn said_now(library: &Library) -> Option<Said> {
    if let Some(question) = library.question() {
        return Some(asked(question));
    }
    let problem = library.problem()?;
    Some(Said {
        title: words::PROBLEM.to_owned(),
        message: problem.to_owned(),
        icon: None,
        dismiss: words::OK,
        confirm: None,
    })
}

fn asked(question: &Question) -> Said {
    let asked = words::asked(question);
    let icon = match question {
        Question::Install(package) => Some(Arc::clone(package.icon())),
        Question::Remove { .. } => None,
    };
    Said {
        title: asked.title,
        message: asked.message,
        icon,
        dismiss: asked.no,
        confirm: Some(Confirm {
            label: asked.yes,
            destructive: matches!(question, Question::Remove { .. }),
        }),
    }
}

/// The dismissing answer to what the library says now: no to the question, or the problem
/// dismissed.
pub(crate) fn dismiss(library: &Entity<Library>, app: &mut App) {
    library.update(app, |library, cx| {
        if library.question().is_some() {
            library.answer(false, cx);
        } else {
            library.dismiss_problem(cx);
        }
    });
}

/// Yes to the question asked now.
pub(crate) fn confirm(library: &Entity<Library>, app: &mut App) {
    library.update(app, |library, cx| library.answer(true, cx));
}
