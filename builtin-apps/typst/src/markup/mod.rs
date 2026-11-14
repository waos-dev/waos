//! The markup as the editor shows it: each stretch of the text marked with what Typst's parser
//! says it is ([`marks`]), drawn in a palette for a light or a dark theme ([`palette`]), by the
//! editor's text controller ([`controller`]), which also underlines where Typst found a problem.

pub(crate) mod controller;
mod marks;
mod palette;
mod utf16;

pub(crate) use self::palette::Palette;
pub(crate) use self::utf16::line_and_column;
