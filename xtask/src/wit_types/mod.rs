//! The WIT's value types as plain Rust, and the engine's conversions between them and its own,
//! generated from `wit/` by `cargo xtask sync-wit` into files that are committed, so no build pays
//! for it.
//!
//! The engine's Rust types of the WIT are wasmtime's `bindgen!`'s, which carry wasmtime's
//! machinery, so whatever names them depends on the engine. The contract the engine is handed,
//! `waos-capabilities`, depends on neither the engine nor wasmtime, and holds the same types as
//! plain Rust ([`TYPES`]): records, variants, enums, flags (a struct of `bool`s) and aliases (of
//! options, lists, tuples, results and other types), with `Clone`, `Debug`, `PartialEq` and serde's
//! derives, each with its WIT's docs. `bindgen!` cannot be pointed at another crate's record or
//! enum, so the engine converts at the boundary, with `From` both ways ([`CONVERSIONS`]), which the
//! orphan rule allows there.
//!
//! Every interface the engine's worlds ([`WORLDS`]) import is generated, each converting to the
//! types of the engine's `bindgen!` for its world, but those whose Rust types the engine's `bindgen!`
//! takes from another crate ([`TAKEN_ELSEWHERE`]): the orphan rule forbids
//! the engine a conversion between two other crates' types. A type that holds a resource, a stream
//! or a future cannot be plain, and one that holds a record, a variant, an enum or flags of those
//! interfaces cannot be converted: each is left out, and named in the types' module docs. An alias
//! of a resource is not a value type, and is left out unnamed; an alias of another crate's
//! interface is written as what it names.
//!
//! The invariant a later editor will break: a type is at `bindgen!`'s path for it, with
//! `bindgen!`'s names, `<namespace>::<package>::<interface>::<Name>` ([`module_path`]), so the
//! conversions name both sides alike and a reader finds a type where the engine's is. Two versions
//! of one package side by side are not handled, since the WIT has none: `bindgen!` then writes the
//! version into the package's module name, and [`module_path`] would have to do the same.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use heck::{ToShoutySnakeCase, ToSnakeCase, ToUpperCamelCase};
use wit_parser::{
    Docs, InterfaceId, Resolve, Type, TypeDefKind, TypeId, TypeOwner, WorldId, WorldItem,
};

/// A world whose imports are generated, and where the engine's `bindgen!` of it puts its types.
pub struct World {
    pub name: &'static str,
    pub engine: &'static str,
}

/// The worlds whose imports are generated, in order: the program's, the engine's own `bindgen!`,
/// then the inspector's, which the engine's `wasdk:inspect` host runs; an interface the first
/// generates is not generated again.
pub const WORLDS: &[World] = &[
    World {
        name: "wasdk:program/program",
        engine: "crate::bindings",
    },
    World {
        name: "wasdk:inspect/inspection",
        engine: "crate::inspection::bindings",
    },
];

/// The plain types, from the workspace's root: `waos_capabilities::wit`.
pub const TYPES: &str = "crates/waos-capabilities/src/wit.rs";

/// The engine's conversions, from the workspace's root.
pub const CONVERSIONS: &str = "crates/waos/src/wit_conversions.rs";

/// Where the plain types are seen from the code that names them: inside their crate, and from the
/// engine.
const PLAIN_IN_ITS_CRATE: &str = "crate::wit";
const PLAIN_FROM_THE_ENGINE: &str = "waos_capabilities::wit";

/// The two files' Rust, before it is formatted.
pub struct Generated {
    pub types: String,
    pub conversions: String,
}

/// Generates the files from the workspace's `wit/` and writes them, formatted as `cargo fmt`
/// formats the workspace.
pub fn sync(root: &Path) -> anyhow::Result<()> {
    let generated = generate(&root.join("wit"), WORLDS)?;
    write_formatted(root, TYPES, &generated.types)?;
    write_formatted(root, CONVERSIONS, &generated.conversions)
}

