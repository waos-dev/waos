//! A program's manifest: its id, the name the user sees, its version, and whether it is an app or
//! a tool, as TOML. An app writes no `kind`, so a manifest from before kinds reads as an app.

use std::fmt;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{IdError, ProgramId};

/// A program's identity: what an installer files it under, shows the user, and compares to
/// tell an update from the version installed; and what kind of program it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    id: ProgramId,
    name: String,
    version: Version,
    kind: ProgramKind,
}

/// What a program is to its user, as its author declares it (decision 3 of
/// `docs/proposals/2026-11-21-views-a-program-asks-for.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgramKind {
    /// A program with a UI, which WaOS gives a screen as it launches on a phone.
    #[default]
    App,
    /// A program with no UI, a command-line program, whose input and output are a terminal's.
    Tool,
}

impl ProgramKind {
    /// The kind as a manifest spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            ProgramKind::App => "app",
            ProgramKind::Tool => "tool",
        }
    }
}

impl std::str::FromStr for ProgramKind {
    type Err = ManifestError;

    fn from_str(kind: &str) -> Result<ProgramKind, ManifestError> {
        match kind {
            "app" => Ok(ProgramKind::App),
            "tool" => Ok(ProgramKind::Tool),
            other => Err(ManifestError::UnknownKind(other.to_owned())),
        }
    }
}

impl Manifest {
    /// The manifest of the program `id`, shown to the user as `name`, at `version`. A name
    /// of nothing but whitespace is refused as empty.
    pub fn new(
        id: ProgramId,
        name: impl Into<String>,
        version: Version,
    ) -> Result<Manifest, ManifestError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }
        Ok(Manifest {
            id,
            name,
            version,
            kind: ProgramKind::App,
        })
    }

    /// The same manifest, of `kind`.
    pub fn with_kind(self, kind: ProgramKind) -> Manifest {
        Manifest { kind, ..self }
    }

    /// Reads a manifest from its TOML. Fields this version does not know are ignored, so a
    /// later version can add its own.
    pub fn from_toml(text: &str) -> Result<Manifest, ManifestError> {
        let fields: ManifestFields =
            toml::from_str(text).map_err(|error| ManifestError::NotAManifest(error.to_string()))?;
        let id = fields.id.parse().map_err(ManifestError::InvalidId)?;
        let version = parse_version(&fields.version)?;
        let kind = match fields.kind {
            Some(kind) => kind.parse()?,
            None => ProgramKind::App,
        };
        Ok(Manifest::new(id, fields.name, version)?.with_kind(kind))
    }

    /// The manifest as TOML, as a package holds it.
    pub fn to_toml(&self) -> String {
        let fields = ManifestFields {
            id: self.id.to_string(),
            name: self.name.clone(),
            version: self.version.to_string(),
            kind: (self.kind != ProgramKind::App).then(|| self.kind.as_str().to_owned()),
        };
        toml::to_string(&fields).expect("a table of strings is TOML")
    }

    pub fn id(&self) -> &ProgramId {
        &self.id
    }

    /// The name the user sees.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn kind(&self) -> ProgramKind {
        self.kind
    }
}

/// A manifest as TOML spells it, before its fields are checked.
#[derive(Serialize, Deserialize)]
struct ManifestFields {
    id: String,
    name: String,
    version: String,
    /// None for an app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
}

fn parse_version(version: &str) -> Result<Version, ManifestError> {
    Version::parse(version).map_err(|error| ManifestError::InvalidVersion {
        version: version.to_owned(),
        reason: error.to_string(),
    })
}

