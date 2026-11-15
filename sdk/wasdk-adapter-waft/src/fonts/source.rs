//! valo's [`FontSource`] over the engine's `fonts` functions, answering as native Waft's
//! `SystemFontSource` answers (`waft-embedder`'s `fonts.rs`): a family by its name, the two
//! names Flutter's engine defines for the platform's user-interface font, and the face that
//! covers a character in the user's languages.
//!
//! Each typeface the engine hands out is parsed once, the first time: its tables rebuilt into a
//! font file with valo's `assemble`, and its fonts, one for a static face and one per named
//! instance of a variable one, kept by the typeface's id with that id as their remote id. What is
//! handed out are clones, which share the parse and are the same fonts to valo; on Apple's
//! systems the system font is one typeface of hundreds of named instances, which a debug
//! component takes most of a second to parse. Where a request names a style, the instance nearest
//! it is picked with valo's `match_style`, so a bold span's fallback arrives bold.
//!
//! The invariant: a font is handed out only for a typeface the engine has just answered the
//! request with; what is kept spares the parse and nothing else. The engine's id names one face
//! for as long as the program runs, so a face it stops serving when the installed fonts change is
//! never answered again, and its fonts are not handed out again.

use std::collections::HashMap;
use std::sync::Arc;

use valo::{Font, FontAttrs, FontSource, match_style};
use valo_fontmgr::Tag;
use wasdk::fonts::{self, Slant, Style, Typeface};
use wasdk::system;

use crate::system::locale_of;

/// Flutter's `CupertinoSystemText`: the platform's user-interface font at text sizes.
const CUPERTINO_TEXT: &str = "CupertinoSystemText";
/// Flutter's `CupertinoSystemDisplay`: the same font at and above the display breakpoint.
const CUPERTINO_DISPLAY: &str = "CupertinoSystemDisplay";
/// The point size Flutter's engine makes its default text-size lookup at.
const TEXT_SIZE: f32 = 17.0;
/// Flutter's engine's `kSFProDisplayBreakPoint`.
const DISPLAY_SIZE: f32 = 29.0;

/// The program's fonts, as the engine answers for them.
#[derive(Default)]
pub(crate) struct ProgramFonts {
    /// Each typeface's fonts, parsed the first time the engine hands it out, by the engine's id
    /// for it.
    typefaces: HashMap<u64, Vec<Font>>,
}

impl ProgramFonts {
    pub(crate) fn new() -> ProgramFonts {
        ProgramFonts::default()
    }

    /// Every weight of the platform's user-interface font for text at `size` points, one font per
    /// weight the platform has, as native Waft registers it under Flutter's names.
    fn system_family(&mut self, size: f32) -> Vec<Font> {
        let mut family: Vec<Font> = Vec::new();
        for weight in (100..=900).step_by(100) {
            let attrs = FontAttrs {
                weight,
                ..FontAttrs::default()
            };
            let Some(typeface) = fonts::system_font(size, style_of(attrs)) else {
                continue;
            };
            let Some(font) = self.nearest_instance(&typeface, attrs) else {
                continue;
            };
            if !family.iter().any(|known| known.attrs() == font.attrs()) {
                family.push(font);
            }
        }
        family
    }

    /// Every font of every face the platform files under `name`.
    fn named_family(&mut self, name: &str) -> Vec<Font> {
        fonts::family(name)
            .iter()
            .flat_map(|typeface| self.instances(typeface).to_vec())
            .collect()
    }

    /// The instance of `typeface` nearest `attrs`.
    fn nearest_instance(&mut self, typeface: &Typeface, attrs: FontAttrs) -> Option<Font> {
        match_style(self.instances(typeface), attrs).cloned()
    }

    /// Every registrable instance of `typeface`, parsed the first time it is handed out.
    fn instances(&mut self, typeface: &Typeface) -> &[Font] {
        self.typefaces
            .entry(typeface.id())
            .or_insert_with(|| parsed_instances(typeface))
    }
}

