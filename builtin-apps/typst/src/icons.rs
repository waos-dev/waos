//! The Fluent icons the app shows, by their code points in the icon font `waft-winui` bundles,
//! Fluent System Icons, the 20-point regular ones.

use waft_winui::FontIcon;

/// `ic_fluent_document_add_20_regular`: New.
pub(crate) const NEW: char = '\u{E4DB}';
/// `ic_fluent_folder_open_20_regular`: Open….
pub(crate) const OPEN: char = '\u{F42E}';
/// `ic_fluent_save_20_regular`: Save.
pub(crate) const SAVE: char = '\u{F67F}';
/// `ic_fluent_document_pdf_20_regular`: Export PDF….
pub(crate) const PDF: char = '\u{F3AC}';
/// `ic_fluent_add_20_regular`: Zoom In.
pub(crate) const ZOOM_IN: char = '\u{F109}';
/// `ic_fluent_subtract_20_regular`: Zoom Out.
pub(crate) const ZOOM_OUT: char = '\u{EBD0}';
/// `ic_fluent_error_circle_20_regular`: an error.
pub(crate) const ERROR: char = '\u{F3F1}';
/// `ic_fluent_warning_20_regular`: a warning.
pub(crate) const WARNING: char = '\u{F869}';

/// The size icons are shown at in the bars, in logical points.
const ICON_SIZE: f64 = 16.0;

pub(crate) fn icon(glyph: char) -> FontIcon {
    FontIcon::new(glyph).font_size(ICON_SIZE)
}
