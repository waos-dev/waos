//! A program's glyph calls: the fonts made of the faces it named, at the points on their axes it
//! named, and valo's rasterizer drawing them, each answer in the interface's terms, with the fake
//! bold and italic the call's font asks for.
//!
//! The invariant: a font is made once, from its face's whole file, and kept for as long as the
//! program runs, so a glyph call after the first parses nothing. A point on a face's axes is
//! drawn when it is the face's default or one of its named instances: valo makes fonts at those
//! alone.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use valo_text::{FontSynthesis, Rasterizer};

use super::typefaces::Typefaces;
use crate::bindings::wasdk::platform::fonts::{
    Cap, Font, GlyphImage, GlyphStroke, Join, PathElement, Point, Tag,
};

/// A font made: its face's id, and its point on the face's axes, sorted by tag, with each value
/// compared bit for bit.
type FontKey = (u64, Vec<(Tag, u32)>);

/// The fonts a program's glyph calls named, and what draws them.
#[derive(Default)]
pub(super) struct Glyphs {
    fonts: HashMap<FontKey, valo_text::Font>,
    /// The faces whose named instances are among `fonts`, all made at the first call naming one.
    instances_made: HashSet<u64>,
    rasterizer: Rasterizer,
}

impl Glyphs {
    /// Coverage of the glyph's outline, one byte per pixel.
    pub(super) fn alpha(
        &mut self,
        typefaces: &Typefaces,
        font: &Font,
        glyph: u32,
        px: f32,
        dx: f32,
    ) -> Option<GlyphImage> {
        self.draw(typefaces, font, |rasterizer, made| {
            rasterizer.alpha(made, glyph, px, synthesis_of(font), dx)
        })
    }

    /// Coverage of the glyph's outline widened by `stroke`.
    pub(super) fn stroked(
        &mut self,
        typefaces: &Typefaces,
        font: &Font,
        glyph: u32,
        px: f32,
        dx: f32,
        stroke: GlyphStroke,
    ) -> Option<GlyphImage> {
        let stroke = stroke_of(stroke);
        self.draw(typefaces, font, |rasterizer, made| {
            rasterizer.stroked(made, glyph, px, synthesis_of(font), dx, &stroke)
        })
    }

    /// The glyph as a signed distance field, one byte per pixel.
    pub(super) fn sdf(
        &mut self,
        typefaces: &Typefaces,
        font: &Font,
        glyph: u32,
        px: f32,
    ) -> Option<GlyphImage> {
        self.draw(typefaces, font, |rasterizer, made| {
            rasterizer.sdf(made, glyph, px, synthesis_of(font))
        })
    }

    /// The glyph's own colours, premultiplied RGBA.
    pub(super) fn color(
        &mut self,
        typefaces: &Typefaces,
        font: &Font,
        glyph: u32,
        px: f32,
    ) -> Option<GlyphImage> {
        self.draw(typefaces, font, |rasterizer, made| {
            rasterizer.color(made, glyph, px)
        })
    }

    /// The glyph's outline at `px`, y down, origin on the baseline.
    pub(super) fn path(
        &mut self,
        typefaces: &Typefaces,
        font: &Font,
        glyph: u32,
        px: f32,
    ) -> Option<Vec<PathElement>> {
        let made = self.font(typefaces, font)?;
        let path = valo_text::glyph_path(made, glyph, px, synthesis_of(font))?;
        Some(path.elements().map(element_to).collect())
    }

    /// `render` over the font `font` names, its image in the interface's terms.
    fn draw(
        &mut self,
        typefaces: &Typefaces,
        font: &Font,
        render: impl FnOnce(&mut Rasterizer, &valo_text::Font) -> Option<valo_text::GlyphImage>,
    ) -> Option<GlyphImage> {
        let key = key_of(font);
        self.make(typefaces, font, &key);
        let made = self.fonts.get(&key)?;
        render(&mut self.rasterizer, made).map(image_to)
    }

    /// The font `font` names, made now if this is the first call naming it.
    fn font(&mut self, typefaces: &Typefaces, font: &Font) -> Option<&valo_text::Font> {
        let key = key_of(font);
        self.make(typefaces, font, &key);
        self.fonts.get(&key)
    }

    /// Makes the font `font` names, as `key`, unless it is made already: its face itself for no
    /// coordinates, or else every named instance of the face.
    fn make(&mut self, typefaces: &Typefaces, font: &Font, key: &FontKey) {
        if self.fonts.contains_key(key) {
            return;
        }
        let Some((data, index)) = typefaces.face(font.typeface) else {
            return;
        };
        if font.variation_coordinates.is_empty() {
            if let Some(made) = valo_text::Font::from_data(Arc::clone(data), *index) {
                self.fonts.insert(key.clone(), made);
            }
        } else if self.instances_made.insert(font.typeface) {
            for instance in valo_text::Font::instances_from_data(Arc::clone(data), *index) {
                let coordinates = instance
                    .variation_coordinates()
                    .iter()
                    .map(|(tag, value)| (u32::from_be_bytes(*tag), *value));
                self.fonts
                    .insert(key_at(font.typeface, coordinates), instance);
            }
        }
    }
}

/// The fake bold and italic `font` asks for.
fn synthesis_of(font: &Font) -> FontSynthesis {
    FontSynthesis {
        embolden: font.embolden,
        skew: font.skew,
    }
}

/// The key of the font `font` names.
fn key_of(font: &Font) -> FontKey {
    key_at(font.typeface, font.variation_coordinates.iter().copied())
}

/// The key of face `typeface` at `coordinates`, in whatever order they come.
fn key_at(typeface: u64, coordinates: impl Iterator<Item = (Tag, f32)>) -> FontKey {
    let mut coordinates: Vec<(Tag, u32)> = coordinates
        .map(|(tag, value)| (tag, value.to_bits()))
        .collect();
    coordinates.sort_unstable();
    (typeface, coordinates)
}

fn stroke_of(stroke: GlyphStroke) -> valo_text::GlyphStroke {
    valo_text::GlyphStroke {
        width: stroke.width,
        miter_limit: stroke.miter_limit,
        cap: match stroke.cap {
            Cap::Butt => valo_geometry::Cap::Butt,
            Cap::Round => valo_geometry::Cap::Round,
            Cap::Square => valo_geometry::Cap::Square,
        },
        join: match stroke.join {
            Join::Miter => valo_geometry::Join::Miter,
            Join::Round => valo_geometry::Join::Round,
            Join::Bevel => valo_geometry::Join::Bevel,
        },
    }
}

fn image_to(image: valo_text::GlyphImage) -> GlyphImage {
    GlyphImage {
        width: image.width,
        height: image.height,
        left: image.left,
        top: image.top,
        data: image.data,
    }
}

fn point_to(point: valo_geometry::Point) -> Point {
    Point {
        x: point.x,
        y: point.y,
    }
}

fn element_to(element: valo_geometry::PathElement) -> PathElement {
    match element {
        valo_geometry::PathElement::MoveTo(to) => PathElement::MoveTo(point_to(to)),
        valo_geometry::PathElement::LineTo(to) => PathElement::LineTo(point_to(to)),
        valo_geometry::PathElement::QuadTo(control, to) => {
            PathElement::QuadTo((point_to(control), point_to(to)))
        }
        valo_geometry::PathElement::ConicTo(control, to, weight) => {
            PathElement::ConicTo((point_to(control), point_to(to), weight))
        }
        valo_geometry::PathElement::CubicTo(first, second, to) => {
            PathElement::CubicTo((point_to(first), point_to(second), point_to(to)))
        }
        valo_geometry::PathElement::Close => PathElement::Close,
    }
}