/// The plain types and their conversions of every interface `worlds` import, from the WIT in
/// `wit`, each interface once, converting to the engine's types of the first world that imports
/// it.
pub fn generate(wit: &Path, worlds: &[World]) -> anyhow::Result<Generated> {
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_dir(wit)?;
    let mut modules = BTreeMap::<Vec<String>, String>::new();
    let mut conversions = String::new();
    let mut left_out = Vec::new();
    let mut generated = BTreeSet::new();
    for world in worlds {
        let id = resolve.select_world(&[package], Some(world.name))?;
        for ty in imported_types(&resolve, id) {
            if !generated.insert(ty) {
                continue;
            }
            match value_type(&resolve, ty) {
                Some(Value::Plain) => {
                    let definition = definition(&resolve, ty);
                    modules
                        .entry(module_path(&resolve, ty))
                        .or_default()
                        .push_str(&definition);
                    let engine = world.engine;
                    conversions += &conversion(&resolve, ty, PLAIN_FROM_THE_ENGINE, engine);
                    conversions += &conversion(&resolve, ty, engine, PLAIN_FROM_THE_ENGINE);
                }
                Some(Value::LeftOut(reason)) => left_out.push((reason, wit_name(&resolve, ty))),
                None => {}
            }
        }
    }
    Ok(Generated {
        types: types_file(&left_out, &nested_modules(&modules)),
        conversions: conversions_file(&conversions),
    })
}

/// Formats `source` with rustfmt, as `cargo fmt` does for the workspace's edition, and writes it
/// to `file`, from `root`.
fn write_formatted(root: &Path, file: &str, source: &str) -> anyhow::Result<()> {
    let mut rustfmt = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("starting rustfmt")?;
    rustfmt
        .stdin
        .take()
        .expect("rustfmt's input is piped")
        .write_all(source.as_bytes())?;
    let formatted = rustfmt.wait_with_output()?;
    if !formatted.status.success() {
        bail!("rustfmt refused what was generated for {file}");
    }
    fs::write(root.join(file), formatted.stdout).with_context(|| format!("writing {file}"))
}

/// Every named type of every interface `world` imports that the engine's `bindgen!` generates
/// itself, in the WIT's order.
fn imported_types(resolve: &Resolve, world: WorldId) -> Vec<TypeId> {
    resolve.worlds[world]
        .imports
        .values()
        .filter_map(|item| match item {
            WorldItem::Interface { id, .. } if !is_taken_elsewhere(resolve, *id) => Some(*id),
            _ => None,
        })
        .flat_map(|interface| resolve.interfaces[interface].types.values().copied())
        .collect()
}

/// The packages whose Rust types the engine's `bindgen!` takes from another crate with `with`
/// (`crates/waos/src/bindings.rs`), by namespace and name, none for the whole namespace:
/// `wasdk:gpu`'s are the GPU host's and WASI's are wasmtime's. The orphan rule forbids the engine
/// a conversion between another crate's type and the contract's, so none of their types is
/// generated.
const TAKEN_ELSEWHERE: &[(&str, Option<&str>)] = &[("wasdk", Some("gpu")), ("wasi", None)];

fn is_taken_elsewhere(resolve: &Resolve, interface: InterfaceId) -> bool {
    let package = &resolve.packages[resolve.interfaces[interface]
        .package
        .expect("an imported interface is a package's")]
    .name;
    TAKEN_ELSEWHERE.iter().any(|(namespace, name)| {
        package.namespace == *namespace && name.is_none_or(|name| package.name == name)
    })
}

/// What a named type is to the generator.
enum Value {
    /// A value type with nothing in it that a plain type cannot hold, which is generated.
    Plain,
    /// A value type left out, and named in the types' module docs.
    LeftOut(LeftOut),
}

/// Why a value type is left out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftOut {
    /// It holds a resource, a stream or a future, which a plain value cannot.
    HoldsResource,
    /// It holds a record, a variant, an enum or flags of a package in [`TAKEN_ELSEWHERE`], which
    /// the engine cannot convert.
    HoldsTypeTakenElsewhere,
}

