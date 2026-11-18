use super::*;

/// A small tree of the site's shape: the playbook, top pages, a folder with an index and
/// separators, a folder of its own pages, the reference with an interface's page, and the
/// icons page.
const FILES: &[(&str, &str)] = &[
    (
        "playbook.md",
        "# Build an app on WaOS\n\n1. **Tools:** rustup.\n",
    ),
    (
        "docs/meta.json",
        r#"{"title": "WaOS", "pages": ["start", "guide", "waft", "reference", "icons"]}"#,
    ),
    (
        "docs/start.mdx",
        "---\ntitle: Start\ndescription: From nothing to a running app.\n---\n\nInstall the WaOS command.\n",
    ),
    (
        "docs/guide/meta.json",
        r#"{"title": "Building the app", "pages": ["---Windows and input---", "windows", "---Network---", "network"]}"#,
    ),
    (
        "docs/guide/index.mdx",
        "---\ntitle: Building the app\ndescription: What the platform gives an app.\n---\n",
    ),
    (
        "docs/guide/windows.mdx",
        "---\ntitle: Windows\ndescription: Ask for a window.\n---\n\nA program's windows.\n",
    ),
    (
        "docs/guide/network.mdx",
        "---\ntitle: Network\ndescription: Reach the network.\n---\n\nAn HTTP GET.\nSee [State](/docs/waft/state#entities).\n",
    ),
    (
        "docs/waft/meta.json",
        r#"{"title": "Waft", "pages": ["state"]}"#,
    ),
    (
        "docs/waft/index.mdx",
        "---\ntitle: Waft\ndescription: \"The UI framework: Flutter's, in Rust.\"\n---\n\nRef and Entity.\n",
    ),
    (
        "docs/waft/state.mdx",
        "---\ntitle: State\ndescription: Where an app keeps its data.\n---\n\nAn entity holds the http get's answer.\n",
    ),
    (
        "docs/reference/meta.json",
        r#"{"title": "Reference", "pages": ["wasdk-platform"]}"#,
    ),
    (
        "docs/reference/index.mdx",
        "---\ntitle: Reference\ndescription: Every interface.\n---\n",
    ),
    (
        "docs/reference/wasdk-platform/meta.json",
        r#"{"title": "wasdk:platform", "pages": ["windows"]}"#,
    ),
    (
        "docs/reference/wasdk-platform/index.md",
        "---\ntitle: \"wasdk:platform\"\ndescription: \"wasdk:platform@1.0.0\"\n---\n",
    ),
    (
        "docs/reference/wasdk-platform/windows.md",
        "---\ntitle: \"windows\"\ndescription: \"wasdk:platform/windows@1.0.0\"\n---\n\n```wit\ninterface windows {\n}\n```\n",
    ),
    (
        "docs/icons.mdx",
        "---\ntitle: Icons\ndescription: The icons.\n---\n\n<AppIcons>\n  <AppIcon name=\"Notes\" src=\"/apps/notes.png\" />\n</AppIcons>\n",
    ),
];

fn docs() -> Docs {
    Docs::from_files(FILES)
}

fn page_names(docs: &Docs) -> Vec<&str> {
    docs.pages().iter().map(|page| page.name.as_str()).collect()
}

#[test]
fn the_map_is_the_playbook_then_the_sidebars_order_with_its_sections() {
    let docs = docs();
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
            "reference/wasdk-platform",
            "reference/wasdk-platform/windows",
            "icons"
        ]
    );
    let heading = |title: &str, depth| Line::Heading {
        title: title.to_owned(),
        depth,
    };
    let page = |name: &str, depth| Line::Page {
        name: name.to_owned(),
        depth,
    };
    assert_eq!(
        docs.lines()[2..5],
        [
            page("guide", 0),
            heading("Windows and input", 1),
            page("guide/windows", 2),
        ]
    );
    assert_eq!(docs.lines()[7], page("waft", 0));
    assert_eq!(docs.lines()[8], page("waft/state", 1));
}

#[test]
fn the_terminal_map_says_how_to_read_on_and_gives_a_line_a_page() {
    let map = terminal_map(&docs());
    let mut lines = map.lines();
    let header = lines.next().unwrap();
    assert!(header.contains("`cargo waos docs <page>`"), "{header}");
    assert!(header.contains("--search"), "{header}");
    assert!(
        header.contains("a link to /docs/<page> in a page is `cargo waos docs <page>`"),
        "{header}"
    );
    assert!(map.contains("\nplaybook "), "{map}");
    assert!(map.contains("Build an app on WaOS: the steps"), "{map}");
    assert!(
        map.contains("\n  Windows and input\n    guide/windows "),
        "{map}"
    );
    assert!(
        map.contains("Waft: The UI framework: Flutter's, in Rust.\n"),
        "{map}"
    );
    let summaries: Vec<usize> = map
        .lines()
        .filter(|line| line.contains("Ask for a window") || line.contains("Reach the network"))
        .map(|line| line.find("Windows:").or(line.find("Network:")).unwrap())
        .collect();
    assert_eq!(summaries[0], summaries[1], "aligned:\n{map}");
}

#[test]
fn a_page_is_found_by_its_name_its_address_or_the_end_of_its_name() {
    let docs = docs();
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
}

#[test]
fn a_page_prints_as_markdown_with_its_title_and_description() {
    let markdown = docs().find("network").unwrap().markdown();
    assert_eq!(
        markdown,
        "# Network\n\nReach the network.\n\nAn HTTP GET.\nSee [State](/docs/waft/state#entities).\n"
    );
}

#[test]
fn an_interfaces_page_is_found_by_its_address_and_yields_its_name_to_a_written_page() {
    let docs = docs();
    let windows = docs.find("/docs/reference/wasdk-platform/windows").unwrap();
    assert_eq!(windows.name, "reference/wasdk-platform/windows");
    assert!(windows.markdown().contains("interface windows {"));
    assert_eq!(docs.find("windows").unwrap().name, "guide/windows");
}

#[test]
fn an_unknown_page_names_close_ones_and_the_map() {
    let error = docs().find("guide/net").unwrap_err().to_string();
    assert!(error.contains("guide/network"), "{error}");
    assert!(error.contains("`cargo waos docs`"), "{error}");
    let error = docs().find("nothing-like-it").unwrap_err().to_string();
    assert!(error.contains("`cargo waos docs` lists them"), "{error}");
}

#[test]
fn a_search_names_each_line_by_page_and_number_in_the_maps_order() {
    let hits = docs().search("HTTP GET");
    assert_eq!(
        hits,
        [
            Hit {
                page: "guide/network".to_owned(),
                line: 5,
                text: "An HTTP GET.".to_owned(),
            },
            Hit {
                page: "waft/state".to_owned(),
                line: 5,
                text: "An entity holds the http get's answer.".to_owned(),
            },
        ]
    );
    assert_eq!(docs().search("not in any page"), []);
}

#[test]
fn the_skill_index_links_each_page_by_its_file() {
    let index = skill_index(&docs());
    assert!(index.starts_with("# WaOS's docs\n"), "{index}");
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
    assert!(
        index.contains("- [Icons](icons.md): The icons.\n"),
        "{index}"
    );
}

#[test]
fn the_bundled_docs_follow_the_sites_hierarchy() {
    let docs = Docs::bundled();
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
        "waft/state",
        "waft/text-input",
        "network",
        "reference/wasdk-platform/windows",
        "reference/wasdk-media/cameras",
    ] {
        assert!(docs.find(name).is_ok(), "{name}");
    }
    for page in docs.pages() {
        assert!(!page.markdown().contains("<AppIcon"), "{}", page.name);
    }
}
