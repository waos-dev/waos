//! A new program's `Cargo.toml`: its package, its dependencies on the SDK and on Waft, and its
//! `[package.metadata.waos]`, with every field `waos package` reads. Made in a workspace, it
//! takes what the workspace gives, as `cargo new` and `cargo add` write it: each key of
//! `[workspace.package]` and each dependency `[workspace.dependencies]` lists as
//! `<key>.workspace = true`, and `[lints] workspace = true` for `[workspace.lints]`; whatever the
//! workspace does not give is written as a published template names it, a dependency by its
//! crates.io version.
//!
//! The invariant: the versions are this release's. The SDK's is this crate's own, since the
//! workspace versions its crates together; Waft's is the one `wasdk-adapter-waft` is built on,
//! which `tests/new.rs` holds it to by building each template against the workspace.

use super::enclosing_workspace::Inherited;
use super::icon;
use super::program_name::ProgramName;
use super::template::Template;

/// The SDK's version, `wasdk`'s and `wasdk-adapter-waft`'s, as a requirement: this release's
/// major and minor.
const SDK_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

/// Waft's version, the facade's and the kits', as a requirement.
const WAFT_VERSION: &str = "0.9";

/// The `Cargo.toml` of a program named `name`, written with `template`'s kit, taking what
/// `inherited` names from its workspace.
pub fn cargo_toml(name: &ProgramName, template: Template, inherited: &Inherited) -> String {
    let id = name.id();
    let display_name = name.display_name();
    let icon = icon::FILE;
    let package = package_fields(inherited);
    let dependencies = dependencies(template, inherited);
    let lints = if inherited.lints {
        "\n[lints]\nworkspace = true\n"
    } else {
        ""
    };
    format!(
        r#"[package]
name = "{name}"
{package}
[dependencies]
{dependencies}{lints}
# What WaOS knows the program by. The id names it for good: every update is published under it.
# The icon is a square PNG, at least 256 pixels on a side. A program is an app, with a UI; one with
# none, a command-line program, says `kind = "tool"`.
[package.metadata.waos]
id = "{id}"
name = "{display_name}"
icon = "{icon}"
"#
    )
}

/// The package's fields after its name, a line each: its version and edition, then, as `cargo
/// new` writes them, each key the workspace's package gives in the place of its own.
fn package_fields(inherited: &Inherited) -> String {
    let mut fields = vec![
        ("version", r#"version = "0.1.0""#.to_owned()),
        ("edition", r#"edition = "2024""#.to_owned()),
    ];
    for key in &inherited.package_keys {
        fields.retain(|(own, _)| own != key);
        fields.push((key, format!("{key}.workspace = true")));
    }
    fields.into_iter().map(|(_, line)| line + "\n").collect()
}

/// The program's dependencies, a line each: the SDK, Waft and `template`'s kit, each from the
/// workspace when it lists it, else by its crates.io version.
fn dependencies(template: Template, inherited: &Inherited) -> String {
    let named = [
        ("wasdk", SDK_VERSION),
        ("wasdk-adapter-waft", SDK_VERSION),
        ("waft", WAFT_VERSION),
        (template.kit(), WAFT_VERSION),
    ];
    named
        .iter()
        .map(|(name, version)| {
            if inherited.dependencies.contains(*name) {
                format!("{name}.workspace = true\n")
            } else {
                format!("{name} = \"{version}\"\n")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(name: &str, template: Template) -> toml::Table {
        let name = name.parse().unwrap();
        cargo_toml(&name, template, &Inherited::default())
            .parse()
            .unwrap()
    }

    #[test]
    fn the_manifest_names_the_sdk_waft_and_the_kit_by_version() {
        let manifest = written("my-app", Template::Winui);
        let dependencies = manifest["dependencies"].as_table().unwrap();
        let mut named: Vec<(&str, &str)> = dependencies
            .iter()
            .map(|(name, version)| (name.as_str(), version.as_str().unwrap()))
            .collect();
        named.sort();
        assert_eq!(
            named,
            [
                ("waft", "0.9"),
                ("waft-winui", "0.9"),
                ("wasdk", SDK_VERSION),
                ("wasdk-adapter-waft", SDK_VERSION),
            ]
        );
        let cupertino = written("my-app", Template::Cupertino);
        assert!(cupertino["dependencies"].get("waft-cupertino").is_some());
        assert!(cupertino["dependencies"].get("waft-winui").is_none());
    }

    #[test]
    fn the_metadata_gives_the_id_the_name_and_the_icon() {
        let manifest = written("my-app", Template::Cupertino);
        let package = &manifest["package"];
        assert_eq!(package["name"].as_str(), Some("my-app"));
        let waos = &package["metadata"]["waos"];
        assert_eq!(waos["id"].as_str(), Some("dev.example.myapp"));
        assert_eq!(waos["name"].as_str(), Some("My app"));
        assert_eq!(waos["icon"].as_str(), Some("icon.png"));
    }

    #[test]
    fn the_program_is_no_workspace_of_its_own() {
        let manifest = written("hello", Template::Cupertino);
        assert!(manifest.get("workspace").is_none());
        assert!(manifest.get("lints").is_none());
    }

    /// As `cargo new` writes a member of a workspace with `[workspace.package]` and
    /// `[workspace.lints]`, and `cargo add` a dependency `[workspace.dependencies]` lists.
    #[test]
    fn a_member_takes_what_its_workspace_gives_and_names_the_rest_by_version() {
        let inherited = Inherited {
            package_keys: vec!["edition".into(), "license".into()],
            lints: true,
            dependencies: [
                "wasdk".to_owned(),
                "waft".to_owned(),
                "waft-winui".to_owned(),
            ]
            .into(),
        };
        let name = "my-app".parse().unwrap();
        let manifest = cargo_toml(&name, Template::Winui, &inherited);
        assert!(
            manifest.starts_with(
                "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\nedition.workspace = true\nlicense.workspace = true\n\n[dependencies]\nwasdk.workspace = true\nwasdk-adapter-waft = \""
            ),
            "{manifest}"
        );
        assert!(
            manifest.contains(
                "waft.workspace = true\nwaft-winui.workspace = true\n\n[lints]\nworkspace = true\n"
            ),
            "{manifest}"
        );
        let parsed: toml::Table = manifest.parse().unwrap();
        assert!(parsed.get("workspace").is_none());
        assert_eq!(
            parsed["package"]["metadata"]["waos"]["id"].as_str(),
            Some("dev.example.myapp")
        );
    }
}
