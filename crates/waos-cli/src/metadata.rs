//! A program's `[package.metadata.waos]`: what packaging it needs that Cargo's own fields do not
//! say, the program's id, the name people see, its icon, whether it is an app or a tool, an app
//! unless it says `kind = "tool"`, what it is for, where it says a `category`, and what a store
//! shows of it, where it says them: a line, a description's text file, who publishes it and its
//! screenshots; and the kinds of file it opens, each a `[[package.metadata.waos.file_associations]]`
//! table, as Waft's own are declared. An install from its source needs the id and the kind alone,
//! and takes the name, the icon, the category and the file associations where the table gives them.
//!
//! ```toml
//! [package.metadata.waos]
//! id = "dev.example.triangle"
//! name = "Triangle"
//! icon = "icon.png"
//! category = "graphics-design"
//! summary = "One triangle, drawn well"
//! description = "store/description.txt"
//! publisher = "Example"
//! screenshots = ["store/triangle-1.png"]
//!
//! [[package.metadata.waos.file_associations]]
//! extensions = ["tri"]
//! name = "Triangle drawing"
//! mime_type = "application/x-triangle"
//! ```
//!
//! The invariant: the id and the file associations are checked here, before anything is built,
//! since every version of a program is published under its id for good, and an association no
//! system could match a file to is a mistake to say at once; and the icon's path is the package's
//! own, relative to its `Cargo.toml`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cargo_metadata::Package;
use serde::Deserialize;
use serde_json::Value;
use waos_package::{Category, FileAssociation, ProgramId, ProgramKind};

/// What `[package.metadata.waos]` says of a program.
#[derive(Debug)]
pub struct WaosMetadata {
    pub id: ProgramId,
    /// The name people see.
    pub name: String,
    /// The icon's PNG.
    pub icon: PathBuf,
    pub kind: ProgramKind,
    /// What the app is for, where the table says.
    pub category: Option<Category>,
    /// What it is, in a line, where the table says; Cargo's `description` otherwise.
    pub summary: Option<String>,
    /// The text file that says what it is at length, where the table names one.
    pub description: Option<PathBuf>,
    /// Who publishes it, as a store names them, where the table says.
    pub publisher: Option<String>,
    /// Its screenshots, PNGs, in the order a store shows them.
    pub screenshots: Vec<PathBuf>,
    /// The kinds of file it opens; none where the table declares none.
    pub file_associations: Vec<FileAssociation>,
}

/// What an install from the program's source needs of `[package.metadata.waos]`: which program it
/// is, of what kind, and the name people see and the icon where it gives them.
#[derive(Debug)]
pub struct InstallIdentity {
    pub id: ProgramId,
    pub kind: ProgramKind,
    pub name: Option<String>,
    /// The icon's PNG, beside the package's `Cargo.toml`.
    pub icon: Option<PathBuf>,
    /// What the app is for, where the table says.
    pub category: Option<Category>,
    /// The kinds of file it opens; none where the table declares none.
    pub file_associations: Vec<FileAssociation>,
}

/// The table as `Cargo.toml` spells it, before its fields are checked.
#[derive(Deserialize)]
struct Fields {
    id: String,
    name: String,
    icon: PathBuf,
    kind: Option<String>,
    category: Option<String>,
    summary: Option<String>,
    description: Option<PathBuf>,
    publisher: Option<String>,
    #[serde(default)]
    screenshots: Vec<PathBuf>,
    #[serde(default)]
    file_associations: Vec<FileAssociation>,
}

/// What an install needs of the table.
#[derive(Deserialize)]
struct InstallFields {
    id: String,
    kind: Option<String>,
    name: Option<String>,
    icon: Option<PathBuf>,
    category: Option<String>,
    #[serde(default)]
    file_associations: Vec<FileAssociation>,
}

/// The kind `kind` spells, an app where none is said.
fn kind_of(kind: Option<&str>) -> Result<ProgramKind> {
    Ok(kind.map(str::parse).transpose()?.unwrap_or_default())
}

