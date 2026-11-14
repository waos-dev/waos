//! What the window and the typesetting worker say to each other: the window's requests, and the
//! worker's replies, one to each request in the order asked. Each crosses as one message, its
//! bytes postcard's.
//!
//! The invariant: a reply names what it answers (a page's index, mark and density), so the window
//! can tell a reply about pages it has since set again from one about the pages it shows.

use std::path::PathBuf;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::Problem;

/// What the window asks of the worker.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Request {
    /// Set `text` as pages, the document's file at `path`, whose folder its files are found in, or
    /// untitled.
    Typeset { path: Option<PathBuf>, text: String },
    /// Draw the page at `index`, as set with `mark`, at `density` pixels to a thousand points.
    Draw {
        index: usize,
        mark: u128,
        density: u32,
    },
    /// Write the pages as set last as a PDF.
    ExportPdf,
}

/// What the worker answers.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Reply {
    Set(SetPages),
    Drawn(DrawnPage),
    Pdf(Result<Vec<u8>, PdfRefusal>),
}

/// What one run of the compiler made of the text.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SetPages {
    /// The pages, where the text has no error.
    pub(crate) pages: Option<Vec<PageOutline>>,
    /// The errors, then the warnings.
    pub(crate) problems: Vec<Problem>,
    /// The document's folder, where the text names a file in it and the program has not been
    /// given the folder.
    pub(crate) closed_folder: Option<PathBuf>,
    pub(crate) took: Duration,
}

/// One page as set: its width and height in points, and what it shows, as a hash.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PageOutline {
    pub(crate) size: [f64; 2],
    pub(crate) mark: u128,
}

/// The page asked for, drawn; its picture none where the pages set last hold no page of that
/// mark at that index.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DrawnPage {
    pub(crate) index: usize,
    pub(crate) mark: u128,
    pub(crate) density: u32,
    pub(crate) picture: Option<PagePicture>,
}

/// A page's pixels, RGBA with their alpha premultiplied, row after row with no gap.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PagePicture {
    pub(crate) width: u32,
    pub(crate) height: u32,
    #[serde(with = "serde_bytes")]
    pub(crate) rgba: Vec<u8>,
}

/// Why no PDF was made.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum PdfRefusal {
    /// The text has an error, so its pages are not set.
    HasErrors,
    /// Typst's exporter failed, in its words.
    Failed(String),
}

/// `message` as the bytes that cross.
pub(crate) fn encoded(message: &impl Serialize) -> Vec<u8> {
    postcard::to_allocvec(message).expect("a message of plain values always encodes")
}

/// The message `bytes` carry; none where they are no such message, which only a build of the
/// window and the worker apart could make.
pub(crate) fn decoded<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}