/// What `ty` is to the generator; none for a resource, a handle, a stream or a future, or an
/// alias of one.
fn value_type(resolve: &Resolve, ty: TypeId) -> Option<Value> {
    let ty = &Type::Id(ty);
    if is_handle_like(resolve, ty) {
        None
    } else if any_part(resolve, ty, &|id| is_handle_like(resolve, &Type::Id(id))) {
        Some(Value::LeftOut(LeftOut::HoldsResource))
    } else if any_part(resolve, ty, &|id| {
        is_named_type_taken_elsewhere(resolve, id)
    }) {
        Some(Value::LeftOut(LeftOut::HoldsTypeTakenElsewhere))
    } else {
        Some(Value::Plain)
    }
}

/// Whether `ty` is a resource, a handle, a stream or a future, or an alias of one.
fn is_handle_like(resolve: &Resolve, ty: &Type) -> bool {
    let Type::Id(id) = ty else { return false };
    match &resolve.types[*id].kind {
        TypeDefKind::Resource
        | TypeDefKind::Handle(_)
        | TypeDefKind::Stream(_)
        | TypeDefKind::Future(_) => true,
        TypeDefKind::Type(ty) => is_handle_like(resolve, ty),
        _ => false,
    }
}

/// Whether `id` is a record, a variant, an enum or flags of a package in [`TAKEN_ELSEWHERE`]: an
/// alias there converts as what it names.
fn is_named_type_taken_elsewhere(resolve: &Resolve, id: TypeId) -> bool {
    is_converted_whole(&resolve.types[id].kind)
        && matches!(resolve.types[id].owner, TypeOwner::Interface(interface) if is_taken_elsewhere(resolve, interface))
}

/// Whether a value of `ty`, or any value inside it, is of a type for which `found` holds, looking
/// through aliases, options, lists, tuples, results, records and variants.
fn any_part(resolve: &Resolve, ty: &Type, found: &dyn Fn(TypeId) -> bool) -> bool {
    let Type::Id(id) = ty else { return false };
    if found(*id) {
        return true;
    }
    let part = |ty: &Type| any_part(resolve, ty, found);
    let optional_part = |ty: &Option<Type>| ty.as_ref().is_some_and(part);
    match &resolve.types[*id].kind {
        TypeDefKind::Record(record) => record.fields.iter().any(|field| part(&field.ty)),
        TypeDefKind::Variant(variant) => variant.cases.iter().any(|case| optional_part(&case.ty)),
        TypeDefKind::Tuple(tuple) => tuple.types.iter().any(part),
        TypeDefKind::Option(ty) | TypeDefKind::List(ty) | TypeDefKind::Type(ty) => part(ty),
        TypeDefKind::Result(result) => optional_part(&result.ok) || optional_part(&result.err),
        _ => false,
    }
}

/// Whether a type of `kind` converts by its own `From`: a record, a variant, an enum or flags,
/// whose Rust types differ between the two sides.
fn is_converted_whole(kind: &TypeDefKind) -> bool {
    matches!(
        kind,
        TypeDefKind::Record(_)
            | TypeDefKind::Variant(_)
            | TypeDefKind::Enum(_)
            | TypeDefKind::Flags(_)
    )
}

const VALUE_DERIVES: &str =
    "#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]\n";
const ENUM_DERIVES: &str =
    "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]\n";
const FLAGS_DERIVES: &str = "#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]\n";

/// The plain Rust of the named type `ty`, with its docs.
fn definition(resolve: &Resolve, ty: TypeId) -> String {
    let def = &resolve.types[ty];
    let name = type_name(resolve, ty);
    let docs = doc_comment(&def.docs);
    match &def.kind {
        TypeDefKind::Record(record) => {
            let fields: String = record
                .fields
                .iter()
                .map(|field| {
                    let docs = doc_comment(&field.docs);
                    let name = field_name(&field.name);
                    format!("{docs}pub {name}: {},\n", rust_type(resolve, &field.ty))
                })
                .collect();
            format!("{docs}{VALUE_DERIVES}pub struct {name} {{\n{fields}}}\n")
        }
        TypeDefKind::Flags(flags) => {
            let fields: String = flags
                .flags
                .iter()
                .map(|flag| {
                    let docs = doc_comment(&flag.docs);
                    format!("{docs}pub {}: bool,\n", field_name(&flag.name))
                })
                .collect();
            format!("{docs}{FLAGS_DERIVES}pub struct {name} {{\n{fields}}}\n")
        }
        TypeDefKind::Enum(enumeration) => {
            let cases: String = enumeration
                .cases
                .iter()
                .map(|case| {
                    let docs = doc_comment(&case.docs);
                    format!("{docs}{},\n", case.name.to_upper_camel_case())
                })
                .collect();
            format!("{docs}{ENUM_DERIVES}pub enum {name} {{\n{cases}}}\n")
        }
        TypeDefKind::Variant(variant) => {
            let cases: String = variant
                .cases
                .iter()
                .map(|case| {
                    let docs = doc_comment(&case.docs);
                    let case_name = case.name.to_upper_camel_case();
                    match &case.ty {
                        Some(ty) => format!("{docs}{case_name}({}),\n", rust_type(resolve, ty)),
                        None => format!("{docs}{case_name},\n"),
                    }
                })
                .collect();
            format!("{docs}{VALUE_DERIVES}pub enum {name} {{\n{cases}}}\n")
        }
        kind => format!("{docs}pub type {name} = {};\n", rust_kind(resolve, kind)),
    }
}

