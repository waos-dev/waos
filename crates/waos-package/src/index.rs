//! A package source's `index.json`: each program it offers, at a version, with its package and
//! that package's SHA-256 (the design's decision 45).
//!
//! ```json
//! {
//!   "programs": [
//!     {
//!       "id": "dev.example.triangle",
//!       "name": "Triangle",
//!       "version": "1.2.0",
//!       "package": "dev.example.triangle-1.2.0.wapk",
//!       "sha256": "<the package file's SHA-256, 64 hex digits>"
//!     }
//!   ]
//! }
//! ```
//!
//! An index is not signed: each package is, and a package's SHA-256 ties what is downloaded to
//! what the index lists. Fields this version does not know are ignored, so a later version can
//! add its own.
//!
//! The invariants a later editor will break: what [`Index::to_json`] writes,
//! [`Index::from_json`] reads; and a listing is one id at one version, so listing a package again
//! replaces it.

use std::fmt;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::ProgramId;

/// A source's list of the programs it offers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub programs: Vec<ListedProgram>,
}

/// One program at one version, as an index lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedProgram {
    pub id: ProgramId,
    /// The name the user sees.
    pub name: String,
    pub version: Version,
    /// The package's URL, relative to the index's own, so a file name is a package beside the
    /// index.
    pub package: String,
    /// The SHA-256 of the package file, which a download is checked against before it is
    /// opened.
    #[serde(with = "hex")]
    pub sha256: [u8; 32],
}

impl Index {
    pub fn from_json(json: &[u8]) -> Result<Index, IndexError> {
        serde_json::from_slice(json).map_err(IndexError)
    }

    /// The index as JSON, indented for people to read, ending with a line feed.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("an index is plain data");
        json.push('\n');
        json
    }

    /// Lists `program` in place of the listing of the same id at the same version, or after the
    /// others when there is none.
    pub fn list(&mut self, program: ListedProgram) {
        let same = |listed: &&mut ListedProgram| {
            listed.id == program.id && listed.version == program.version
        };
        match self.programs.iter_mut().find(same) {
            Some(listed) => *listed = program,
            None => self.programs.push(program),
        }
    }
}

/// Why bytes are not an index, in the JSON parser's words, which name the line and column.
#[derive(Debug)]
pub struct IndexError(serde_json::Error);

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "not an index of programs, each with an id, a name, a version, a package and a sha256: {}",
            self.0
        )
    }
}

impl std::error::Error for IndexError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA256: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn listed(id: &str, version: &str, package: &str) -> ListedProgram {
        ListedProgram {
            id: id.parse().unwrap(),
            name: "Triangle".to_owned(),
            version: version.parse().unwrap(),
            package: package.to_owned(),
            sha256: [7; 32],
        }
    }

    fn index_with_sha256(sha256: &str) -> String {
        format!(
            r#"{{"programs": [{{"id": "dev.example.triangle", "name": "Triangle", "version": "1.2.0", "package": "triangle-1.2.0.wapk", "sha256": "{sha256}"}}]}}"#
        )
    }

    #[test]
    fn an_index_reads_back_what_it_writes() {
        let index = Index {
            programs: vec![
                listed("dev.example.triangle", "1.2.0", "a.wapk"),
                listed("dev.example.square", "0.1.0", "b.wapk"),
            ],
        };
        assert_eq!(Index::from_json(index.to_json().as_bytes()).unwrap(), index);
    }

    #[test]
    fn an_index_as_a_source_writes_it_is_read() {
        let index = Index::from_json(index_with_sha256(SHA256).as_bytes()).unwrap();
        let program = &index.programs[0];
        assert_eq!(program.id.as_str(), "dev.example.triangle");
        assert_eq!(program.version, Version::new(1, 2, 0));
        assert_eq!(program.package, "triangle-1.2.0.wapk");
        assert_eq!(hex::encode(program.sha256), SHA256);
    }

    #[test]
    fn a_sha256_is_written_in_lowercase_hex() {
        let index = Index {
            programs: vec![ListedProgram {
                sha256: hex::decode(SHA256).unwrap().try_into().unwrap(),
                ..listed("dev.example.triangle", "1.2.0", "a.wapk")
            }],
        };
        assert!(index.to_json().contains(SHA256), "{}", index.to_json());
    }

    #[test]
    fn a_sha256_that_is_not_32_bytes_of_hex_is_refused() {
        for sha256 in [
            "",
            "9f86",
            &SHA256.replace('9', "g"),
            &format!("{SHA256}00"),
        ] {
            assert!(Index::from_json(index_with_sha256(sha256).as_bytes()).is_err());
        }
    }

    #[test]
    fn an_invalid_id_is_refused_by_name() {
        let json = index_with_sha256(SHA256).replace("dev.example.triangle", "triangle");
        let error = Index::from_json(json.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("\"triangle\""), "{error}");
    }

    #[test]
    fn fields_a_later_version_adds_are_ignored() {
        let json = index_with_sha256(SHA256).replace(r#""name""#, r#""size": 7, "name""#);
        assert_eq!(
            Index::from_json(json.as_bytes()).unwrap(),
            Index::from_json(index_with_sha256(SHA256).as_bytes()).unwrap()
        );
    }

    #[test]
    fn listing_a_version_again_replaces_it_and_keeps_the_others() {
        let mut index = Index {
            programs: vec![
                listed("dev.example.triangle", "1.1.0", "old.wapk"),
                listed("dev.example.triangle", "1.2.0", "first.wapk"),
                listed("dev.example.square", "1.2.0", "square.wapk"),
            ],
        };
        index.list(listed("dev.example.triangle", "1.2.0", "again.wapk"));
        index.list(listed("dev.example.circle", "0.1.0", "circle.wapk"));
        let packages: Vec<&str> = index
            .programs
            .iter()
            .map(|program| program.package.as_str())
            .collect();
        assert_eq!(
            packages,
            ["old.wapk", "again.wapk", "square.wapk", "circle.wapk"]
        );
    }
}
