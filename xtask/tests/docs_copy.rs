//! `cargo waos`'s copy of the docs is the site's content as it is now.

#[test]
fn the_cli_copy_of_the_docs_is_the_sites_as_it_is() {
    let root = xtask::workspace_root();
    let drifted = xtask::docs_copy::COPY.drift(&root).unwrap();
    let named: Vec<String> = drifted.iter().map(ToString::to_string).collect();
    assert!(
        named.is_empty(),
        "cargo waos's copy of the docs differs from packages/waos-site/content; `cargo xtask sync-docs` copies it again:\n{}",
        named.join("\n")
    );
}