/// The Rust type of a value of WIT type `ty`, a named type by its path from the plain types'
/// root.
fn rust_type(resolve: &Resolve, ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".into(),
        Type::U8 => "u8".into(),
        Type::U16 => "u16".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::S8 => "i8".into(),
        Type::S16 => "i16".into(),
        Type::S32 => "i32".into(),
        Type::S64 => "i64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Char => "char".into(),
        Type::String => "String".into(),
        Type::ErrorContext => unimplemented!("an error-context in a value type"),
        Type::Id(id) if is_named_here(resolve, *id) => type_path(resolve, *id, PLAIN_IN_ITS_CRATE),
        Type::Id(id) => rust_kind(resolve, &resolve.types[*id].kind),
    }
}

/// Whether `id` is named among the plain types: named, and not of a package in
/// [`TAKEN_ELSEWHERE`], whose aliases are written as what they name.
fn is_named_here(resolve: &Resolve, id: TypeId) -> bool {
    let def = &resolve.types[id];
    def.name.is_some()
        && !matches!(def.owner, TypeOwner::Interface(interface) if is_taken_elsewhere(resolve, interface))
}

/// The Rust type of an unnamed WIT type, or of what an alias names.
fn rust_kind(resolve: &Resolve, kind: &TypeDefKind) -> String {
    match kind {
        TypeDefKind::Type(ty) => rust_type(resolve, ty),
        TypeDefKind::Option(ty) => format!("Option<{}>", rust_type(resolve, ty)),
        TypeDefKind::List(ty) => format!("Vec<{}>", rust_type(resolve, ty)),
        TypeDefKind::Tuple(tuple) => {
            let parts: Vec<String> = tuple
                .types
                .iter()
                .map(|ty| rust_type(resolve, ty))
                .collect();
            format!("({},)", parts.join(", "))
        }
        TypeDefKind::Result(result) => {
            let part = |ty: &Option<Type>| ty.map_or("()".into(), |ty| rust_type(resolve, &ty));
            format!("Result<{}, {}>", part(&result.ok), part(&result.err))
        }
        other => unimplemented!("{other:?} in a value type"),
    }
}

