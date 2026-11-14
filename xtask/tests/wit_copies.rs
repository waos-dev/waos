//! Every crate's copy of the WIT is `wit/` as it is now.

use xtask::wit_copies::COPIES;

#[test]
fn every_copy_of_the_wit_is_wit_as_it_is() {
    let root = xtask::workspace_root();
    let mut drifted = Vec::new();
    for copy in &COPIES {
        drifted.extend(copy.drift(&root).unwrap());
    }
    let named: Vec<String> = drifted.iter().map(ToString::to_string).collect();
    assert!(
        named.is_empty(),
        "a copy of the WIT differs from wit/; `cargo xtask sync-wit` copies it again:\n{}",
        named.join("\n")
    );
}
