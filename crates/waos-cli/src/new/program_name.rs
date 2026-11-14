//! The name a new program is made with: its package's name, checked as cargo checks a binary's,
//! and what follows from it, the id it is published under and the name people see.
//!
//! The invariant: every name the check lets through gives an id `waos-package` accepts. The id
//! is the name without its `-` and `_`, so the check must keep a name to lowercase ASCII letters
//! and digits besides those, starting with a letter.

use std::fmt;
use std::str::FromStr;

use anyhow::{Error, bail};
use waos_package::ProgramId;

/// Rust's keywords, strict and reserved, in lowercase: none names a crate.
const KEYWORDS: [&str; 51] = [
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// What cargo refuses to name a package with a binary: Rust's test library, and the folders of
/// its own build directory.
const CARGO_RESERVED: [&str; 5] = ["test", "deps", "examples", "build", "incremental"];

/// A new program's package name, like `my-app`: lowercase ASCII letters, digits, `-` and `_`,
/// starting with a letter, and neither a keyword nor a name cargo keeps for itself.
#[derive(Debug)]
pub struct ProgramName(String);

impl FromStr for ProgramName {
    type Err = Error;

    fn from_str(name: &str) -> Result<ProgramName, Error> {
        if !is_package_name(name) {
            bail!(
                "`{name}` is not a package name: lowercase letters, digits, `-` and `_`, starting with a letter"
            );
        }
        if KEYWORDS.contains(&name) {
            bail!("`{name}` is a Rust keyword, which cannot name a package");
        }
        if CARGO_RESERVED.contains(&name) {
            bail!("`{name}` is a name cargo keeps for itself, which cannot name a package");
        }
        Ok(ProgramName(name.to_owned()))
    }
}

impl ProgramName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The id the program is published under: `dev.example.` and the name without its `-` and
    /// `_`, which an id cannot hold.
    pub fn id(&self) -> ProgramId {
        let segment: String = self.0.chars().filter(char::is_ascii_alphanumeric).collect();
        format!("dev.example.{segment}")
            .parse()
            .expect("a checked name gives an id")
    }

    /// The name people see: `My app` for `my-app`.
    pub fn display_name(&self) -> String {
        let spaced = self.0.replace(['-', '_'], " ");
        // A checked name starts with an ASCII letter.
        let (first, rest) = spaced.split_at(1);
        format!("{}{rest}", first.to_ascii_uppercase())
    }
}

impl fmt::Display for ProgramName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `name` is made of lowercase ASCII letters, digits, `-` and `_`, starting with a
/// letter.
fn is_package_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(name: &str) -> String {
        name.parse::<ProgramName>().unwrap_err().to_string()
    }

    #[test]
    fn a_package_name_is_lowercase_letters_digits_dashes_and_underscores_from_a_letter() {
        for name in ["hello", "my-app", "my_app", "app2", "a"] {
            assert_eq!(name.parse::<ProgramName>().unwrap().as_str(), name);
        }
        for name in [
            "", "My-app", "my app", "2app", "-app", "_app", "café", "my.app",
        ] {
            assert!(refusal(name).contains("not a package name"), "{name}");
        }
    }

    #[test]
    fn a_keyword_or_a_name_cargo_keeps_is_refused() {
        assert!(refusal("fn").contains("keyword"));
        assert!(refusal("gen").contains("keyword"));
        assert!(refusal("test").contains("cargo keeps"));
        assert!(refusal("build").contains("cargo keeps"));
    }

    #[test]
    fn the_id_is_the_name_without_its_dashes_and_underscores() {
        let id = |name: &str| name.parse::<ProgramName>().unwrap().id().to_string();
        assert_eq!(id("hello"), "dev.example.hello");
        assert_eq!(id("my-app"), "dev.example.myapp");
        assert_eq!(id("my_app2"), "dev.example.myapp2");
    }

    #[test]
    fn the_name_people_see_is_spaced_and_capitalized() {
        let shown = |name: &str| name.parse::<ProgramName>().unwrap().display_name();
        assert_eq!(shown("hello"), "Hello");
        assert_eq!(shown("my-app"), "My app");
        assert_eq!(shown("photo_booth"), "Photo booth");
    }
}