/// `From<from::T> for to::T` for a record, a variant, an enum or flags, its parts converted one by
/// one; nothing for an alias, which converts as what it names.
fn conversion(resolve: &Resolve, ty: TypeId, from: &str, to: &str) -> String {
    let (source, target) = (type_path(resolve, ty, from), type_path(resolve, ty, to));
    let body = match &resolve.types[ty].kind {
        TypeDefKind::Record(record) => {
            let fields: String = record
                .fields
                .iter()
                .map(|field| {
                    let name = field_name(&field.name);
                    let value = converted(resolve, &field.ty, &format!("value.{name}"));
                    format!("{name}: {value},")
                })
                .collect();
            format!("{target} {{ {fields} }}")
        }
        TypeDefKind::Flags(flags) if from != PLAIN_FROM_THE_ENGINE => {
            let fields: String = flags
                .flags
                .iter()
                .map(|flag| {
                    let constant = flag.name.to_shouty_snake_case();
                    let name = field_name(&flag.name);
                    format!("{name}: value.contains({source}::{constant}),")
                })
                .collect();
            format!("{target} {{ {fields} }}")
        }
        TypeDefKind::Flags(flags) => {
            let sets: String = flags
                .flags
                .iter()
                .map(|flag| {
                    let constant = flag.name.to_shouty_snake_case();
                    let name = field_name(&flag.name);
                    format!("if value.{name} {{ flags |= {target}::{constant}; }}")
                })
                .collect();
            format!("let mut flags = {target}::empty(); {sets} flags")
        }
        TypeDefKind::Enum(enumeration) => {
            let arms: String = enumeration
                .cases
                .iter()
                .map(|case| {
                    let case = case.name.to_upper_camel_case();
                    format!("{source}::{case} => {target}::{case},")
                })
                .collect();
            format!("match value {{ {arms} }}")
        }
        TypeDefKind::Variant(variant) => {
            let arms: String = variant
                .cases
                .iter()
                .map(|case| {
                    let name = case.name.to_upper_camel_case();
                    match &case.ty {
                        Some(ty) => format!(
                            "{source}::{name}(value) => {target}::{name}({}),",
                            converted(resolve, ty, "value")
                        ),
                        None => format!("{source}::{name} => {target}::{name},"),
                    }
                })
                .collect();
            format!("match value {{ {arms} }}")
        }
        _ => return String::new(),
    };
    format!(
        "impl From<{source}> for {target} {{ fn from(value: {source}) -> Self {{ {body} }} }}\n"
    )
}

/// The expression that converts `value`, of WIT type `ty`, to the other side's type: `value`
/// itself where nothing in it differs between the sides.
fn converted(resolve: &Resolve, ty: &Type, value: &str) -> String {
    if !any_part(resolve, ty, &|id| {
        is_converted_whole(&resolve.types[id].kind)
    }) {
        return value.into();
    }
    let Type::Id(id) = ty else {
        unreachable!("a primitive holds no generated type")
    };
    match &resolve.types[*id].kind {
        kind if is_converted_whole(kind) => format!("{value}.into()"),
        TypeDefKind::Type(ty) => converted(resolve, ty, value),
        TypeDefKind::Option(ty) => {
            format!("{value}.map(|value| {})", converted(resolve, ty, "value"))
        }
        TypeDefKind::List(ty) => format!(
            "{value}.into_iter().map(|value| {}).collect()",
            converted(resolve, ty, "value")
        ),
        TypeDefKind::Tuple(tuple) => {
            let names: Vec<String> = (0..tuple.types.len()).map(|i| format!("part{i}")).collect();
            let parts: Vec<String> = tuple
                .types
                .iter()
                .zip(&names)
                .map(|(ty, name)| converted(resolve, ty, name))
                .collect();
            format!(
                "{{ let ({},) = {value}; ({},) }}",
                names.join(", "),
                parts.join(", ")
            )
        }
        TypeDefKind::Result(result) => {
            let part = |ty: &Option<Type>| {
                ty.map_or("value".into(), |ty| converted(resolve, &ty, "value"))
            };
            format!(
                "{value}.map(|value| {}).map_err(|value| {})",
                part(&result.ok),
                part(&result.err)
            )
        }
        other => unimplemented!("{other:?} in a value type"),
    }
}

/// The modules `bindgen!` puts `ty` in: its package's namespace and name, and its interface.
fn module_path(resolve: &Resolve, ty: TypeId) -> Vec<String> {
    let TypeOwner::Interface(id) = resolve.types[ty].owner else {
        unimplemented!("a type of a world's own")
    };
    let interface = &resolve.interfaces[id];
    let package = &resolve.packages[interface
        .package
        .expect("an imported interface is a package's")]
    .name;
    let interface_name = interface
        .name
        .as_ref()
        .expect("an imported interface is named");
    [&package.namespace, &package.name, interface_name]
        .map(|name| field_name(name))
        .to_vec()
}

/// `ty`'s path under `root`, as `bindgen!` lays its modules out.
fn type_path(resolve: &Resolve, ty: TypeId, root: &str) -> String {
    let modules = module_path(resolve, ty).join("::");
    format!("{root}::{modules}::{}", type_name(resolve, ty))
}

