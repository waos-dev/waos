//! The font files the program carries, in `fonts/` beside its source: Libertinus Serif, which
//! Typst sets text in unless a document says otherwise; New Computer Modern Math, its font for
//! formulas; and DejaVu Sans Mono, its font for code, which the editor is written in too.
//!
//! These are the fonts Typst's own tools bundle, less New Computer Modern's text faces, whose
//! regular one is under the GPL. `fonts/LICENSES.txt` has each font's terms.

/// Libertinus Serif: regular, bold, italic and bold italic.
pub(crate) const LIBERTINUS_SERIF: [&[u8]; 4] = [
    include_bytes!("../fonts/LibertinusSerif-Regular.otf"),
    include_bytes!("../fonts/LibertinusSerif-Bold.otf"),
    include_bytes!("../fonts/LibertinusSerif-Italic.otf"),
    include_bytes!("../fonts/LibertinusSerif-BoldItalic.otf"),
];

/// New Computer Modern Math: regular and bold.
pub(crate) const NEW_COMPUTER_MODERN_MATH: [&[u8]; 2] = [
    include_bytes!("../fonts/NewCMMath-Regular.otf"),
    include_bytes!("../fonts/NewCMMath-Bold.otf"),
];

/// DejaVu Sans Mono, upright: regular and bold.
pub(crate) const DEJAVU_SANS_MONO: [&[u8]; 2] = [
    include_bytes!("../fonts/DejaVuSansMono.ttf"),
    include_bytes!("../fonts/DejaVuSansMono-Bold.ttf"),
];

/// DejaVu Sans Mono, oblique: regular and bold.
pub(crate) const DEJAVU_SANS_MONO_OBLIQUE: [&[u8]; 2] = [
    include_bytes!("../fonts/DejaVuSansMono-Oblique.ttf"),
    include_bytes!("../fonts/DejaVuSansMono-BoldOblique.ttf"),
];
