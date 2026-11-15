//! The fonts a document is set in: the ones the program carries ([`crate::bundled_fonts`]), Typst's
//! own, and the system's after them, as Typst's command-line tool has them, so a document that
//! names no font is set as Typst sets it everywhere, and one that names an installed family, or
//! a character the carried faces lack, finds it.
//!
//! Typst picks a face itself, from a book of every face it may use given before it sets anything.
//! So the book is made at once, a system face's entry from its tables as the platform serves them
//! (`system_faces`), and a face's file is opened only when Typst first asks for that face, as
//! `typst-kit`'s `FontStore` loads each face on its first use; a file is read once for all the
//! faces in it.
//!
//! The invariant a later editor will break: a face's index in the book is its index among the
//! slots, the carried faces first, so where a carried face and a system face are alike, Typst
//! takes the carried one.

use std::sync::OnceLock;

use typst::foundations::Bytes;
use typst::text::{Font, FontBook, FontInfo};
use typst::utils::LazyHash;

use super::system_faces::{SystemFace, SystemFiles};
use crate::bundled_fonts::{
    DEJAVU_SANS_MONO, DEJAVU_SANS_MONO_OBLIQUE, LIBERTINUS_SERIF, NEW_COMPUTER_MODERN_MATH,
};

/// Every face a document may be set in, and Typst's book of them, in one order: a face's index
/// in the book is its index here.
pub(crate) struct DocumentFonts {
    book: LazyHash<FontBook>,
    slots: Vec<FontSlot>,
    system_files: SystemFiles,
}

/// One face, and the font made of it once Typst has asked for it.
struct FontSlot {
    source: FontSource,
    font: OnceLock<Option<Font>>,
}

enum FontSource {
    /// A face the program carries, read already.
    Carried(Font),
    /// A face installed on the system, read when first asked for.
    System(SystemFace),
}

impl DocumentFonts {
    /// The faces the program carries alone.
    pub(crate) fn carried() -> DocumentFonts {
        let mut fonts = DocumentFonts {
            book: LazyHash::new(FontBook::new()),
            slots: Vec::new(),
            system_files: SystemFiles::default(),
        };
        for font in carried_fonts() {
            fonts.push(font.info().clone(), FontSource::Carried(font));
        }
        fonts
    }

    /// The faces the program carries, then every face installed on the system.
    pub(crate) fn with_system() -> DocumentFonts {
        let mut fonts = DocumentFonts::carried();
        for (face, info) in SystemFace::installed() {
            fonts.push(info, FontSource::System(face));
        }
        fonts
    }

    pub(crate) fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    /// The face at `index`, read the first time it is asked for.
    pub(crate) fn font(&self, index: usize) -> Option<Font> {
        let slot = self.slots.get(index)?;
        slot.font
            .get_or_init(|| slot.source.load(&self.system_files))
            .clone()
    }

    fn push(&mut self, info: FontInfo, source: FontSource) {
        self.book.push(info);
        self.slots.push(FontSlot {
            source,
            font: OnceLock::new(),
        });
    }
}

impl FontSource {
    fn load(&self, system_files: &SystemFiles) -> Option<Font> {
        match self {
            FontSource::Carried(font) => Some(font.clone()),
            FontSource::System(face) => face.load(system_files),
        }
    }
}

/// The faces the program carries, in the order Typst's tool embeds its own.
fn carried_fonts() -> impl Iterator<Item = Font> {
    LIBERTINUS_SERIF
        .into_iter()
        .chain(NEW_COMPUTER_MODERN_MATH)
        .chain(DEJAVU_SANS_MONO)
        .chain(DEJAVU_SANS_MONO_OBLIQUE)
        .flat_map(|file| Font::iter(Bytes::new(file)))
}
