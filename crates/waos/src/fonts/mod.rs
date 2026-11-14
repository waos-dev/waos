//! `wasdk:platform/fonts`: the fonts a program is given, answered by the font manager in its
//! environment, and the glyphs of the faces it was handed, drawn here.
//!
//! [`ProgramFonts`] is one program's: its manager's answers, each face given an id of this
//! program's ([`typefaces`]), the fonts made of those faces and the rasterizer that draws them
//! ([`glyphs`]), and the installed fonts' changes ([`changes`]). The WIT's `typeface` is a
//! resource; [`Typeface`] is the value its methods answer from, and the code serving the
//! interface, [`host`], makes the handles. With no manager every answer is empty, as on a
//! machine with no fonts installed.
//!
//! The invariant: a glyph call draws only a face this program was handed, named by the id it was
//! handed with. Ids are the program's own, never the engine's: an engine-wide id would let a
//! program draw from a face another program was given, or from any face at all when it was
//! given no fonts, and would keep every file any program ever named mapped for the engine's life.
//! A file's bytes are still one mapping in the process, shared by every program through the one
//! manager, so the per-program ids cost nothing but the map that names them.
//!
//! A face's whole file, which `typeface.open-stream` hands a program that reads fonts itself with
//! its length, is copied into the program as it reads, from that one mapping: the file's size in
//! each program that asks. The interface would let the engine map the file into the program instead, with no
//! copy (`../experiments/2026-10-17-font-file-mapping`, on macOS and Linux); that is not built.

mod changes;
mod glyphs;
pub(crate) mod host;
mod typefaces;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::channel::mpsc;
use valo_fontmgr::FontManager;

use crate::bindings::wasdk::platform::fonts::{
    Font, GlyphImage, GlyphStroke, PathElement, Slant, Style, Tag,
};
use changes::FontChanges;
use glyphs::Glyphs;
use typefaces::Typefaces;

/// One face as the engine handed it to a program: what the WIT's `typeface` answers.
pub struct Typeface {
    id: u64,
    face: valo_fontmgr::Typeface,
}

impl Typeface {
    /// The engine's id for this face, which a `font` names it by.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The name the platform files the face under.
    pub(crate) fn family(&self) -> String {
        self.face.family().to_owned()
    }

    pub(crate) fn style(&self) -> Style {
        style_to(self.face.style())
    }

    /// The face's tables in the file's order, without the outline and colour-glyph tables,
    /// which valo's `Typeface::tables` leaves out: what keeps a font file in the engine.
    pub(crate) fn tables(&self) -> Vec<Tag> {
        self.face
            .tables()
            .iter()
            .map(|(tag, _)| u32::from_be_bytes(tag.to_be_bytes()))
            .collect()
    }

    /// The font file the face is in, as the manager shares it, and the face's index in it.
    pub(crate) fn file(&self) -> (valo_fontmgr::FontData, u32) {
        (self.face.shared_data(), self.face.index())
    }

    /// One of [`tables`](Typeface::tables)' bytes; none for a tag the face lacks or the engine
    /// keeps.
    pub(crate) fn table(&self, tag: Tag) -> Option<Vec<u8>> {
        let wanted = valo_fontmgr::Tag::new(&tag.to_be_bytes());
        self.face
            .tables()
            .into_iter()
            .find(|(tag, _)| *tag == wanted)
            .map(|(_, bytes)| bytes.to_vec())
    }
}

/// One program's fonts: its manager's answers, each face named by an id of this program's, and
/// the glyphs of the faces it was handed.
pub(crate) struct ProgramFonts {
    /// The manager in the program's environment, shared with every other program it serves;
    /// none when the program was given no fonts.
    manager: Option<Arc<Mutex<dyn FontManager>>>,
    typefaces: Typefaces,
    glyphs: Glyphs,
    changes: FontChanges,
}

