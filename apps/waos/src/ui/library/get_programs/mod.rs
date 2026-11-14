//! Getting programs, a page of its own behind the home's plus, with a way back: the field that adds
//! a source, and each source with each program it lists, its icon, name and version, and Get,
//! Update, Installing… or Installed; what a source says while it is read or could not be read; and
//! a way to remove each source. What is shared between the layouts is here: each listed program's
//! row as the page shows it, read from the library; the controls are each layout's.
//!
//! The invariant: a program's row offers exactly what the library offers for it (`logic::offers`),
//! and Installing… while the library installs it, whatever the offer, until its code is ready, as
//! the home says.

pub(crate) mod cupertino;
pub(crate) mod fluent;

use std::sync::Arc;

use waos_package::ListedProgram;

use super::words;
use crate::logic::offers::Offer;
use crate::state::library::{Library, Listing, SourceListing};

/// A program a source lists, as its row shows it.
pub(crate) struct ListedRow {
    pub(crate) program: ListedProgram,
    /// Its version, and the version installed where it updates one.
    pub(crate) detail: String,
    /// Its icon where it is installed; none for one not installed, whose icon is in its package.
    pub(crate) icon: Option<Arc<[u8]>>,
    pub(crate) action: RowAction,
}

/// What a listed program's row offers, or says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowAction {
    Get,
    Update,
    Installing,
    Installed,
}

/// The rows of what `listing` lists, or what the source says while it has none to show: that it
/// is being read, or why it could not be.
pub(crate) fn listed_rows(
    listing: &SourceListing,
    library: &Library,
) -> Result<Vec<ListedRow>, String> {
    let offers = match &listing.listing {
        Listing::Reading => return Err(words::READING.to_owned()),
        Listing::Failed(why) => return Err(words::unreadable(why)),
        Listing::Read(offers) => offers,
    };
    Ok(offers
        .iter()
        .map(|(program, offer)| listed_row(program, offer, library))
        .collect())
}

fn listed_row(program: &ListedProgram, offer: &Offer, library: &Library) -> ListedRow {
    let version = program.version.to_string();
    let (detail, action) = match offer {
        Offer::Install => (version, RowAction::Get),
        Offer::Update { installed } => {
            (words::update_detail(&version, installed), RowAction::Update)
        }
        Offer::Installed => (version, RowAction::Installed),
    };
    let action = if library.is_installing(&program.id) || library.is_compiling(&program.id) {
        RowAction::Installing
    } else {
        action
    };
    ListedRow {
        program: program.clone(),
        detail,
        icon: library.icon(&program.id).cloned(),
        action,
    }
}
