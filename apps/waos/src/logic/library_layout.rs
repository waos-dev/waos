//! Which layout the library takes in a view: Cupertino's in a view narrower than a tablet's, as a
//! phone's screen is held upright or a desktop window is made narrow, and Fluent's in a wider one,
//! a desktop's or a tablet's. The line is Material 3's between its compact window class and the
//! rest: 600 logical points wide. A phone turned on its side is as wide as a small tablet, and
//! takes Fluent's.

/// The width from which a view takes Fluent's layout.
const FLUENT_FROM_WIDTH: f64 = 600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryLayout {
    /// Fluent's controls, at a desktop's or a tablet's width.
    Fluent,
    /// Cupertino's, at a phone's width.
    Cupertino,
}

impl LibraryLayout {
    /// The layout of a view `width` logical points wide.
    pub(crate) fn for_width(width: f64) -> LibraryLayout {
        if width < FLUENT_FROM_WIDTH {
            LibraryLayout::Cupertino
        } else {
            LibraryLayout::Fluent
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phones_width_takes_cupertinos_layout() {
        assert_eq!(LibraryLayout::for_width(390.0), LibraryLayout::Cupertino);
        assert_eq!(LibraryLayout::for_width(599.5), LibraryLayout::Cupertino);
    }

    #[test]
    fn a_tablets_or_a_desktops_width_takes_fluents() {
        assert_eq!(LibraryLayout::for_width(600.0), LibraryLayout::Fluent);
        assert_eq!(LibraryLayout::for_width(720.0), LibraryLayout::Fluent);
        assert_eq!(LibraryLayout::for_width(1024.0), LibraryLayout::Fluent);
    }
}
