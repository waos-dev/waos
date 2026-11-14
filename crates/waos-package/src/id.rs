//! A program's id: reverse-DNS in the one form both an Apple bundle id and an Android
//! application id accept, since the id becomes part of both. Widening what an id may hold
//! breaks that, and breaks the installer's use of an id as a directory's name.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A program's id, like `dev.example.triangle`: two or more segments joined by dots, each of
/// lowercase ASCII letters and digits and starting with a letter.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProgramId(String);

impl ProgramId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProgramId {
    type Err = IdError;

    fn from_str(id: &str) -> Result<ProgramId, IdError> {
        check_characters(id)?;
        check_segments(id)?;
        Ok(ProgramId(id.to_owned()))
    }
}

impl TryFrom<String> for ProgramId {
    type Error = IdError;

    fn try_from(id: String) -> Result<ProgramId, IdError> {
        id.parse()
    }
}

impl From<ProgramId> for String {
    fn from(id: ProgramId) -> String {
        id.0
    }
}

impl fmt::Display for ProgramId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a string is not a program's id. Each carries the id as it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdError {
    /// A character other than a lowercase ASCII letter, a digit or a dot.
    InvalidCharacter { id: String, character: char },
    /// Fewer than two segments: `triangle` rather than `dev.example.triangle`.
    TooFewSegments { id: String },
    /// A segment with nothing in it: a dot at the start or the end, or two together.
    EmptySegment { id: String },
    /// A segment that starts with a digit rather than a letter.
    SegmentStartsWithDigit { id: String, segment: String },
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdError::InvalidCharacter { id, character } => write!(
                f,
                "the id {id:?} has {character:?}; an id holds lowercase ASCII letters, digits and dots"
            ),
            IdError::TooFewSegments { id } => write!(
                f,
                "the id {id:?} has fewer than two segments; an id is reverse-DNS, like \"dev.example.triangle\""
            ),
            IdError::EmptySegment { id } => write!(
                f,
                "the id {id:?} has an empty segment; an id neither starts nor ends with a dot, nor has two together"
            ),
            IdError::SegmentStartsWithDigit { id, segment } => write!(
                f,
                "the id {id:?} has the segment {segment:?}, which starts with a digit; each segment starts with a letter"
            ),
        }
    }
}

impl std::error::Error for IdError {}

fn check_characters(id: &str) -> Result<(), IdError> {
    let is_allowed = |character: char| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '.'
    };
    match id.chars().find(|character| !is_allowed(*character)) {
        Some(character) => Err(IdError::InvalidCharacter {
            id: id.to_owned(),
            character,
        }),
        None => Ok(()),
    }
}

/// Checks the segments of an id whose characters are already known to be allowed.
fn check_segments(id: &str) -> Result<(), IdError> {
    let segments: Vec<&str> = id.split('.').collect();
    if segments.len() < 2 {
        return Err(IdError::TooFewSegments { id: id.to_owned() });
    }
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(IdError::EmptySegment { id: id.to_owned() });
    }
    let starts_with_digit =
        |segment: &&str| segment.starts_with(|first: char| first.is_ascii_digit());
    match segments.into_iter().find(starts_with_digit) {
        Some(segment) => Err(IdError::SegmentStartsWithDigit {
            id: id.to_owned(),
            segment: segment.to_owned(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(id: &str) -> IdError {
        id.parse::<ProgramId>().unwrap_err()
    }

    #[test]
    fn two_single_letters_are_the_shortest_id() {
        assert_eq!("a.b".parse::<ProgramId>().unwrap().as_str(), "a.b");
    }

    #[test]
    fn digits_may_follow_the_first_letter_of_any_segment() {
        for id in [
            "a1.b2",
            "dev.example.triangle",
            "com.example9.app2go",
            "a.b.c.d.e.f",
        ] {
            assert_eq!(id.parse::<ProgramId>().unwrap().to_string(), id);
        }
    }

    #[test]
    fn one_segment_is_too_few() {
        for id in ["triangle", ""] {
            assert_eq!(refusal(id), IdError::TooFewSegments { id: id.to_owned() });
        }
    }

    #[test]
    fn a_dot_at_either_end_or_two_together_make_an_empty_segment() {
        for id in [".a.b", "a.b.", "a..b", "."] {
            assert_eq!(refusal(id), IdError::EmptySegment { id: id.to_owned() });
        }
    }

    #[test]
    fn a_segment_starting_with_a_digit_is_refused() {
        assert_eq!(
            refusal("dev.9lives"),
            IdError::SegmentStartsWithDigit {
                id: "dev.9lives".to_owned(),
                segment: "9lives".to_owned(),
            }
        );
        assert_eq!(
            refusal("1dev.app"),
            IdError::SegmentStartsWithDigit {
                id: "1dev.app".to_owned(),
                segment: "1dev".to_owned(),
            }
        );
    }

    #[test]
    fn anything_but_lowercase_letters_digits_and_dots_is_refused() {
        for (id, character) in [
            ("dev.Example", 'E'),
            ("dev.exam-ple", '-'),
            ("dev.exam_ple", '_'),
            ("dév.example", 'é'),
            ("dev.example ", ' '),
            ("dev/example.app", '/'),
        ] {
            assert_eq!(
                refusal(id),
                IdError::InvalidCharacter {
                    id: id.to_owned(),
                    character,
                }
            );
        }
    }

    #[test]
    fn a_refusal_names_the_id_and_what_is_wrong_with_it() {
        let message = refusal("dev.9lives").to_string();
        assert!(message.contains("\"dev.9lives\""), "{message}");
        assert!(message.contains("starts with a digit"), "{message}");
    }
}