fn type_name(resolve: &Resolve, ty: TypeId) -> String {
    let name = resolve.types[ty]
        .name
        .as_ref()
        .expect("a generated type is named");
    name.to_upper_camel_case()
}

/// `ty`'s name in the WIT, `<namespace>:<package>/<interface>.<name>`.
fn wit_name(resolve: &Resolve, ty: TypeId) -> String {
    let TypeOwner::Interface(id) = resolve.types[ty].owner else {
        unimplemented!("a type of a world's own")
    };
    let interface = &resolve.interfaces[id];
    let package = &resolve.packages[interface
        .package
        .expect("an imported interface is a package's")]
    .name;
    format!(
        "{}:{}/{}.{}",
        package.namespace,
        package.name,
        interface.name.as_deref().unwrap_or_default(),
        resolve.types[ty].name.as_deref().unwrap_or_default()
    )
}

/// A field's, a flag's or a module's name, in snake case with a keyword escaped as `bindgen!`
/// escapes it, by an underscore after it.
fn field_name(name: &str) -> String {
    let name = name.to_snake_case();
    if RUST_KEYWORDS.contains(&name.as_str()) {
        format!("{name}_")
    } else {
        name
    }
}

/// Rust's keywords, strict and reserved, of the 2024 edition.
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// `docs` as Rust's doc comment, a line for each of theirs; nothing for none.
fn doc_comment(docs: &Docs) -> String {
    let Some(contents) = &docs.contents else {
        return String::new();
    };
    contents
        .lines()
        .map(|line| match line.trim_end() {
            "" => "///\n".to_owned(),
            line => format!("/// {line}\n"),
        })
        .collect()
}

/// The modules, nested by their paths, each holding its types.
fn nested_modules(modules: &BTreeMap<Vec<String>, String>) -> String {
    let mut children = BTreeMap::<&str, BTreeMap<Vec<String>, String>>::new();
    let mut out = String::new();
    for (path, body) in modules {
        match path.split_first() {
            Some((first, rest)) => {
                children
                    .entry(first)
                    .or_default()
                    .insert(rest.to_vec(), body.clone());
            }
            None => out += body,
        }
    }
    for (name, inner) in children {
        write!(out, "pub mod {name} {{\n{}}}\n", nested_modules(&inner)).unwrap();
    }
    out
}

const GENERATED: &str = "//! Generated by `cargo xtask sync-wit` from `wit/` (`xtask/src/wit_types`): nothing edits it.\n";

fn types_file(left_out: &[(LeftOut, String)], modules: &str) -> String {
    let named = |reason: LeftOut| -> String {
        left_out
            .iter()
            .filter(|(why, _)| *why == reason)
            .map(|(_, name)| format!("//! - `{name}`\n"))
            .collect()
    };
    format!(
        "//! The WIT's value types as plain Rust, with no dependency on the engine or wasmtime: each at\n\
         //! the path the engine's `bindgen!` gives it, `<namespace>::<package>::<interface>::<Name>`,\n\
         //! flags a struct of `bool`s, and the engine converts each to its own with `From`. Of the\n\
         //! interfaces the engine's worlds import, the program's and the inspector's, those whose Rust types the engine takes from another\n\
         //! crate, `wasdk:gpu`'s and WASI's, are not here.\n\
         //!\n\
         {GENERATED}\
         //!\n\
         //! Left out, as each holds a resource, a stream or a future, which a plain value cannot:\n\
         //!\n\
         {}\
         //!\n\
         //! Left out, as each holds a type of `wasdk:gpu` or WASI, which is not here:\n\
         //!\n\
         {}\n\
         {modules}",
        named(LeftOut::HoldsResource),
        named(LeftOut::HoldsTypeTakenElsewhere),
    )
}

fn conversions_file(conversions: &str) -> String {
    format!(
        "//! `From` both ways between each of the WIT's plain types, `waos_capabilities::wit`, and\n\
         //! `bindgen!`'s of the same WIT type, `crate::bindings` or, for `wasdk:inspect`,\n\
         //! `crate::inspection::bindings`: a call into the contract converts\n\
         //! at the boundary, field by field.\n\
         //!\n\
         {GENERATED}\n\
         {conversions}"
    )
}
