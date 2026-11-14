//! The widget kits a new program starts from. Each gives the kit's crate and the program's
//! `src/main.rs`, a short Waft app in the kit's look, kept in `templates/` beside `src/` so it
//! reads as the program it becomes; the rest of a new program is the same for both.

use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Template {
    /// WinUI's controls, in Fluent's look, in a window sized for a desktop; the app works on a
    /// phone too.
    #[default]
    Winui,
    /// Cupertino's widgets, in the look of Apple's iOS, for an app meant for phones alone.
    Cupertino,
}

impl Template {
    /// The kit's crate, which the program depends on beside Waft.
    pub fn kit(self) -> &'static str {
        match self {
            Template::Winui => "waft-winui",
            Template::Cupertino => "waft-cupertino",
        }
    }

    /// The program's `src/main.rs`.
    pub fn main_rs(self) -> &'static str {
        match self {
            Template::Winui => include_str!("../../templates/winui.rs"),
            Template::Cupertino => include_str!("../../templates/cupertino.rs"),
        }
    }
}
