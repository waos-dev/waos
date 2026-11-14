//! The font the terminal's text is drawn in: the first monospace family the platform has
//! installed, of those each system ships.

/// Monospace families, a system's own first: macOS's, Windows', Linux's, then Android's.
const MONOSPACE_FAMILIES: [&str; 10] = [
    "SF Mono",
    "Menlo",
    "Cascadia Mono",
    "Consolas",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Roboto Mono",
    "Droid Sans Mono",
    "Courier New",
];

/// What a platform with none of them is asked for: the generic name, which some resolve.
const GENERIC_FAMILY: &str = "monospace";

/// The family the terminal's text is drawn in, of those `installed`.
pub(crate) fn monospace_family(installed: &[String]) -> &'static str {
    MONOSPACE_FAMILIES
        .into_iter()
        .find(|family| installed.iter().any(|name| name == family))
        .unwrap_or(GENERIC_FAMILY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_installed_family_is_taken() {
        let installed = ["Consolas".to_owned(), "Menlo".to_owned()];
        assert_eq!(monospace_family(&installed), "Menlo");
    }

    #[test]
    fn a_platform_with_none_is_asked_for_the_generic_name() {
        assert_eq!(monospace_family(&["Arial".to_owned()]), "monospace");
    }
}
