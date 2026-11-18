//! The Inspector's widgets, thin over the state: Fluent's at a desktop's width, Cupertino's at a
//! phone's.

pub(crate) mod cupertino;
pub(crate) mod fluent;

use waft::App;

use crate::clock;
use crate::logic::panels::Said;

/// What `said` reads, in the user's time zone: its text, or the moment it names, with the words
/// that come before it.
pub(crate) fn said(said: &Said, app: &App) -> String {
    match said {
        Said::Text(text) => text.clone(),
        Said::Moment { before, moment } => {
            let said = clock::said(moment, app);
            if before.is_empty() {
                said
            } else {
                format!("{before} {said}")
            }
        }
    }
}