/// `associations`, each checked: one no system could match a file to is refused.
fn checked(associations: Vec<FileAssociation>) -> Result<Vec<FileAssociation>> {
    for association in &associations {
        association.check()?;
    }
    Ok(associations)
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
        Ok(WaosMetadata::install_identity_of(package)?.id)
    }

    /// What `package`'s `[package.metadata.waos]` says of the program an install from its source
    /// is: its id and kind, and its name and icon where it says them.
    pub fn install_identity_of(package: &Package) -> Result<InstallIdentity> {
        let folder = package
            .manifest_path
            .parent()
            .expect("a manifest is in a folder");
        WaosMetadata::install_identity_from_table(table_of(package)?, folder.as_std_path())
            .with_context(|| format!("[package.metadata.waos] of {}", package.name))
    }

    /// The install's identity `table` holds, its icon's path relative to `folder`.
    fn install_identity_from_table(table: &Value, folder: &Path) -> Result<InstallIdentity> {
        let fields: InstallFields = serde_json::from_value(table.clone())?;
        Ok(InstallIdentity {
            id: fields.id.parse()?,
            kind: kind_of(fields.kind.as_deref())?,
            name: fields.name,
            icon: fields.icon.map(|icon| folder.join(icon)),
            category: fields.category.as_deref().map(str::parse).transpose()?,
            file_associations: checked(fields.file_associations)?,
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
            // A category mistyped is refused here, with the categories there are, where a
            // package read later would take it for one a later version knows.
            category: fields.category.as_deref().map(str::parse).transpose()?,
            summary: fields.summary,
            description: fields.description.map(|text| folder.join(text)),
            publisher: fields.publisher,
            screenshots: fields
                .screenshots
                .into_iter()
                .map(|screenshot| folder.join(screenshot))
                .collect(),
            file_associations: checked(fields.file_associations)?,
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
    fn the_table_says_what_a_store_shows_its_files_beside_the_manifest() {
        let table = json!({"id": "dev.example.triangle", "name": "Triangle", "icon": "icon.png",
            "summary": "One triangle", "description": "store/description.txt",
            "publisher": "Example", "screenshots": ["store/one.png", "store/two.png"]});
        let metadata = WaosMetadata::from_table(&table, Path::new("/programs/triangle")).unwrap();
        assert_eq!(metadata.summary.as_deref(), Some("One triangle"));
        assert_eq!(
            metadata.description.as_deref(),
            Some(Path::new("/programs/triangle/store/description.txt"))
        );
        assert_eq!(metadata.publisher.as_deref(), Some("Example"));
        assert_eq!(
            metadata.screenshots,
            [
                Path::new("/programs/triangle/store/one.png"),
                Path::new("/programs/triangle/store/two.png")
            ]
        );
    }

    #[test]
    fn the_table_says_what_the_app_is_for_and_a_category_mistyped_is_refused() {
        let table = json!({"id": "dev.example.triangle", "name": "Triangle", "icon": "icon.png",
            "category": "graphics-design"});
        let metadata = WaosMetadata::from_table(&table, Path::new("/programs/triangle")).unwrap();
        assert_eq!(metadata.category, Some(Category::GraphicsDesign));

        let mistyped = json!({"id": "dev.example.triangle", "name": "Triangle",
            "icon": "icon.png", "category": "graphics"});
        let refusal = WaosMetadata::from_table(&mistyped, Path::new("/programs/triangle"))
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("`graphics` is no category"), "{refusal}");
    }

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
    fn an_install_needs_the_id_alone_and_takes_the_name_and_the_icon_given() {
        let folder = Path::new("/programs/triangle");
        let table = json!({"id": "dev.example.triangle"});
        let install = WaosMetadata::install_identity_from_table(&table, folder).unwrap();
        assert_eq!(install.id.as_str(), "dev.example.triangle");
        assert_eq!(
            install.kind,
            ProgramKind::App,
            "an app unless it says otherwise"
        );
        assert_eq!((install.name, install.icon), (None, None));
        let whole = json!({"id": "dev.example.triangle", "name": "Triangle", "icon": "icon.png"});
        let install = WaosMetadata::install_identity_from_table(&whole, folder).unwrap();
        assert_eq!(install.name.as_deref(), Some("Triangle"));
        assert_eq!(install.icon, Some(folder.join("icon.png")));
        let error = WaosMetadata::install_identity_from_table(&json!({"name": "Triangle"}), folder)
            .unwrap_err();
        assert!(error.to_string().contains("`id`"), "{error}");
    }

    #[test]
    fn a_tool_says_so_and_an_unknown_kind_is_refused() {
        let folder = Path::new("/");
        let tool = json!({"id": "dev.example.grep", "kind": "tool"});
        let install = WaosMetadata::install_identity_from_table(&tool, folder).unwrap();
        assert_eq!(install.kind, ProgramKind::Tool);
        let gadget = json!({"id": "dev.example.grep", "kind": "gadget"});
        let error = WaosMetadata::install_identity_from_table(&gadget, folder).unwrap_err();
        assert!(error.to_string().contains("gadget"), "{error}");
    }

    #[test]
    fn an_id_that_is_not_reverse_dns_is_refused() {
        let table = json!({"id": "triangle", "name": "Triangle", "icon": "icon.png"});
        let error = WaosMetadata::from_table(&table, Path::new("/")).unwrap_err();
        assert!(error.to_string().contains("reverse-DNS"), "{error}");
    }

    #[test]
    fn the_kinds_of_file_a_program_opens_are_read_for_a_package_and_an_install() {
        let table = json!({"id": "dev.example.ruffle", "name": "Ruffle", "icon": "icon.png",
            "file_associations": [{"extensions": ["swf"], "name": "Flash movie",
                "mime_type": "application/x-shockwave-flash"}]});
        let flash = FileAssociation {
            extensions: vec!["swf".to_owned()],
            name: "Flash movie".to_owned(),
            mime_type: Some("application/x-shockwave-flash".to_owned()),
        };
        let metadata = WaosMetadata::from_table(&table, Path::new("/")).unwrap();
        assert_eq!(metadata.file_associations, std::slice::from_ref(&flash));
        let install = WaosMetadata::install_identity_from_table(&table, Path::new("/")).unwrap();
        assert_eq!(install.file_associations, [flash]);
        let none = json!({"id": "dev.example.triangle", "name": "Triangle", "icon": "icon.png"});
        let metadata = WaosMetadata::from_table(&none, Path::new("/")).unwrap();
        assert!(metadata.file_associations.is_empty());
    }

    #[test]
    fn an_association_no_system_could_match_a_file_to_is_refused_before_a_build() {
        let dotted = json!({"id": "dev.example.ruffle", "name": "Ruffle", "icon": "icon.png",
            "file_associations": [{"extensions": [".swf"], "name": "Flash movie"}]});
        let error = WaosMetadata::from_table(&dotted, Path::new("/")).unwrap_err();
        assert!(error.to_string().contains("without the dot"), "{error}");
        let empty = json!({"id": "dev.example.ruffle",
            "file_associations": [{"extensions": [], "name": "Flash movie"}]});
        let error = WaosMetadata::install_identity_from_table(&empty, Path::new("/")).unwrap_err();
        assert!(error.to_string().contains("no extensions"), "{error}");
    }
}
