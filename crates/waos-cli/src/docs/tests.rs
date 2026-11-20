use std::fs;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;

use super::*;

/// A small archive's files, of the site's shape: the manifest with a section and its headings, the
/// playbook, pages, an interface's page, and the icons page as the bundle script writes it.
const FILES: &[(&str, &str)] = &[
    (
        "manifest.json",
        r#"{"version": "9.8.7", "pages": [
            {"kind": "page", "name": "start", "title": "Start", "description": "From nothing to a running app.", "depth": 0},
            {"kind": "page", "name": "guide", "title": "Building the app", "description": "What the platform gives an app.", "depth": 0},
            {"kind": "heading", "title": "Windows and input", "depth": 1},
            {"kind": "page", "name": "guide/windows", "title": "Windows", "description": "Ask for a window.", "depth": 2},
            {"kind": "heading", "title": "Network", "depth": 1},
            {"kind": "page", "name": "guide/network", "title": "Network", "description": "Reach the network.", "depth": 2},
            {"kind": "page", "name": "waft", "title": "Waft", "description": "The UI framework.", "depth": 0},
            {"kind": "page", "name": "waft/state", "title": "State", "description": null, "depth": 1},
            {"kind": "page", "name": "reference", "title": "Reference", "description": "Every interface.", "depth": 0},
            {"kind": "page", "name": "reference/wasdk-platform/windows", "title": "windows", "description": "wasdk:platform/windows@1.0.0", "depth": 2},
            {"kind": "page", "name": "icons", "title": "Icons", "description": "The icons.", "depth": 0}
        ]}"#,
    ),
    (
        "agents.md",
        "# Build an app on WaOS\n\n1. **Tools:** rustup.\n",
    ),
    (
        "docs/start.md",
        "# Start\n\nFrom nothing to a running app.\n\nInstall the WaOS command.\n",
    ),
    (
        "docs/guide.md",
        "# Building the app\n\nWhat the platform gives an app.\n",
    ),
    (
        "docs/guide/windows.md",
        "# Windows\n\nAsk for a window.\n\nA program's windows.\n",
    ),
    (
        "docs/guide/network.md",
        "# Network\n\nReach the network.\n\nAn HTTP GET.\nSee [State](/docs/waft/state#entities).\n",
    ),
    (
        "docs/waft.md",
        "# Waft\n\nThe UI framework.\n\nRef and Entity.\n",
    ),
    (
        "docs/waft/state.md",
        "# State\n\nAn entity holds the http get's answer.\n",
    ),
    ("docs/reference.md", "# Reference\n\nEvery interface.\n"),
    (
        "docs/reference/wasdk-platform/windows.md",
        "# windows\n\nwasdk:platform/windows@1.0.0\n\n```wit\ninterface windows {\n}\n```\n",
    ),
    (
        "docs/icons.md",
        "# Icons\n\nThe icons.\n\n- Notes: https://waos.app/apps/notes.png\n",
    ),
];

/// The archive of [`FILES`], gzipped, as a release carries it.
fn archive() -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (path, text) in FILES {
        let mut header = tar::Header::new_gnu();
        header.set_size(text.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, text.as_bytes()).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// The docs of [`FILES`], unpacked from their archive into a cache under `root`.
fn docs_in(root: &Path) -> Docs {
    let folder = cache::install(root, "9.8.7", &archive()).unwrap();
    Docs::from_bundle(&folder).unwrap()
}

fn docs() -> (tempfile::TempDir, Docs) {
    let root = tempfile::tempdir().unwrap();
    let docs = docs_in(root.path());
    (root, docs)
}

fn page_names(docs: &Docs) -> Vec<&str> {
    docs.pages().iter().map(|page| page.name.as_str()).collect()
}

#[test]
fn an_archive_unpacks_into_its_versions_folder_which_the_cache_then_finds() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(cache::bundle(root.path(), "9.8.7"), None);
    let folder = cache::install(root.path(), "9.8.7", &archive()).unwrap();
    assert_eq!(folder, root.path().join("9.8.7"));
    assert_eq!(cache::bundle(root.path(), "9.8.7"), Some(folder));
    let leftovers: Vec<_> = fs::read_dir(root.path()).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "only the version's folder is left");
}

