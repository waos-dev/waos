use std::fs;

use super::{Generated, World, generate};

/// A package's WIT, with a world importing its one interface, and what is generated from it.
fn generated_from(interface: &str) -> Generated {
    let wit = tempfile::tempdir().unwrap();
    let package = format!(
        "package a:shapes;\n\ninterface things {{\n{interface}\n}}\n\nworld w {{ import things; }}\n"
    );
    fs::write(wit.path().join("shapes.wit"), package).unwrap();
    generate(wit.path(), &[program_world()]).unwrap()
}

/// The world `w`, whose types the engine's `crate::bindings` has.
fn program_world() -> World {
    World {
        name: "a:shapes/w",
        engine: "crate::bindings",
    }
}

#[test]
fn a_record_is_a_struct_at_bindgens_path_with_its_docs() {
    let generated = generated_from("/// Where.\nrecord point { x: f64, y: f64 }");
    assert!(
        generated.types.contains(
            "pub mod a {\npub mod shapes {\npub mod things {\n/// Where.\n#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]\npub struct Point {\npub x: f64,\npub y: f64,\n}\n"
        ),
        "{}",
        generated.types
    );
}

#[test]
fn a_record_converts_both_ways_field_by_field() {
    let generated =
        generated_from("enum side { left, right }\nrecord edge { side: side, at: f64 }");
    let to_engine = "impl From<waos_capabilities::wit::a::shapes::things::Edge> for crate::bindings::a::shapes::things::Edge { fn from(value: waos_capabilities::wit::a::shapes::things::Edge) -> Self { crate::bindings::a::shapes::things::Edge { side: value.side.into(),at: value.at, } } }";
    let to_plain = "impl From<crate::bindings::a::shapes::things::Edge> for waos_capabilities::wit::a::shapes::things::Edge";
    assert!(
        generated.conversions.contains(to_engine),
        "{}",
        generated.conversions
    );
    assert!(
        generated.conversions.contains(to_plain),
        "{}",
        generated.conversions
    );
}

#[test]
fn a_field_named_by_a_keyword_ends_in_an_underscore_as_bindgens_does() {
    let generated = generated_from("record token { %type: u8 }");
    assert!(
        generated.types.contains("pub type_: u8,"),
        "{}",
        generated.types
    );
}

#[test]
fn flags_are_bools_set_from_and_read_as_bindgens_constants() {
    let generated = generated_from("flags held { primary, back-button }");
    assert!(
        generated
            .types
            .contains("pub primary: bool,\npub back_button: bool,"),
        "{}",
        generated.types
    );
    let engine = "crate::bindings::a::shapes::things::Held";
    assert!(
        generated.conversions.contains(&format!(
            "if value.back_button {{ flags |= {engine}::BACK_BUTTON; }}"
        )),
        "{}",
        generated.conversions
    );
    assert!(
        generated.conversions.contains(&format!(
            "back_button: value.contains({engine}::BACK_BUTTON),"
        )),
        "{}",
        generated.conversions
    );
}

#[test]
fn an_alias_of_an_option_converts_inside_it_and_has_no_conversion_of_its_own() {
    let generated = generated_from(
        "enum side { left, right }\ntype maybe-side = option<side>;\nrecord edge { side: maybe-side, sides: list<side>, count: option<u32> }",
    );
    assert!(
        generated
            .types
            .contains("pub type MaybeSide = Option<crate::wit::a::shapes::things::Side>;"),
        "{}",
        generated.types
    );
    assert!(
        generated.conversions.contains("side: value.side.map(|value| value.into()),sides: value.sides.into_iter().map(|value| value.into()).collect(),count: value.count,"),
        "{}",
        generated.conversions
    );
    assert!(
        !generated
            .conversions
            .contains("for crate::bindings::a::shapes::things::MaybeSide")
    );
}

#[test]
fn a_type_holding_a_resource_is_left_out_and_named_but_not_the_resource() {
    let generated = generated_from("resource cursor;\nrecord place { at: cursor, line: u32 }");
    assert!(
        !generated.types.contains("struct Place"),
        "{}",
        generated.types
    );
    assert!(
        !generated.conversions.contains("Place"),
        "{}",
        generated.conversions
    );
    assert!(
        generated.types.contains("//! - `a:shapes/things.place`\n"),
        "{}",
        generated.types
    );
    assert!(
        !generated.types.contains("things.cursor"),
        "{}",
        generated.types
    );
}

#[test]
fn of_a_package_bindgen_takes_from_another_crate_an_alias_is_what_it_names_and_a_record_is_left_out()
 {
    let wit = tempfile::tempdir().unwrap();
    let clocks = "package wasi:clocks;\n\ninterface types {\ntype duration = u64;\nrecord instant { seconds: u64 }\n}\n";
    fs::create_dir_all(wit.path().join("deps/clocks")).unwrap();
    fs::write(wit.path().join("deps/clocks/clocks.wit"), clocks).unwrap();
    let shapes = "package a:shapes;\n\ninterface things {\nuse wasi:clocks/types.{duration, instant};\nrecord wait { %for: duration }\nrecord alarm { at: instant }\n}\n\nworld w { import things; }\n";
    fs::write(wit.path().join("shapes.wit"), shapes).unwrap();
    let generated = generate(wit.path(), &[program_world()]).unwrap();
    assert!(
        generated.types.contains("pub type Duration = u64;"),
        "{}",
        generated.types
    );
    assert!(
        generated
            .types
            .contains("pub for_: crate::wit::a::shapes::things::Duration,"),
        "{}",
        generated.types
    );
    assert!(
        !generated.types.contains("pub mod wasi"),
        "{}",
        generated.types
    );
    assert!(
        !generated.types.contains("struct Alarm"),
        "{}",
        generated.types
    );
    assert!(
        generated.types.contains("//! - `a:shapes/things.alarm`\n"),
        "{}",
        generated.types
    );
}

#[test]
fn a_second_worlds_types_convert_to_its_own_bindings_and_one_imported_by_both_is_generated_once() {
    let wit = tempfile::tempdir().unwrap();
    let package = "package a:shapes;\n\ninterface things {\n  record point { x: f64 }\n}\n\n\
                   interface looks {\n  record view { at: f64 }\n}\n\n\
                   world w { import things; }\n\nworld v { import things; import looks; }\n";
    fs::write(wit.path().join("shapes.wit"), package).unwrap();
    let worlds = [
        program_world(),
        World {
            name: "a:shapes/v",
            engine: "crate::looking::bindings",
        },
    ];
    let generated = generate(wit.path(), &worlds).unwrap();
    assert_eq!(
        generated.types.matches("pub struct Point").count(),
        1,
        "{}",
        generated.types
    );
    assert!(
        generated.conversions.contains(
            "impl From<waos_capabilities::wit::a::shapes::looks::View> for crate::looking::bindings::a::shapes::looks::View"
        ),
        "{}",
        generated.conversions
    );
    assert!(
        generated.conversions.contains(
            "impl From<waos_capabilities::wit::a::shapes::things::Point> for crate::bindings::a::shapes::things::Point"
        ),
        "{}",
        generated.conversions
    );
}
