//! A program's `[package.metadata.waos]`: what packaging it needs that Cargo's own fields do not
//! say, the program's id, the name people see, its icon, and whether it is an app or a tool, an app
//! unless it says `kind = "tool"`; a run needs the id and the kind alone.
//!
//! ```toml
//! [package.metadata.waos]
//! id = "dev.example.triangle"
//! name = "Triangle"
//! icon = "icon.png"
//! ```
//!
//! The invariant: the id is checked here, before anything is built, since every version of a
//! program is published under it for good; and the icon's path is the package's own, relative to
//! its `Cargo.toml`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cargo_metadata::Package;
use serde::Deserialize;
use serde_json::Value;
use waos_package::{ProgramId, ProgramKind};

/// What `[package.metadata.waos]` says of a program.
#[derive(Debug)]
pub struct WaosMetadata {
    pub id: ProgramId,
    /// The name people see.
    pub name: String,
    /// The icon's PNG.
    pub icon: PathBuf,
    pub kind: ProgramKind,
}

/// What a run needs of `[package.metadata.waos]`: which program it is, and of what kind.
#[derive(Debug)]
pub struct RunIdentity {
    pub id: ProgramId,
    pub kind: ProgramKind,
}

/// The table as `Cargo.toml` spells it, before its fields are checked.
#[derive(Deserialize)]
struct Fields {
    id: String,
    name: String,
    icon: PathBuf,
    kind: Option<String>,
}

/// What a run needs of the table.
#[derive(Deserialize)]
struct RunFields {
    id: String,
    kind: Option<String>,
}

/// The kind `kind` spells, an app where none is said.
fn kind_of(kind: Option<&str>) -> Result<ProgramKind> {
    Ok(kind.map(str::parse).transpose()?.unwrap_or_default())
}

impl WaosMetadata {
    /// The `[package.metadata.waos]` of `package`.
    pub fn of(package: &Package) -> Result<WaosMetadata> {
        let folder = package
            .manifest_path
            .parent()
            .expect("a manifest is in a folder");
        WaosMetadata::from_table(table_of(package)?, folder.as_std_path())
            .with_context(|| format!("[package.metadata.waos] of {}", package.name))
    }

    /// The id in `package`'s `[package.metadata.waos]`, which names the program a command reaches.
    pub fn id_of(package: &Package) -> Result<ProgramId> {
        Ok(WaosMetadata::run_identity_of(package)?.id)
    }

    /// The id and kind in `package`'s `[package.metadata.waos]`, which a run is the program of.
    pub fn run_identity_of(package: &Package) -> Result<RunIdentity> {
        WaosMetadata::run_identity_from_table(table_of(package)?)
            .with_context(|| format!("[package.metadata.waos] of {}", package.name))
    }

    fn run_identity_from_table(table: &Value) -> Result<RunIdentity> {
        let fields: RunFields = serde_json::from_value(table.clone())?;
        Ok(RunIdentity {
            id: fields.id.parse()?,
            kind: kind_of(fields.kind.as_deref())?,
        })
    }

    /// The metadata `table` holds, its icon's path relative to `folder`.
    fn from_table(table: &Value, folder: &Path) -> Result<WaosMetadata> {
        let fields: Fields = serde_json::from_value(table.clone())?;
        Ok(WaosMetadata {
            id: fields.id.parse()?,
            name: fields.name,
            icon: folder.join(fields.icon),
            kind: kind_of(fields.kind.as_deref())?,
        })
    }
}

/// `package`'s `[package.metadata.waos]`, or what to add when it has none.
fn table_of(package: &Package) -> Result<&Value> {
    package
        .metadata
        .get("waos")
        .with_context(|| missing_table(&package.name))
}

/// What to say of a package with no `[package.metadata.waos]`: what to add.
fn missing_table(package: &str) -> String {
    format!(
        "{package}'s Cargo.toml has no [package.metadata.waos], which a package's id, name and icon come from, like:\n\n[package.metadata.waos]\nid = \"dev.example.{package}\"\nname = \"{package}\"\nicon = \"icon.png\"\n\nThe id is reverse-DNS and names the program for good; the icon is a square PNG at least 256 pixels on a side"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_table_gives_the_id_the_name_and_the_icon_beside_the_manifest() {
        let table =
            json!({"id": "dev.example.triangle", "name": "Triangle", "icon": "assets/icon.png"});
        let metadata = WaosMetadata::from_table(&table, Path::new("/programs/triangle")).unwrap();
        assert_eq!(metadata.id.as_str(), "dev.example.triangle");
        assert_eq!(metadata.name, "Triangle");
        assert_eq!(
            metadata.icon,
            Path::new("/programs/triangle/assets/icon.png")
        );
    }

    #[test]
    fn a_missing_field_is_named() {
        let table = json!({"id": "dev.example.triangle", "name": "Triangle"});
        let error = WaosMetadata::from_table(&table, Path::new("/")).unwrap_err();
        assert!(error.to_string().contains("`icon`"), "{error}");
    }

    #[test]
    fn a_run_reads_the_id_and_the_kind_alone() {
        let table = json!({"id": "dev.example.triangle"});
        let run = WaosMetadata::run_identity_from_table(&table).unwrap();
        assert_eq!(run.id.as_str(), "dev.example.triangle");
        assert_eq!(
            run.kind,
            ProgramKind::App,
            "an app unless it says otherwise"
        );
        let error =
            WaosMetadata::run_identity_from_table(&json!({"name": "Triangle"})).unwrap_err();
        assert!(error.to_string().contains("`id`"), "{error}");
    }

    #[test]
    fn a_tool_says_so_and_an_unknown_kind_is_refused() {
        let tool = json!({"id": "dev.example.grep", "kind": "tool"});
        let run = WaosMetadata::run_identity_from_table(&tool).unwrap();
        assert_eq!(run.kind, ProgramKind::Tool);
        let gadget = json!({"id": "dev.example.grep", "kind": "gadget"});
        let error = WaosMetadata::run_identity_from_table(&gadget).unwrap_err();
        assert!(error.to_string().contains("gadget"), "{error}");
    }

    #[test]
    fn an_id_that_is_not_reverse_dns_is_refused() {
        let table = json!({"id": "triangle", "name": "Triangle", "icon": "icon.png"});
        let error = WaosMetadata::from_table(&table, Path::new("/")).unwrap_err();
        assert!(error.to_string().contains("reverse-DNS"), "{error}");
    }
}
