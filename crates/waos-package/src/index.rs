//! A package source's `index.json`: each program it offers, at a version, with its package and that
//! package's SHA-256, and what a store shows of it before it is downloaded: its icon, a line and a
//! description, its category, who publishes it, when this version was released and its screenshots;
//! with what the source calls itself, its own icon and the apps it puts forward.
//!
//! ```json
//! {
//!   "name": "Example Apps",
//!   "icon": "example-apps.png",
//!   "featured": ["dev.example.triangle"],
//!   "programs": [
//!     {
//!       "id": "dev.example.triangle",
//!       "name": "Triangle",
//!       "version": "1.2.0",
//!       "package": "dev.example.triangle-1.2.0.wapk",
//!       "sha256": "<the package file's SHA-256, 64 hex digits>",
//!       "icon": "dev.example.triangle-1.2.0.png",
//!       "summary": "One triangle, drawn well",
//!       "description": "A triangle of one colour over another.\n\nNothing else.",
//!       "category": "graphics-design",
//!       "publisher": "Example",
//!       "released": "2026-11-23",
//!       "screenshots": ["dev.example.triangle-1.2.0-1.png"]
//!     }
//!   ]
//! }
//! ```
//!
//! Only a program's first five fields are needed; a source that lists no more is read as before.
//! An icon and a screenshot are files' URLs, relative to the index's own as a package's is.
//!
//! An index is not signed: each package is, and a package's SHA-256 ties what is downloaded to
//! what the index lists. So what an index says of a program is the source's word, for a store to
//! show, and decides nothing: what is installed is the package, under its publisher's key. Fields
//! this version does not know are ignored, so a later version can add its own.
//!
//! The invariants a later editor will break: what [`Index::to_json`] writes,
//! [`Index::from_json`] reads; a listing is one id at one version, so listing a package again
//! replaces it; and nothing a store only shows makes an index unreadable: a category or a date
//! this version cannot read is read as none.

use std::fmt;

use jiff::civil::Date;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Category, ProgramId};

/// A source's list of the programs it offers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    /// What the source calls itself; a store shows its address where it says none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The source's own icon's URL, a PNG, relative to the index's own; a store shows the
    /// picture of where the source is where it says none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The programs the source puts forward, the first the most, by their ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub featured: Vec<ProgramId>,
    pub programs: Vec<ListedProgram>,
}

/// One program at one version, as an index lists it: what installs it, then what a store shows
/// of it before it is downloaded, each where the source says it.
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
    /// Its icon's URL, a PNG, relative to the index's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// What it is, in a line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What it is, at length: plain text, its paragraphs apart by an empty line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What it is for; none where the source says none, or one this version does not know.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "spelled",
        deserialize_with = "read_or_none"
    )]
    pub category: Option<Category>,
    /// Who publishes it, as the source names them: a label, where its key is what proves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// The day this version was released; none where the source says none, or no date.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "spelled",
        deserialize_with = "read_or_none"
    )]
    pub released: Option<Date>,
    /// Its screenshots' URLs, relative to the index's own, in the order to show them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<String>,
}

impl ListedProgram {
    /// The listing of `id` at `version`, named `name`, whose package is at `package` with the
    /// SHA-256 `sha256`, saying nothing more of it.
    pub fn new(
        id: ProgramId,
        name: impl Into<String>,
        version: Version,
        package: impl Into<String>,
        sha256: [u8; 32],
    ) -> ListedProgram {
        ListedProgram {
            id,
            name: name.into(),
            version,
            package: package.into(),
            sha256,
            icon: None,
            summary: None,
            description: None,
            category: None,
            publisher: None,
            released: None,
            screenshots: Vec::new(),
        }
    }
}

/// Writes `value` as it spells itself, where there is one.
fn spelled<T: fmt::Display, S: Serializer>(value: &Option<T>, to: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => to.collect_str(value),
        None => to.serialize_none(),
    }
}

/// Reads what a store only shows: the value a string spells, or none where it spells none this
/// version knows, so a later version's category never makes an index unreadable.
fn read_or_none<'de, T: std::str::FromStr, D: Deserializer<'de>>(
    from: D,
) -> Result<Option<T>, D::Error> {
    let spelled = Option::<String>::deserialize(from)?;
    Ok(spelled.and_then(|spelled| spelled.parse().ok()))
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
        ListedProgram::new(
            id.parse().unwrap(),
            "Triangle",
            version.parse().unwrap(),
            package,
            [7; 32],
        )
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
            ..Index::default()
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
    fn what_a_store_shows_of_a_program_reads_back_and_a_plain_listing_writes_none_of_it() {
        let shown = ListedProgram {
            icon: Some("triangle.png".to_owned()),
            summary: Some("One triangle, drawn well".to_owned()),
            description: Some("A triangle.\n\nNothing else.".to_owned()),
            category: Some(Category::GraphicsDesign),
            publisher: Some("Example".to_owned()),
            released: Some("2026-11-23".parse().unwrap()),
            screenshots: vec!["triangle-1.png".to_owned()],
            ..listed("dev.example.triangle", "1.2.0", "a.wapk")
        };
        let index = Index {
            name: Some("Example Apps".to_owned()),
            icon: Some("example-apps.png".to_owned()),
            featured: vec!["dev.example.triangle".parse().unwrap()],
            programs: vec![shown],
        };
        let json = index.to_json();
        assert!(json.contains(r#""category": "graphics-design""#), "{json}");
        assert!(json.contains(r#""released": "2026-11-23""#), "{json}");
        assert_eq!(Index::from_json(json.as_bytes()).unwrap(), index);

        let plain = Index {
            programs: vec![listed("dev.example.triangle", "1.2.0", "a.wapk")],
            ..Index::default()
        };
        let json = plain.to_json();
        for field in [
            "icon",
            "summary",
            "category",
            "released",
            "screenshots",
            "featured",
        ] {
            assert!(!json.contains(field), "{field} in {json}");
        }
    }

    #[test]
    fn a_category_or_a_date_this_version_cannot_read_is_none_and_the_index_still_reads() {
        let json = index_with_sha256(SHA256).replace(
            r#""name""#,
            r#""category": "astronomy", "released": "last Tuesday", "name""#,
        );
        let index = Index::from_json(json.as_bytes()).unwrap();
        assert_eq!(index.programs[0].category, None);
        assert_eq!(index.programs[0].released, None);
        assert_eq!(index.programs[0].id.as_str(), "dev.example.triangle");
    }

    #[test]
    fn a_sha256_is_written_in_lowercase_hex() {
        let index = Index {
            programs: vec![ListedProgram {
                sha256: hex::decode(SHA256).unwrap().try_into().unwrap(),
                ..listed("dev.example.triangle", "1.2.0", "a.wapk")
            }],
            ..Index::default()
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
            ..Index::default()
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
