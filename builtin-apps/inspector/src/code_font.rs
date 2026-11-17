//! The font what reads as code is drawn in: URLs, paths, addresses, times, sizes and a program's
//! output. Cascadia Mono, Microsoft's fixed-width face under the SIL Open Font License 1.1
//! (`assets/fonts/OFL.txt`), carried by the program and registered with Waft, so a column of
//! times or sizes lines up on every system.

use waft::{App, PaintingBinding};

/// The family code is drawn in, and the font's own name.
pub(crate) const CODE_FONT: &str = "Cascadia Mono";

/// Cascadia Mono, regular.
const CASCADIA_MONO: &[u8] = include_bytes!("../assets/fonts/CascadiaMono-Regular.ttf");

/// Registers the code font; after `run_app_with`, which makes the font collection it joins.
pub(crate) fn install_code_font(app: &mut App) {
    PaintingBinding::instance(app).register_font(app, CODE_FONT, CASCADIA_MONO.to_vec());
}