impl ProgramFonts {
    /// The fonts of a program given `manager`, whose changes it counts from now on: the
    /// program's start.
    pub(crate) fn new(manager: Option<Arc<Mutex<dyn FontManager>>>) -> ProgramFonts {
        let changes = FontChanges::watching(manager.as_deref());
        ProgramFonts {
            manager,
            typefaces: Typefaces::default(),
            glyphs: Glyphs::default(),
            changes,
        }
    }

    /// The installed family names.
    pub(crate) fn families(&self) -> Vec<String> {
        self.ask(|manager| manager.families()).unwrap_or_default()
    }

    /// Every face the platform has under `family`; empty for a family it does not know.
    pub(crate) fn family(&mut self, family: &str) -> Vec<Typeface> {
        let faces = self.ask(|manager| manager.family(family));
        faces
            .unwrap_or_default()
            .into_iter()
            .map(|face| self.handed_out(face))
            .collect()
    }

    /// The family's face nearest `style`.
    pub(crate) fn match_family_style(&mut self, family: &str, style: Style) -> Option<Typeface> {
        let face = self.ask(|manager| manager.match_family_style(family, style_of(style)))?;
        face.map(|face| self.handed_out(face))
    }

    /// A face that covers `character`, preferring the languages in `locales`, then `family`,
    /// then `style`.
    pub(crate) fn match_character(
        &mut self,
        family: Option<&str>,
        style: Style,
        locales: &[&str],
        character: char,
    ) -> Option<Typeface> {
        let face = self
            .ask(|manager| manager.match_character(family, style_of(style), locales, character))?;
        face.map(|face| self.handed_out(face))
    }

    /// The platform's own user-interface face for text at `size` points, nearest `style`.
    pub(crate) fn system_font(&mut self, size: f32, style: Style) -> Option<Typeface> {
        let face = self.ask(|manager| manager.system_font(size, style_of(style)))?;
        face.map(|face| self.handed_out(face))
    }

    /// How many times the installed fonts have changed since the program started, at each
    /// change from now on, on a stream of its own.
    pub(crate) fn changes(&self) -> mpsc::UnboundedReceiver<u64> {
        self.changes.subscribe()
    }

    /// The glyph's coverage, one byte per pixel.
    pub(crate) fn glyph_alpha(
        &mut self,
        font: &Font,
        glyph: u32,
        px: f32,
        dx: f32,
    ) -> Option<GlyphImage> {
        self.glyphs.alpha(&self.typefaces, font, glyph, px, dx)
    }

    /// The coverage of the glyph's outline widened by `stroke`.
    pub(crate) fn glyph_stroked(
        &mut self,
        font: &Font,
        glyph: u32,
        px: f32,
        dx: f32,
        stroke: GlyphStroke,
    ) -> Option<GlyphImage> {
        self.glyphs
            .stroked(&self.typefaces, font, glyph, px, dx, stroke)
    }

    /// The glyph as a signed distance field, one byte per pixel.
    pub(crate) fn glyph_sdf(&mut self, font: &Font, glyph: u32, px: f32) -> Option<GlyphImage> {
        self.glyphs.sdf(&self.typefaces, font, glyph, px)
    }

    /// The glyph's own colours, premultiplied RGBA.
    pub(crate) fn glyph_color(&mut self, font: &Font, glyph: u32, px: f32) -> Option<GlyphImage> {
        self.glyphs.color(&self.typefaces, font, glyph, px)
    }

    /// The glyph's outline at `px`, y down, origin on the baseline.
    pub(crate) fn glyph_path(
        &mut self,
        font: &Font,
        glyph: u32,
        px: f32,
    ) -> Option<Vec<PathElement>> {
        self.glyphs.path(&self.typefaces, font, glyph, px)
    }

    /// The manager's answer to `question`; none when the program was given no fonts.
    fn ask<T>(&self, question: impl FnOnce(&mut dyn FontManager) -> T) -> Option<T> {
        let manager = self.manager.as_deref()?;
        Some(question(&mut *lock(manager)))
    }

    /// `face`, about to reach the program, with its id: every answer carrying a face comes
    /// through here, so a glyph call can name any face the program holds.
    fn handed_out(&mut self, face: valo_fontmgr::Typeface) -> Typeface {
        let id = self.typefaces.id(&face);
        Typeface { id, face }
    }
}

