//! The reference's pages for each interface, made from the WIT as the website makes its own: for
//! each package, a page listing its interfaces, a `meta.json` ordering them, and a page for each
//! interface, its doc comment and then its WIT as written.
//!
//! The invariant: a page is named as the website names it, `docs/reference/<package>/<interface>`
//! with the package's folder less its version (`wasdk-platform` for `wasdk-platform-1.0.0`), so a
//! link to the reference in a written page is the name of a made one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One package of the WIT: its id, what its doc comment says, and its interfaces.
struct Package {
    id: String,
    docs: String,
    interfaces: Vec<Interface>,
}

/// One interface: its name, its doc comment, and its WIT from `interface` to its closing brace.
struct Interface {
    name: String,
    docs: String,
    wit: String,
}

/// Writes the pages of every package under `wit` into `out`, and answers each by its path in the
/// docs and where it was written.
pub fn write_pages(wit: &Path, out: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut written = Vec::new();
    for folder in sorted_entries(wit)? {
        let slug = slug_of(&folder);
        let package = read_package(&folder)?;
        let folder_out = out.join(&slug);
        fs::create_dir_all(&folder_out)?;
        let mut pages = vec![
            ("meta.json".to_owned(), meta(&package)),
            ("index.md".to_owned(), package_page(&package, &slug)),
        ];
        for interface in &package.interfaces {
            pages.push((
                format!("{}.md", interface.name),
                interface_page(&package, interface),
            ));
        }
        for (file, text) in pages {
            let path = folder_out.join(&file);
            fs::write(&path, text)?;
            written.push((format!("docs/reference/{slug}/{file}"), path));
        }
    }
    Ok(written)
}

/// The package in `folder`, its interfaces in the order of their names.
fn read_package(folder: &Path) -> io::Result<Package> {
    let mut package = Package {
        id: String::new(),
        docs: String::new(),
        interfaces: Vec::new(),
    };
    for file in sorted_entries(folder)? {
        if file.extension().is_some_and(|extension| extension == "wit") {
            read_file(&fs::read_to_string(&file)?, &mut package);
        }
    }
    package
        .interfaces
        .sort_by(|one, other| one.name.cmp(&other.name));
    Ok(package)
}

/// Adds what `wit` declares to `package`: its id and doc comment where it has them, and each of
/// its interfaces.
fn read_file(wit: &str, package: &mut Package) {
    let lines: Vec<&str> = wit.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if let Some(id) = line.strip_prefix("package ") {
            package.id = id.trim_end_matches(';').trim().to_owned();
            if package.docs.is_empty() {
                package.docs = doc_comment(&lines[..index]);
            }
        } else if let Some(rest) = line.strip_prefix("interface ") {
            let name = rest.trim_end_matches('{').trim().to_owned();
            let end = lines[index..]
                .iter()
                .position(|line| *line == "}")
                .map_or(lines.len(), |offset| index + offset + 1);
            package.interfaces.push(Interface {
                name,
                docs: doc_comment(&lines[..index]),
                wit: lines[index..end].join("\n"),
            });
        }
    }
}

/// The `///` comment just above what follows `above`, past its `@` gates, without its slashes.
fn doc_comment(above: &[&str]) -> String {
    let mut docs: Vec<&str> = above
        .iter()
        .rev()
        .skip_while(|line| line.trim_start().starts_with('@'))
        .map_while(|line| line.trim_start().strip_prefix("///"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect();
    docs.reverse();
    docs.join("\n")
}

fn meta(package: &Package) -> String {
    let pages: Vec<String> = package
        .interfaces
        .iter()
        .map(|interface| format!("{:?}", interface.name))
        .collect();
    format!(
        "{{\"title\": {:?}, \"pages\": [{}]}}\n",
        name_of(&package.id),
        pages.join(", ")
    )
}

/// The package's page: its doc comment, then its interfaces, each with its first sentence.
fn package_page(package: &Package, slug: &str) -> String {
    let list: Vec<String> = package
        .interfaces
        .iter()
        .map(|interface| {
            let summary = first_sentence(&interface.docs);
            let link = format!("- [`{0}`](/docs/reference/{slug}/{0})", interface.name);
            if summary.is_empty() {
                link
            } else {
                format!("{link}: {summary}")
            }
        })
        .collect();
    page(
        name_of(&package.id),
        &package.id,
        &[&package.docs, &list.join("\n")],
    )
}

/// An interface's page: its doc comment, then its WIT as written.
fn interface_page(package: &Package, interface: &Interface) -> String {
    let (name, version) = package.id.split_once('@').unwrap_or((&package.id, ""));
    let id = format!("{name}/{}@{version}", interface.name);
    let wit = format!("```wit\n{}\n```", interface.wit);
    page(&interface.name, &id, &[&interface.docs, &wit])
}

fn page(title: &str, description: &str, sections: &[&str]) -> String {
    let body: Vec<&str> = sections
        .iter()
        .copied()
        .filter(|section| !section.is_empty())
        .collect();
    format!(
        "---\ntitle: {title:?}\ndescription: {description:?}\n---\n\n{}\n",
        body.join("\n\n")
    )
}

/// The first sentence of `docs`, on one line.
fn first_sentence(docs: &str) -> String {
    let flat = docs.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.find(". ") {
        Some(end) => flat[..=end].to_owned(),
        None => flat,
    }
}

/// `wasdk:platform` of `wasdk:platform@1.0.0`.
fn name_of(id: &str) -> &str {
    id.split_once('@').map_or(id, |(name, _)| name)
}

/// `wasdk-platform` of the folder `wasdk-platform-1.0.0`.
fn slug_of(folder: &Path) -> String {
    let name = folder.file_name().unwrap_or_default().to_string_lossy();
    match name.rsplit_once('-') {
        Some((slug, version)) if version.starts_with(|c: char| c.is_ascii_digit()) => {
            slug.to_owned()
        }
        _ => name.into_owned(),
    }
}

fn sorted_entries(folder: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}
