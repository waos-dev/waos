//! The icons the app shows, glyphs of the Fluent System Icons font that Waft's WinUI kit
//! bundles, by their codepoints in its `codepoints.json`; the kit's own `FluentSymbol`s name only
//! the glyphs its controls draw.

use waft_winui::FontIcon;

pub(crate) const PLAY: char = '\u{f605}';
pub(crate) const PAUSE: char = '\u{f5a1}';
pub(crate) const SKIP_BACK_10: char = '\u{eafb}';
pub(crate) const SKIP_FORWARD_10: char = '\u{eb00}';
pub(crate) const SPEAKER: char = '\u{eb42}';
pub(crate) const SPEAKER_MUTE: char = '\u{eb4a}';
pub(crate) const SUBTITLES: char = '\u{ebcc}';
pub(crate) const REPLAY: char = '\u{f13f}';
pub(crate) const BACK: char = '\u{f15b}';
pub(crate) const LIBRARY: char = '\u{e762}';
pub(crate) const HISTORY: char = '\u{f47e}';
pub(crate) const FOLDER: char = '\u{f418}';
pub(crate) const FOLDER_ADD: char = '\u{f41c}';
pub(crate) const DOCUMENT: char = '\u{f378}';
pub(crate) const LINK: char = '\u{f4e4}';
pub(crate) const VIDEO: char = '\u{f84c}';
pub(crate) const DELETE: char = '\u{f34c}';

/// `glyph` at `size`, in the colour of the text around it.
pub(crate) fn icon(glyph: char, size: f64) -> FontIcon {
    FontIcon::new(glyph.to_string()).font_size(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The codepoints `codepoints.json` gives each icon's name.
    #[test]
    fn each_icon_is_its_glyph_in_the_bundled_font() {
        let named = [
            (PLAY, 62981),
            (PAUSE, 62881),
            (SKIP_BACK_10, 60155),
            (SKIP_FORWARD_10, 60160),
            (SPEAKER, 60226),
            (SPEAKER_MUTE, 60234),
            (SUBTITLES, 60364),
            (REPLAY, 61759),
            (BACK, 61787),
            (LIBRARY, 59234),
            (HISTORY, 62590),
            (FOLDER, 62488),
            (FOLDER_ADD, 62492),
            (DOCUMENT, 62328),
            (LINK, 62692),
            (VIDEO, 63564),
            (DELETE, 62284),
        ];
        for (glyph, codepoint) in named {
            assert_eq!(u32::from(glyph), codepoint);
        }
    }
}