/// The manager, locked for one question. It is the embedder's and every program's: a lookup
/// that panicked on another program's thread leaves at worst a cache half filled, so this
/// program asks it still.
fn lock<'a>(
    manager: &'a Mutex<dyn FontManager + 'static>,
) -> MutexGuard<'a, dyn FontManager + 'static> {
    manager.lock().unwrap_or_else(PoisonError::into_inner)
}

/// valo's style for the interface's.
fn style_of(style: Style) -> valo_fontmgr::Style {
    valo_fontmgr::Style {
        weight: style.weight,
        width: style.width,
        slant: match style.slant {
            Slant::Upright => valo_fontmgr::Slant::Upright,
            Slant::Italic => valo_fontmgr::Slant::Italic,
            Slant::Oblique => valo_fontmgr::Slant::Oblique,
        },
    }
}

/// The interface's style for valo's, which is what a face answers with.
fn style_to(style: valo_fontmgr::Style) -> Style {
    Style {
        weight: style.weight,
        width: style.width,
        slant: match style.slant {
            valo_fontmgr::Slant::Upright => Slant::Upright,
            valo_fontmgr::Slant::Italic => Slant::Italic,
            valo_fontmgr::Slant::Oblique => Slant::Oblique,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use valo_fontmgr::Watch;

    const REGULAR: Style = Style {
        weight: 400,
        width: 100.0,
        slant: Slant::Upright,
    };

    /// What a font watch is told to call at each change.
    type OnChange = Box<dyn Fn() + Send + Sync>;

    /// A manager over the faces it is made with, whose changes a test reports.
    #[derive(Default)]
    struct Faces {
        faces: Vec<valo_fontmgr::Typeface>,
        on_change: Arc<Mutex<Option<OnChange>>>,
    }

    impl FontManager for Faces {
        fn family(&mut self, family: &str) -> Vec<valo_fontmgr::Typeface> {
            self.faces
                .iter()
                .filter(|face| face.family() == family)
                .cloned()
                .collect()
        }

        fn match_character(
            &mut self,
            _family: Option<&str>,
            _style: valo_fontmgr::Style,
            _locales: &[&str],
            character: char,
        ) -> Option<valo_fontmgr::Typeface> {
            self.faces
                .iter()
                .find(|face| face.covers(character))
                .cloned()
        }

        fn system_font(
            &mut self,
            _size: f32,
            _style: valo_fontmgr::Style,
        ) -> Option<valo_fontmgr::Typeface> {
            self.faces.first().cloned()
        }

        fn families(&self) -> Vec<String> {
            self.faces
                .iter()
                .map(|face| face.family().to_owned())
                .collect()
        }

        fn face_count(&self) -> usize {
            self.faces.len()
        }

        fn watch(&mut self, on_change: OnChange) -> Option<Watch> {
            *self.on_change.lock().unwrap() = Some(on_change);
            Some(Watch::new(()))
        }
    }

    fn fonts_over(manager: Faces) -> ProgramFonts {
        ProgramFonts::new(Some(Arc::new(Mutex::new(manager))))
    }

    /// The platform's system font, the face the glyph tests draw from; none on a machine with
    /// no fonts, where they pass without looking.
    fn system_face() -> Option<valo_fontmgr::Typeface> {
        valo_fontmgr::platform().system_font(13.0, valo_fontmgr::Style::default())
    }

    /// The glyph `face` maps `character` to.
    fn glyph_of(face: &valo_fontmgr::Typeface, character: char) -> u32 {
        let font = valo_text::Font::from_data(face.shared_data(), face.index()).unwrap();
        font.glyph_for(character).unwrap()
    }

    fn at_default(typeface: &Typeface) -> Font {
        Font {
            typeface: typeface.id(),
            variation_coordinates: Vec::new(),
            embolden: 0.0,
            skew: 0.0,
        }
    }

    #[test]
    fn a_program_given_no_fonts_gets_empty_answers() {
        let mut fonts = ProgramFonts::new(None);
        assert!(fonts.families().is_empty());
        assert!(fonts.family("Helvetica").is_empty());
        assert!(fonts.match_family_style("Helvetica", REGULAR).is_none());
        assert!(fonts.match_character(None, REGULAR, &[], 'a').is_none());
        assert!(fonts.system_font(13.0, REGULAR).is_none());
        let named_anyway = Font {
            typeface: 1,
            variation_coordinates: Vec::new(),
            embolden: 0.0,
            skew: 0.0,
        };
        assert!(fonts.glyph_alpha(&named_anyway, 1, 32.0, 0.0).is_none());
        assert!(fonts.glyph_path(&named_anyway, 1, 32.0).is_none());
    }

    #[test]
    fn every_stream_hears_how_many_changes_there_have_been_since_the_start() {
        let manager = Faces::default();
        let on_change = Arc::clone(&manager.on_change);
        let fonts = fonts_over(manager);
        let change = || (on_change.lock().unwrap().as_ref().unwrap())();
        let mut early = fonts.changes();
        change();
        let mut late = fonts.changes();
        change();
        assert_eq!(early.try_recv().unwrap(), 1);
        assert_eq!(early.try_recv().unwrap(), 2);
        assert_eq!(
            late.try_recv().unwrap(),
            2,
            "a late stream counts from the start"
        );
        assert!(early.try_recv().is_err() && late.try_recv().is_err());
    }

    #[test]
    fn a_face_keeps_its_id_however_it_was_matched() {
        let Some(face) = system_face() else {
            return;
        };
        let family = face.family().to_owned();
        let mut fonts = fonts_over(Faces {
            faces: vec![face],
            ..Faces::default()
        });
        let by_family = fonts.family(&family).pop().unwrap();
        let by_character = fonts.match_character(None, REGULAR, &[], 'a').unwrap();
        let by_size = fonts.system_font(13.0, REGULAR).unwrap();
        assert_eq!(by_family.id(), by_character.id());
        assert_eq!(by_family.id(), by_size.id());
    }

    #[test]
    fn a_typeface_serves_its_tables_without_outlines_or_colour_glyphs() {
        let Some(face) = system_face() else {
            return;
        };
        let mut fonts = fonts_over(Faces {
            faces: vec![face],
            ..Faces::default()
        });
        let typeface = fonts.system_font(13.0, REGULAR).unwrap();
        let tag = |name: &[u8; 4]| u32::from_be_bytes(*name);
        let tables = typeface.tables();
        assert!(tables.contains(&tag(b"cmap")) && tables.contains(&tag(b"hmtx")));
        for kept in [
            b"glyf", b"loca", b"gvar", b"CFF ", b"CFF2", b"COLR", b"sbix",
        ] {
            assert!(
                !tables.contains(&tag(kept)),
                "{}",
                String::from_utf8_lossy(kept)
            );
            assert!(typeface.table(tag(kept)).is_none());
        }
        assert!(!typeface.table(tag(b"cmap")).unwrap().is_empty());
    }

    #[test]
    fn a_glyph_is_drawn_from_a_face_handed_out_and_from_no_other() {
        let Some(face) = system_face() else {
            return;
        };
        let glyph = glyph_of(&face, 'a');
        let mut fonts = fonts_over(Faces {
            faces: vec![face],
            ..Faces::default()
        });
        let typeface = fonts.system_font(13.0, REGULAR).unwrap();
        let font = at_default(&typeface);
        let coverage = fonts.glyph_alpha(&font, glyph, 32.0, 0.0).unwrap();
        assert_eq!(
            coverage.data.len(),
            (coverage.width * coverage.height) as usize
        );
        assert!(coverage.data.iter().any(|&covered| covered > 0));
        assert!(!fonts.glyph_path(&font, glyph, 32.0).unwrap().is_empty());
        let never_handed_out = Font {
            typeface: typeface.id() + 1,
            variation_coordinates: Vec::new(),
            embolden: 0.0,
            skew: 0.0,
        };
        assert!(
            fonts
                .glyph_alpha(&never_handed_out, glyph, 32.0, 0.0)
                .is_none()
        );
    }

    #[test]
    fn a_font_with_fake_bold_draws_a_wider_coverage_than_one_without() {
        let Some(face) = system_face() else {
            return;
        };
        let glyph = glyph_of(&face, 'l');
        let mut fonts = fonts_over(Faces {
            faces: vec![face],
            ..Faces::default()
        });
        let typeface = fonts.system_font(13.0, REGULAR).unwrap();
        let plain = at_default(&typeface);
        let bold = Font {
            embolden: 1.0 / 24.0,
            ..at_default(&typeface)
        };
        let plain = fonts.glyph_alpha(&plain, glyph, 64.0, 0.0).unwrap();
        let bold = fonts.glyph_alpha(&bold, glyph, 64.0, 0.0).unwrap();
        let inked = |image: &GlyphImage| {
            image
                .data
                .iter()
                .map(|&covered| u64::from(covered))
                .sum::<u64>()
        };
        assert!(
            bold.width > plain.width,
            "{} against {}",
            bold.width,
            plain.width
        );
        assert!(inked(&bold) > inked(&plain));
    }

    #[test]
    fn a_variable_face_is_drawn_at_its_named_instances() {
        let Some(face) = system_face() else {
            return;
        };
        let instances = valo_text::Font::instances_from_data(face.shared_data(), face.index());
        let bold = valo_text::FontAttrs {
            weight: 700,
            ..valo_text::FontAttrs::default()
        };
        let Some(bold) = valo_text::match_style(&instances, bold)
            .filter(|instance| !instance.variation_coordinates().is_empty())
        else {
            return;
        };
        let glyph = glyph_of(&face, 'a');
        let mut fonts = fonts_over(Faces {
            faces: vec![face],
            ..Faces::default()
        });
        let typeface = fonts.system_font(13.0, REGULAR).unwrap();
        let coordinates: Vec<(Tag, f32)> = bold
            .variation_coordinates()
            .iter()
            .map(|(tag, value)| (u32::from_be_bytes(*tag), *value))
            .collect();
        let inked = |fonts: &mut ProgramFonts, variation_coordinates: Vec<(Tag, f32)>| {
            let font = Font {
                typeface: typeface.id(),
                variation_coordinates,
                embolden: 0.0,
                skew: 0.0,
            };
            let coverage = fonts.glyph_alpha(&font, glyph, 32.0, 0.0)?;
            Some(
                coverage
                    .data
                    .iter()
                    .map(|&covered| u64::from(covered))
                    .sum::<u64>(),
            )
        };
        let regular_ink = inked(&mut fonts, Vec::new()).unwrap();
        let bold_ink = inked(&mut fonts, coordinates.clone()).unwrap();
        assert!(
            bold_ink > regular_ink,
            "bold inks more: {bold_ink} against {regular_ink}"
        );
        let mut reversed = coordinates.clone();
        reversed.reverse();
        assert_eq!(
            inked(&mut fonts, reversed),
            Some(bold_ink),
            "by tag, in any order"
        );
        let between: Vec<(Tag, f32)> = coordinates
            .iter()
            .map(|&(tag, value)| (tag, value + 0.5))
            .collect();
        assert_eq!(
            inked(&mut fonts, between),
            None,
            "no named instance is there"
        );
    }

    #[test]
    fn a_style_survives_the_round_trip() {
        let style = Style {
            weight: 700,
            width: 87.5,
            slant: Slant::Oblique,
        };
        let back = style_to(style_of(style));
        assert_eq!(
            (back.weight, back.width, back.slant),
            (style.weight, style.width, style.slant)
        );
    }

    /// A program's fonts live in its store, which crosses to the program's thread.
    #[test]
    fn a_programs_fonts_cross_threads() {
        fn assert_sent<T: Send>() {}
        assert_sent::<ProgramFonts>();
        assert_sent::<Typeface>();
    }
}
