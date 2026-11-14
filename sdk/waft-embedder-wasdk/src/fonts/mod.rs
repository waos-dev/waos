//! The program's fonts, as the design's decisions 36 and 37 set them: the engine's `fonts`
//! functions as valo's [`FontSource`](valo::FontSource) ([`source`]), and the engine's glyph calls
//! as valo's [`GlyphRaster`](valo::GlyphRaster) ([`glyphs`]).
//!
//! The program shapes and lays text out itself, from each face's tables as the engine serves
//! them, which is the font without its outline tables; a glyph's shape comes from the engine,
//! drawn from the file it kept, so the pixels are the ones a native app makes. A font made here
//! carries its typeface's id as valo's `remote_id`, which the glyph raster reads back to name the
//! typeface to the engine; a font the program registered itself has its outlines here, has no
//! remote id, and is drawn here.
//!
//! The invariant: every font made from a typeface the engine handed out has that typeface's id
//! as its remote id, and no other font has one.

mod glyphs;
mod source;

pub(crate) use glyphs::ProgramGlyphs;
pub(crate) use source::ProgramFonts;
