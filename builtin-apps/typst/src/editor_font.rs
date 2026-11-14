//! The font the markup is written in: DejaVu Sans Mono, regular and bold, the fixed-width font
//! the program carries for Typst's code, registered with Waft so the editor looks the same on
//! every system.

use waft::{App, PaintingBinding};

use crate::bundled_fonts::DEJAVU_SANS_MONO;

/// The family the editor's text names, and the font's own.
pub(crate) const EDITOR_FONT: &str = "DejaVu Sans Mono";

/// Registers the editor's font; after `run_app_with`, which makes the font collection it joins.
pub(crate) fn install_editor_font(app: &mut App) {
    for face in DEJAVU_SANS_MONO {
        PaintingBinding::instance(app).register_font(app, EDITOR_FONT, face.to_vec());
    }
}