impl FontSource for ProgramFonts {
    fn family(&mut self, name: &str) -> Vec<Font> {
        match system_font_size(name) {
            Some(size) => self.system_family(size),
            None => self.named_family(name),
        }
    }

    fn face_for_codepoint(&mut self, codepoint: char, attrs: FontAttrs) -> Option<Font> {
        let locales = language_tags(system::locales());
        let typeface = fonts::match_character(None, style_of(attrs), &locales, codepoint)?;
        self.nearest_instance(&typeface, attrs)
            .filter(|font| font.covers(codepoint))
    }
}

/// The size the platform's user-interface font is asked at for `family`, one of Flutter's two
/// names for that font; none for any other family. On macOS a lookup of the system font's own
/// family name finds nothing, so this is the only way to it.
fn system_font_size(family: &str) -> Option<f32> {
    match family {
        CUPERTINO_TEXT => Some(TEXT_SIZE),
        CUPERTINO_DISPLAY => Some(DISPLAY_SIZE),
        _ => None,
    }
}

/// Every registrable instance of `typeface`, each naming it by its id, from its font file.
fn parsed_instances(typeface: &Typeface) -> Vec<Font> {
    Font::instances_from_data_with_remote_id(Arc::new(font_file(typeface)), 0, typeface.id())
}

/// A single-face font file holding the tables the engine serves for `typeface`, in its order.
fn font_file(typeface: &Typeface) -> Vec<u8> {
    let tables: Vec<(Tag, Vec<u8>)> = typeface
        .tables()
        .into_iter()
        .filter_map(|tag| {
            let bytes = typeface.table(tag)?;
            Some((Tag::new(&tag.to_be_bytes()), bytes))
        })
        .collect();
    let borrowed: Vec<(Tag, &[u8])> = tables
        .iter()
        .map(|(tag, bytes)| (*tag, bytes.as_slice()))
        .collect();
    valo_fontmgr::assemble(&borrowed)
}

/// valo's attributes as the `fonts` interface's style.
fn style_of(attrs: FontAttrs) -> Style {
    Style {
        weight: attrs.weight,
        width: attrs.stretch,
        slant: if attrs.italic {
            Slant::Italic
        } else {
            Slant::Upright
        },
    }
}

/// The user's languages as BCP 47 tags, most preferred first.
fn language_tags(locales: Vec<system::Locale>) -> Vec<String> {
    locales
        .into_iter()
        .map(|locale| locale_of(locale).to_language_tag())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flutters_names_for_the_system_font_ask_for_it_at_text_and_display_sizes() {
        assert_eq!(system_font_size("CupertinoSystemText"), Some(17.0));
        assert_eq!(system_font_size("CupertinoSystemDisplay"), Some(29.0));
        assert_eq!(system_font_size("Helvetica"), None);
        assert_eq!(system_font_size(".SF NS"), None);
    }

    #[test]
    fn a_style_keeps_its_weight_and_width_and_an_italic_slants() {
        let bold_italic = FontAttrs {
            weight: 700,
            italic: true,
            stretch: 87.5,
        };
        let style = style_of(bold_italic);
        assert_eq!((style.weight, style.width), (700, 87.5));
        assert_eq!(style.slant, Slant::Italic);
        assert_eq!(style_of(FontAttrs::default()).slant, Slant::Upright);
    }

    #[test]
    fn the_users_languages_are_bcp_47_tags_in_their_order() {
        let locales = vec![
            system::Locale {
                language: String::from("zh"),
                script: Some(String::from("Hant")),
                region: Some(String::from("TW")),
            },
            system::Locale {
                language: String::from("en"),
                script: None,
                region: Some(String::from("GB")),
            },
        ];
        assert_eq!(language_tags(locales), ["zh-Hant-TW", "en-GB"]);
    }
}
