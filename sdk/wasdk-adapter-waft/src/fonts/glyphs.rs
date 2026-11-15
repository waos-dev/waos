//! valo's [`GlyphRaster`] over the engine's glyph calls: a font the engine handed out is drawn
//! by the engine, named by its typeface's id and its point on the typeface's axes; a font the
//! program registered itself is drawn here, from its own bytes. Either is drawn with the fake bold
//! and italic valo asks for, which a glyph call carries in its font.

use std::sync::Arc;

use valo::{
    Cap, FontSynthesis, GlyphImage, GlyphRaster, GlyphStroke, Join, Path, PathBuilder, Rasterizer,
};
use wasdk::fonts::{self, PathElement};

/// The glyphs of the program's fonts, as valo's renderer asks for them.
#[derive(Default)]
pub(crate) struct ProgramGlyphs {
    /// What draws the glyphs of a font with no remote id: one the program registered itself.
    own: Rasterizer,
}

impl ProgramGlyphs {
    pub(crate) fn new() -> ProgramGlyphs {
        ProgramGlyphs::default()
    }
}

impl GlyphRaster for ProgramGlyphs {
    fn alpha(
        &mut self,
        font: &valo::Font,
        glyph: u32,
        px: f32,
        synthesis: FontSynthesis,
        dx: f32,
    ) -> Option<GlyphImage> {
        match font_of(font, synthesis) {
            Some(named) => fonts::glyph_alpha(&named, glyph, px, dx).map(image_of),
            None => self.own.alpha(font, glyph, px, synthesis, dx),
        }
    }

    fn stroked(
        &mut self,
        font: &valo::Font,
        glyph: u32,
        px: f32,
        synthesis: FontSynthesis,
        dx: f32,
        stroke: &GlyphStroke,
    ) -> Option<GlyphImage> {
        match font_of(font, synthesis) {
            Some(named) => {
                fonts::glyph_stroked(&named, glyph, px, dx, stroke_of(stroke)).map(image_of)
            }
            None => self.own.stroked(font, glyph, px, synthesis, dx, stroke),
        }
    }

    fn sdf(
        &mut self,
        font: &valo::Font,
        glyph: u32,
        px: f32,
        synthesis: FontSynthesis,
    ) -> Option<GlyphImage> {
        match font_of(font, synthesis) {
            Some(named) => fonts::glyph_sdf(&named, glyph, px).map(image_of),
            None => self.own.sdf(font, glyph, px, synthesis),
        }
    }

    fn color(&mut self, font: &valo::Font, glyph: u32, px: f32) -> Option<GlyphImage> {
        match font_of(font, FontSynthesis::NONE) {
            Some(named) => fonts::glyph_color(&named, glyph, px).map(image_of),
            None => self.own.color(font, glyph, px),
        }
    }

    fn path(
        &mut self,
        font: &valo::Font,
        glyph: u32,
        px: f32,
        synthesis: FontSynthesis,
    ) -> Option<Arc<Path>> {
        match font_of(font, synthesis) {
            Some(named) => fonts::glyph_path(&named, glyph, px).map(|outline| path_of(&outline)),
            None => GlyphRaster::path(&mut self.own, font, glyph, px, synthesis),
        }
    }
}

/// The font a glyph call names for `font` with `synthesis`: its typeface's id, its point on the
/// typeface's axes, and its fake bold and italic. None for a font with no remote id, which the
/// engine did not hand out.
fn font_of(font: &valo::Font, synthesis: FontSynthesis) -> Option<fonts::Font> {
    Some(fonts::Font {
        typeface: font.remote_id()?,
        variation_coordinates: font
            .variation_coordinates()
            .iter()
            .map(|(tag, value)| (u32::from_be_bytes(*tag), *value))
            .collect(),
        embolden: synthesis.embolden,
        skew: synthesis.skew,
    })
}

fn image_of(image: fonts::GlyphImage) -> GlyphImage {
    GlyphImage {
        width: image.width,
        height: image.height,
        left: image.left,
        top: image.top,
        data: image.data,
    }
}

fn stroke_of(stroke: &GlyphStroke) -> fonts::GlyphStroke {
    fonts::GlyphStroke {
        width: stroke.width,
        miter_limit: stroke.miter_limit,
        cap: match stroke.cap {
            Cap::Butt => fonts::Cap::Butt,
            Cap::Round => fonts::Cap::Round,
            Cap::Square => fonts::Cap::Square,
        },
        join: match stroke.join {
            Join::Miter => fonts::Join::Miter,
            Join::Round => fonts::Join::Round,
            Join::Bevel => fonts::Join::Bevel,
        },
    }
}

/// The outline the engine answered, as valo's path.
fn path_of(outline: &[PathElement]) -> Arc<Path> {
    let mut path = PathBuilder::new();
    for element in outline {
        match *element {
            PathElement::MoveTo(to) => path.move_to(point_of(to)),
            PathElement::LineTo(to) => path.line_to(point_of(to)),
            PathElement::QuadTo((control, to)) => path.quad_to(point_of(control), point_of(to)),
            PathElement::ConicTo((control, to, weight)) => {
                path.conic_to(point_of(control), point_of(to), weight)
            }
            PathElement::CubicTo((first, second, to)) => {
                path.cubic_to(point_of(first), point_of(second), point_of(to))
            }
            PathElement::Close => path.close(),
        };
    }
    path.build()
}

fn point_of(point: fonts::Point) -> (f32, f32) {
    (point.x, point.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stroke_keeps_its_width_limit_cap_and_join() {
        let stroke = stroke_of(&GlyphStroke {
            width: 2.5,
            cap: Cap::Round,
            join: Join::Bevel,
            miter_limit: 4.0,
        });
        assert_eq!(stroke.width, 2.5);
        assert_eq!(stroke.miter_limit, 4.0);
        assert_eq!(stroke.cap, fonts::Cap::Round);
        assert_eq!(stroke.join, fonts::Join::Bevel);
    }

    #[test]
    fn an_image_keeps_its_placement_and_pixels() {
        let image = image_of(fonts::GlyphImage {
            width: 2,
            height: 1,
            left: -1,
            top: 9,
            data: vec![0, 255],
        });
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!((image.left, image.top), (-1, 9));
        assert_eq!(image.data, [0, 255]);
    }

    #[test]
    fn an_outline_becomes_a_path_through_the_same_points() {
        let at = |x, y| fonts::Point { x, y };
        let path = path_of(&[
            PathElement::MoveTo(at(0.0, 0.0)),
            PathElement::LineTo(at(10.0, 0.0)),
            PathElement::QuadTo((at(10.0, 10.0), at(0.0, 10.0))),
            PathElement::Close,
        ]);
        let bounds = path.bounds();
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (0.0, 0.0, 10.0, 10.0)
        );
    }
}
