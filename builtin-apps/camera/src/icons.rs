//! The icons the app shows, glyphs of the Fluent System Icons font that Waft's WinUI kit
//! bundles, by their codepoints in its `codepoints.json`; the kit's own `FluentSymbol`s name only
//! the glyphs its controls draw.

use waft_winui::FontIcon;

pub(crate) const BACK: char = '\u{f15c}';
pub(crate) const CAMERA: char = '\u{f255}';
pub(crate) const CAMERA_OFF: char = '\u{e2a1}';
pub(crate) const CAMERA_SWITCH: char = '\u{f25a}';
pub(crate) const DELETE: char = '\u{f34d}';
pub(crate) const EXPORT: char = '\u{e0cc}';
pub(crate) const IMAGE: char = '\u{f489}';
pub(crate) const LOCK: char = '\u{e790}';

/// `glyph` at `size`, in the colour of the text around it.
pub(crate) fn icon(glyph: char, size: f64) -> FontIcon {
    FontIcon::new(glyph.to_string()).font_size(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The codepoints `codepoints.json` gives each icon's name, its 24-pixel regular glyph.
    #[test]
    fn each_icon_is_its_glyph_in_the_bundled_font() {
        let named = [
            (BACK, 0xf15c),
            (CAMERA, 0xf255),
            (CAMERA_OFF, 0xe2a1),
            (CAMERA_SWITCH, 0xf25a),
            (DELETE, 0xf34d),
            (EXPORT, 0xe0cc),
            (IMAGE, 0xf489),
            (LOCK, 0xe790),
        ];
        for (glyph, codepoint) in named {
            assert_eq!(u32::from(glyph), codepoint);
        }
    }
}
