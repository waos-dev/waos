//! The code serving `wasdk:platform/fonts` to a program: the manager's answers as handles on
//! typefaces, a typeface's getters and its file, as a stream with its length, the glyph calls,
//! each recorded under [`targets::GLYPHS`], and the installed fonts' changes.

use wasmtime::component::{Access, HasData, Linker, Resource, ResourceTable, StreamReader};

use crate::bindings::wasdk::platform::fonts::{
    self, Font, FontFile, GlyphImage, GlyphStroke, HostTypeface, HostTypefaceWithStore,
    HostWithStore, PathElement, Style, Tag,
};
use crate::stream::{stream_of, stream_of_shared_bytes};
use crate::targets;

use super::{ProgramFonts, Typeface};

/// What the fonts interface reads and writes in a program's store.
pub(crate) struct FontsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) fonts: &'a mut ProgramFonts,
}

pub(crate) trait FontsCtxView: Send {
    fn fonts_ctx(&mut self) -> FontsCtx<'_>;
}

struct HasFonts;

impl HasData for HasFonts {
    type Data<'a> = FontsCtx<'a>;
}

pub(crate) fn add_to_linker<T: FontsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    fonts::add_to_linker::<_, HasFonts>(linker, T::fonts_ctx)
}

impl FontsCtx<'_> {
    fn typeface(&self, typeface: &Resource<Typeface>) -> wasmtime::Result<&Typeface> {
        Ok(self.table.get(typeface)?)
    }

    /// A handle on `typeface`, if there is one.
    fn handle(
        &mut self,
        typeface: Option<Typeface>,
    ) -> wasmtime::Result<Option<Resource<Typeface>>> {
        typeface
            .map(|typeface| Ok(self.table.push(typeface)?))
            .transpose()
    }
}

impl fonts::Host for FontsCtx<'_> {
    fn families(&mut self) -> wasmtime::Result<Vec<String>> {
        Ok(self.fonts.families())
    }

    fn family(&mut self, family: String) -> wasmtime::Result<Vec<Resource<Typeface>>> {
        self.fonts
            .family(&family)
            .into_iter()
            .map(|typeface| Ok(self.table.push(typeface)?))
            .collect()
    }

    fn match_family_style(
        &mut self,
        family: String,
        style: Style,
    ) -> wasmtime::Result<Option<Resource<Typeface>>> {
        let typeface = self.fonts.match_family_style(&family, style);
        self.handle(typeface)
    }

    fn match_character(
        &mut self,
        family: Option<String>,
        style: Style,
        locales: Vec<String>,
        character: char,
    ) -> wasmtime::Result<Option<Resource<Typeface>>> {
        let locales: Vec<&str> = locales.iter().map(String::as_str).collect();
        let typeface = self
            .fonts
            .match_character(family.as_deref(), style, &locales, character);
        self.handle(typeface)
    }

    fn system_font(
        &mut self,
        size: f32,
        style: Style,
    ) -> wasmtime::Result<Option<Resource<Typeface>>> {
        let typeface = self.fonts.system_font(size, style);
        self.handle(typeface)
    }

    fn glyph_alpha(
        &mut self,
        font: Font,
        glyph: u32,
        px: f32,
        dx: f32,
    ) -> wasmtime::Result<Option<GlyphImage>> {
        let image = self.fonts.glyph_alpha(&font, glyph, px, dx);
        record("alpha", &font, glyph, image.is_some());
        Ok(image)
    }

    fn glyph_stroked(
        &mut self,
        font: Font,
        glyph: u32,
        px: f32,
        dx: f32,
        stroke: GlyphStroke,
    ) -> wasmtime::Result<Option<GlyphImage>> {
        let image = self.fonts.glyph_stroked(&font, glyph, px, dx, stroke);
        record("stroked", &font, glyph, image.is_some());
        Ok(image)
    }

    fn glyph_sdf(
        &mut self,
        font: Font,
        glyph: u32,
        px: f32,
    ) -> wasmtime::Result<Option<GlyphImage>> {
        let image = self.fonts.glyph_sdf(&font, glyph, px);
        record("sdf", &font, glyph, image.is_some());
        Ok(image)
    }

    fn glyph_color(
        &mut self,
        font: Font,
        glyph: u32,
        px: f32,
    ) -> wasmtime::Result<Option<GlyphImage>> {
        let image = self.fonts.glyph_color(&font, glyph, px);
        record("color", &font, glyph, image.is_some());
        Ok(image)
    }

    fn glyph_path(
        &mut self,
        font: Font,
        glyph: u32,
        px: f32,
    ) -> wasmtime::Result<Option<Vec<PathElement>>> {
        let outline = self.fonts.glyph_path(&font, glyph, px);
        record("path", &font, glyph, outline.is_some());
        Ok(outline)
    }
}

/// Records one glyph call: which call it was, the glyph of `font` it named, and whether the engine
/// drew a shape for it.
fn record(call: &'static str, font: &Font, glyph: u32, drawn: bool) {
    tracing::trace!(
        target: targets::GLYPHS,
        call,
        typeface = font.typeface,
        glyph,
        drawn,
        "a glyph call"
    );
}

impl<T: FontsCtxView + 'static> HostWithStore<T> for HasFonts {
    fn changes(mut access: Access<T, Self>) -> wasmtime::Result<StreamReader<u64>> {
        let changes = access.get().fonts.changes();
        stream_of(access, changes, |_: &mut T, count| Ok(count))
    }
}

impl<T: FontsCtxView + 'static> HostTypefaceWithStore<T> for HasFonts {
    fn open_stream(
        mut access: Access<T, Self>,
        typeface: Resource<Typeface>,
    ) -> wasmtime::Result<FontFile> {
        let (file, index) = access.get().typeface(&typeface)?.file();
        let length = (*file).as_ref().len() as u64;
        Ok(FontFile {
            data: stream_of_shared_bytes(access, file)?,
            length,
            index,
        })
    }
}

impl HostTypeface for FontsCtx<'_> {
    fn id(&mut self, typeface: Resource<Typeface>) -> wasmtime::Result<u64> {
        Ok(self.typeface(&typeface)?.id())
    }

    fn family(&mut self, typeface: Resource<Typeface>) -> wasmtime::Result<String> {
        Ok(self.typeface(&typeface)?.family())
    }

    fn style(&mut self, typeface: Resource<Typeface>) -> wasmtime::Result<Style> {
        Ok(self.typeface(&typeface)?.style())
    }

    fn file_id(&mut self, typeface: Resource<Typeface>) -> wasmtime::Result<u64> {
        Ok(self.typeface(&typeface)?.file_id())
    }

    fn index(&mut self, typeface: Resource<Typeface>) -> wasmtime::Result<u32> {
        Ok(self.typeface(&typeface)?.index())
    }

    fn tables(&mut self, typeface: Resource<Typeface>) -> wasmtime::Result<Vec<Tag>> {
        Ok(self.typeface(&typeface)?.tables())
    }

    fn table(
        &mut self,
        typeface: Resource<Typeface>,
        tag: Tag,
    ) -> wasmtime::Result<Option<Vec<u8>>> {
        Ok(self.typeface(&typeface)?.table(tag))
    }

    fn drop(&mut self, typeface: Resource<Typeface>) -> wasmtime::Result<()> {
        self.table.delete(typeface)?;
        Ok(())
    }
}