/// Why a manifest is not a valid one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// Not TOML, or a field missing or not a string, in the TOML parser's words.
    NotAManifest(String),
    InvalidId(IdError),
    /// The name is empty, or nothing but whitespace.
    EmptyName,
    /// The version is not a semver version, in the semver parser's words.
    InvalidVersion {
        version: String,
        reason: String,
    },
    /// The kind is neither `app` nor `tool`.
    UnknownKind(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::NotAManifest(reason) => {
                write!(
                    f,
                    "not a manifest with an id, a name and a version: {reason}"
                )
            }
            ManifestError::InvalidId(error) => write!(f, "{error}"),
            ManifestError::EmptyName => {
                f.write_str("the name is empty; a program's name is what the user sees")
            }
            ManifestError::InvalidVersion { version, reason } => {
                write!(
                    f,
                    "the version {version:?} is not a semver version: {reason}"
                )
            }
            ManifestError::UnknownKind(kind) => {
                write!(f, "the kind {kind:?} is neither \"app\" nor \"tool\"")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIANGLE: &str =
        "id = \"dev.example.triangle\"\nname = \"Triangle\"\nversion = \"0.1.0\"\n";

    #[test]
    fn a_manifest_reads_back_what_it_writes() {
        let manifest = Manifest::from_toml(TRIANGLE).unwrap();
        assert_eq!(manifest.id().as_str(), "dev.example.triangle");
        assert_eq!(manifest.name(), "Triangle");
        assert_eq!(manifest.version(), &Version::new(0, 1, 0));
        assert_eq!(Manifest::from_toml(&manifest.to_toml()).unwrap(), manifest);
    }

    #[test]
    fn a_manifest_with_no_kind_is_an_app_and_an_app_writes_none() {
        let manifest = Manifest::from_toml(TRIANGLE).unwrap();
        assert_eq!(manifest.kind(), ProgramKind::App);
        assert_eq!(manifest.to_toml(), TRIANGLE);
    }

    #[test]
    fn a_tool_reads_back_as_one() {
        let tool = Manifest::from_toml(TRIANGLE)
            .unwrap()
            .with_kind(ProgramKind::Tool);
        assert!(tool.to_toml().contains("kind = \"tool\""));
        assert_eq!(Manifest::from_toml(&tool.to_toml()).unwrap(), tool);
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        let gadget = format!("{TRIANGLE}kind = \"gadget\"\n");
        assert_eq!(
            Manifest::from_toml(&gadget),
            Err(ManifestError::UnknownKind("gadget".to_owned()))
        );
    }

    #[test]
    fn fields_a_later_version_adds_are_ignored() {
        let later = format!("{TRIANGLE}minimum-host = \"2.0.0\"\n");
        assert_eq!(
            Manifest::from_toml(&later).unwrap(),
            Manifest::from_toml(TRIANGLE).unwrap()
        );
    }

    #[test]
    fn a_missing_field_is_named() {
        let without_name = "id = \"dev.example.triangle\"\nversion = \"0.1.0\"\n";
        let error = Manifest::from_toml(without_name).unwrap_err();
        assert!(matches!(error, ManifestError::NotAManifest(_)));
        assert!(
            error.to_string().contains("missing field `name`"),
            "{error}"
        );
    }

    #[test]
    fn text_that_is_not_toml_is_refused() {
        let error = Manifest::from_toml("id: dev.example.triangle").unwrap_err();
        assert!(matches!(error, ManifestError::NotAManifest(_)));
    }

    #[test]
    fn an_invalid_id_says_what_is_wrong_with_it() {
        let uppercase = TRIANGLE.replace("dev.example.triangle", "dev.example.Triangle");
        assert_eq!(
            Manifest::from_toml(&uppercase).unwrap_err(),
            ManifestError::InvalidId(IdError::InvalidCharacter {
                id: "dev.example.Triangle".to_owned(),
                character: 'T',
            })
        );
    }

    #[test]
    fn an_empty_or_blank_name_is_refused() {
        for name in ["", "  \t"] {
            let blank = TRIANGLE.replace("Triangle", name);
            assert_eq!(
                Manifest::from_toml(&blank).unwrap_err(),
                ManifestError::EmptyName
            );
        }
    }

    #[test]
    fn a_version_that_is_not_semver_is_refused() {
        let two_parts = TRIANGLE.replace("0.1.0", "0.1");
        let error = Manifest::from_toml(&two_parts).unwrap_err();
        assert!(
            matches!(&error, ManifestError::InvalidVersion { version, .. } if version == "0.1"),
            "{error}"
        );
    }
}