#[test]
fn an_archive_without_a_manifest_is_refused_and_leaves_nothing_that_looks_done() {
    let root = tempfile::tempdir().unwrap();
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(2);
    header.set_cksum();
    tar.append_data(&mut header, "agents.md", &b"# "[..])
        .unwrap();
    let archive = tar.into_inner().unwrap().finish().unwrap();
    assert!(cache::install(root.path(), "1.0.0", &archive).is_err());
    assert_eq!(cache::bundle(root.path(), "1.0.0"), None);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn the_map_is_the_playbook_then_the_manifests_order_with_its_sections() {
    let (_root, docs) = docs();
    assert_eq!(docs.version, "9.8.7");
    assert_eq!(docs.origin, Origin::Release);
    assert_eq!(
        page_names(&docs),
        [
            "playbook",
            "start",
            "guide",
            "guide/windows",
            "guide/network",
            "waft",
            "waft/state",
            "reference",
            "reference/wasdk-platform/windows",
            "icons"
        ]
    );
    let map = terminal_map(&docs);
    assert!(map.starts_with("WaOS's docs, version 9.8.7's."), "{map}");
    assert!(
        map.contains("Building the app: What the platform gives an app.\n  Windows and input\n    guide/windows"),
        "{map}"
    );
    assert!(map.contains("playbook"), "{map}");
}

#[test]
fn a_page_is_found_by_its_name_its_address_or_the_end_of_its_name() {
    let (_root, docs) = docs();
    for asked in [
        "guide/network",
        "/docs/guide/network",
        "docs/guide/network.md",
        "https://waos.app/docs/guide/network#traps",
        "network",
    ] {
        assert_eq!(docs.find(asked).unwrap().name, "guide/network", "{asked}");
    }
    assert_eq!(docs.find("waft").unwrap().name, "waft");
    assert_eq!(
        docs.find("/docs/waft/state#entities").unwrap().name,
        "waft/state"
    );
    assert_eq!(docs.find("/agents.md").unwrap().name, "playbook");
    assert_eq!(
        docs.find("reference/wasdk-platform/windows").unwrap().name,
        "reference/wasdk-platform/windows"
    );
    assert_eq!(docs.find("windows").unwrap().name, "guide/windows");
}

#[test]
fn a_page_prints_as_the_archive_has_it() {
    let (_root, docs) = docs();
    assert_eq!(
        docs.find("network").unwrap().markdown,
        "# Network\n\nReach the network.\n\nAn HTTP GET.\nSee [State](/docs/waft/state#entities).\n"
    );
}

#[test]
fn an_unknown_page_names_close_ones_and_the_map() {
    let (_root, docs) = docs();
    let error = docs.find("guide/net").unwrap_err().to_string();
    assert!(error.contains("guide/network"), "{error}");
    assert!(error.contains("`cargo waos docs`"), "{error}");
    let error = docs.find("nothing-like-it").unwrap_err().to_string();
    assert!(error.contains("`cargo waos docs` lists them"), "{error}");
}

#[test]
fn a_search_names_each_line_by_page_and_number_in_the_maps_order() {
    let (_root, docs) = docs();
    assert_eq!(
        docs.search("HTTP GET"),
        [
            Hit {
                page: "guide/network".to_owned(),
                line: 5,
                text: "An HTTP GET.".to_owned(),
            },
            Hit {
                page: "waft/state".to_owned(),
                line: 3,
                text: "An entity holds the http get's answer.".to_owned(),
            },
        ]
    );
    assert_eq!(docs.search("not in any page"), []);
}

#[test]
fn the_skill_index_links_each_page_by_its_file() {
    let (_root, docs) = docs();
    let index = skill_index(&docs);
    assert!(
        index.starts_with("# WaOS's docs\n\nWaOS's docs, version 9.8.7's,"),
        "{index}"
    );
    assert!(
        index.contains("- [Build an app on WaOS](../SKILL.md)"),
        "{index}"
    );
    assert!(
        index.contains(
            "  - **Windows and input**\n    - [Windows](guide/windows.md): Ask for a window.\n"
        ),
        "{index}"
    );
}

#[test]
fn the_latest_docs_are_the_sites_pages_said_to_be_the_latest() {
    let pages = "# Start (https://waos.app/docs/start)\n\nInstall.\n\n# Network (https://waos.app/docs/guide/network)\n\nAn HTTP GET.\n";
    let docs = Docs::from_latest("9.8.7", pages, "# Build an app on WaOS\n\n1. Tools.\n");
    assert_eq!(docs.origin, Origin::Latest);
    assert_eq!(page_names(&docs), ["playbook", "start", "guide/network"]);
    assert_eq!(
        docs.find("network").unwrap().markdown,
        "# Network\n\nAn HTTP GET.\n"
    );
    let map = terminal_map(&docs);
    assert!(
        map.starts_with("WaOS's latest docs, from waos.app, which may not match version 9.8.7."),
        "{map}"
    );
}

#[test]
fn the_version_is_the_wasdk_an_apps_lock_file_names() {
    let lock = "version = 4\n\n[[package]]\nname = \"waft\"\nversion = \"0.6.0\"\n\n[[package]]\nname = \"wasdk\"\nversion = \"0.3.1\"\n";
    assert_eq!(version::wasdk_in(lock).as_deref(), Some("0.3.1"));
    assert_eq!(version::wasdk_in("version = 4\n"), None);
    assert_eq!(
        download::archive_url("0.3.1"),
        "https://github.com/waos-dev/waos/releases/download/v0.3.1/waos-docs-0.3.1.tar.gz"
    );
}

/// The archive the site's `write-docs-bundle.mjs` writes, read as a release's: run it, then
/// `WAOS_DOCS_ARCHIVE=<archive> cargo test -p waos-cli -- --ignored`.
#[test]
#[ignore = "needs an archive from packages/waos-site's `npm run docs-bundle`"]
fn the_bundle_scripts_archive_serves_the_sites_hierarchy() {
    let archive = fs::read(std::env::var("WAOS_DOCS_ARCHIVE").unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let folder = cache::install(root.path(), "archive", &archive).unwrap();
    let docs = Docs::from_bundle(&folder).unwrap();
    let top: Vec<&str> = docs
        .lines()
        .iter()
        .filter_map(|line| match line {
            Line::Page { name, depth: 0 } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        top,
        [
            "playbook",
            "start",
            "concepts",
            "guide",
            "waft",
            "tools",
            "reference",
            "compatibility",
            "icons"
        ]
    );
    for name in [
        "guide/network",
        "waft",
        "guide/keys",
        "network",
        "reference/wasdk-platform/windows",
        "reference/wasdk-media/cameras",
    ] {
        assert!(docs.find(name).is_ok(), "{name}");
    }
    for page in docs.pages() {
        assert!(!page.markdown.contains("<AppIcon"), "{}", page.name);
        assert!(!page.markdown.starts_with("---"), "{}", page.name);
    }
    let skills = tempfile::tempdir().unwrap();
    let written = crate::skill::install(skills.path(), &docs).unwrap();
    assert!(written.join("SKILL.md").is_file());
    assert!(
        written
            .join("docs/reference/wasdk-platform/windows.md")
            .is_file()
    );
}
