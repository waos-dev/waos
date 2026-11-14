//! The terminal's colours: what a cell's colour, as a command named it, is drawn as. A command
//! names the terminal's own text or background colour, one of 256 by number (the first sixteen the
//! theme's, the rest fixed for every terminal), or a colour outright.

use waft_embedder::Brightness;

/// A colour as red, green and blue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Rgb(pub(crate) u8, pub(crate) u8, pub(crate) u8);

/// The colours a theme chooses: the terminal's own, and the sixteen a command names by number,
/// the eight dim ones first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Palette {
    pub(crate) background: Rgb,
    pub(crate) foreground: Rgb,
    pub(crate) cursor: Rgb,
    /// What selected cells are drawn on.
    pub(crate) selection: Rgb,
    ansi: [Rgb; 16],
}

impl Palette {
    /// Dark text on white, for the system's light look.
    pub(crate) const LIGHT: Palette = Palette {
        background: Rgb(0xff, 0xff, 0xff),
        foreground: Rgb(0x1f, 0x23, 0x28),
        cursor: Rgb(0x5c, 0x62, 0x6b),
        selection: Rgb(0xbb, 0xd6, 0xfb),
        ansi: [
            Rgb(0x24, 0x29, 0x2f),
            Rgb(0xc4, 0x2b, 0x1c),
            Rgb(0x11, 0x7a, 0x37),
            Rgb(0x8a, 0x64, 0x00),
            Rgb(0x0b, 0x5c, 0xc4),
            Rgb(0x8b, 0x3f, 0xc7),
            Rgb(0x0d, 0x7f, 0x8c),
            Rgb(0x6e, 0x77, 0x81),
            Rgb(0x57, 0x60, 0x6a),
            Rgb(0xdb, 0x3b, 0x2b),
            Rgb(0x1a, 0x93, 0x45),
            Rgb(0xa8, 0x7b, 0x00),
            Rgb(0x1f, 0x75, 0xe0),
            Rgb(0xa3, 0x55, 0xde),
            Rgb(0x14, 0x99, 0xa8),
            Rgb(0x8c, 0x95, 0x9f),
        ],
    };

    /// Light text on near black, for the system's dark look.
    pub(crate) const DARK: Palette = Palette {
        background: Rgb(0x1c, 0x1c, 0x1e),
        foreground: Rgb(0xe3, 0xe5, 0xe8),
        cursor: Rgb(0xb4, 0xb8, 0xbf),
        selection: Rgb(0x2c, 0x4f, 0x7c),
        ansi: [
            Rgb(0x3b, 0x3f, 0x46),
            Rgb(0xf0, 0x71, 0x64),
            Rgb(0x5f, 0xc9, 0x7b),
            Rgb(0xe0, 0xb3, 0x4f),
            Rgb(0x62, 0xa6, 0xf5),
            Rgb(0xc2, 0x8a, 0xf0),
            Rgb(0x4f, 0xc4, 0xd1),
            Rgb(0xc9, 0xcd, 0xd3),
            Rgb(0x6b, 0x72, 0x7c),
            Rgb(0xff, 0x8f, 0x84),
            Rgb(0x7d, 0xe0, 0x96),
            Rgb(0xf2, 0xca, 0x6d),
            Rgb(0x86, 0xbd, 0xff),
            Rgb(0xd6, 0xa8, 0xff),
            Rgb(0x73, 0xdb, 0xe6),
            Rgb(0xff, 0xff, 0xff),
        ],
    };

    /// The palette for the system's `look`, light or dark.
    pub(crate) fn of_look(look: Brightness) -> Palette {
        match look {
            Brightness::Dark => Palette::DARK,
            Brightness::Light => Palette::LIGHT,
        }
    }

    /// What text of `color` is drawn in; `bold` text named by one of the eight dim colours takes
    /// its bright one, as terminals have drawn bold since they had eight colours.
    pub(crate) fn text(&self, color: vt100::Color, bold: bool) -> Rgb {
        match color {
            vt100::Color::Default => self.foreground,
            vt100::Color::Idx(index) if bold && index < 8 => self.indexed(index + 8),
            vt100::Color::Idx(index) => self.indexed(index),
            vt100::Color::Rgb(red, green, blue) => Rgb(red, green, blue),
        }
    }

    /// What a cell of `color` is filled with; none for the terminal's own background, which is
    /// drawn once behind every cell.
    pub(crate) fn fill(&self, color: vt100::Color) -> Option<Rgb> {
        match color {
            vt100::Color::Default => None,
            vt100::Color::Idx(index) => Some(self.indexed(index)),
            vt100::Color::Rgb(red, green, blue) => Some(Rgb(red, green, blue)),
        }
    }

    /// One of the 256 colours: the theme's sixteen, a 6×6×6 cube of colours, then 24 greys.
    fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..16 => self.ansi[usize::from(index)],
            16..232 => {
                let cube = index - 16;
                Rgb(
                    cube_level(cube / 36),
                    cube_level(cube / 6 % 6),
                    cube_level(cube % 6),
                )
            }
            232.. => {
                let grey = 8 + 10 * (index - 232);
                Rgb(grey, grey, grey)
            }
        }
    }
}

/// One of the six levels a channel takes in xterm's colour cube.
fn cube_level(step: u8) -> u8 {
    match step {
        0 => 0,
        step => 55 + 40 * step,
    }
}

/// `color` at half its strength over `background`: how dim text is drawn.
pub(crate) fn dimmed(color: Rgb, background: Rgb) -> Rgb {
    let halfway = |channel: u8, behind: u8| ((u16::from(channel) + u16::from(behind)) / 2) as u8;
    Rgb(
        halfway(color.0, background.0),
        halfway(color.1, background.1),
        halfway(color.2, background.2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cube_and_the_greys_are_xterms() {
        let palette = Palette::DARK;
        assert_eq!(palette.indexed(16), Rgb(0, 0, 0));
        assert_eq!(palette.indexed(196), Rgb(255, 0, 0));
        assert_eq!(palette.indexed(231), Rgb(255, 255, 255));
        assert_eq!(palette.indexed(232), Rgb(8, 8, 8));
        assert_eq!(palette.indexed(255), Rgb(238, 238, 238));
    }

    #[test]
    fn bold_text_in_a_dim_colour_takes_the_bright_one() {
        let palette = Palette::DARK;
        let red = vt100::Color::Idx(1);
        assert_eq!(palette.text(red, false), palette.ansi[1]);
        assert_eq!(palette.text(red, true), palette.ansi[9]);
    }

    #[test]
    fn the_terminals_own_background_is_not_filled() {
        assert_eq!(Palette::LIGHT.fill(vt100::Color::Default), None);
        assert_eq!(
            Palette::LIGHT.fill(vt100::Color::Rgb(1, 2, 3)),
            Some(Rgb(1, 2, 3))
        );
    }
}
