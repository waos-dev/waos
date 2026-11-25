//! The kinds of file a program opens, on Windows: registry settings under the user's classes
//! (`HKEY_CURRENT_USER\Software\Classes`), as an installer registers an app's file associations.
//! Each kind is a ProgID of the program's own, `<app id>.<first extension>`, named what Explorer
//! calls such a file, with the program's icon, and opening a file by starting
//! `waos.exe open "<file>" --program <id>`; and each of its extensions lists that ProgID among
//! the apps that open it (`OpenWithProgids`), so Explorer offers the program in Open with.
//!
//! The invariant a later editor will break: an extension's own key is shared with every app that
//! opens it, so the program sets only its value in `OpenWithProgids`, never the extension's
//! default, which says the app a double-click opens it with, the user's to choose; and every
//! setting is named in what this answers, so all of it is taken back.

use std::path::Path;

use waos_package::FileAssociation;

use super::{ProgramEntry, RegistrySetting};
use crate::logic::program_identities::{Identities, windows_arguments, windows_command_line};

/// Where an app's classes are, under `HKEY_CURRENT_USER`.
const CLASSES: &str = r"Software\Classes";

/// One registry value to set under `HKEY_CURRENT_USER`: its key, its name, none for the key's
/// default value, and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegistryValue {
    pub(crate) key: String,
    pub(crate) name: Option<String>,
    pub(crate) data: String,
}

/// What registering `entry`'s kinds of file sets, the program's `.ico` at `icon` where it has one,
/// opening a file by starting the WaOS `identities` names; and the settings that take it back.
pub(crate) fn registry_of(
    entry: &ProgramEntry,
    identities: &Identities,
    icon: Option<&Path>,
) -> (Vec<RegistryValue>, Vec<RegistrySetting>) {
    let mut values = Vec::new();
    let mut settings = Vec::new();
    for association in &entry.file_associations {
        let prog_id = prog_id_of(entry, association);
        let class = format!(r"{CLASSES}\{prog_id}");
        values.push(default_value(&class, &association.name));
        if let Some(icon) = icon {
            let icon = format!("{},0", icon.to_string_lossy());
            values.push(default_value(&format!(r"{class}\DefaultIcon"), &icon));
        }
        let command = open_command(entry, identities);
        values.push(default_value(
            &format!(r"{class}\shell\open\command"),
            &command,
        ));
        settings.push(RegistrySetting::Key(class));
        for extension in &association.extensions {
            let key = format!(r"{CLASSES}\.{extension}\OpenWithProgids");
            values.push(RegistryValue {
                key: key.clone(),
                name: Some(prog_id.clone()),
                data: String::new(),
            });
            settings.push(RegistrySetting::Value {
                key,
                name: prog_id.clone(),
            });
        }
    }
    (values, settings)
}

/// The ProgID of `association`, one of `entry`'s kinds of file: the program's identity and the
/// kind's first extension, which no other program's ProgID shares.
fn prog_id_of(entry: &ProgramEntry, association: &FileAssociation) -> String {
    format!("{}.{}", entry.app_id, association.extensions[0])
}

/// What opening a file of one of `entry`'s kinds starts: WaOS, opening the file Explorer puts in
/// place of `%1` with the program.
fn open_command(entry: &ProgramEntry, identities: &Identities) -> String {
    let waos_open = windows_command_line(&identities.waos, &["open".to_owned()]);
    let program = windows_arguments(&["--program".to_owned(), entry.id.to_string()]);
    format!("{waos_open} \"%1\" {program}")
}

fn default_value(key: &str, data: &str) -> RegistryValue {
    RegistryValue {
        key: key.to_owned(),
        name: None,
        data: data.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn ruffle(file_associations: Vec<FileAssociation>) -> ProgramEntry {
        ProgramEntry {
            id: "dev.example.ruffle".parse().unwrap(),
            app_id: "dev.waos.app.dev.example.ruffle".to_owned(),
            name: "Ruffle".to_owned(),
            version: "1.0.0".to_owned(),
            icon: None,
            category: None,
            file_associations,
        }
    }

    fn flash() -> FileAssociation {
        FileAssociation {
            extensions: vec!["swf".to_owned(), "spl".to_owned()],
            name: "Flash movie".to_owned(),
            mime_type: Some("application/x-shockwave-flash".to_owned()),
        }
    }

    fn identities() -> Identities {
        Identities {
            waos: PathBuf::from(r"C:\Program Files\WaOS\waos.exe"),
            icons: PathBuf::from(r"C:\Users\me\AppData\Local\WaOS\icons"),
        }
    }

    #[test]
    fn a_kind_of_file_is_a_progid_of_the_programs_own_listed_by_each_extension() {
        let ico = PathBuf::from(r"C:\Users\me\AppData\Local\WaOS\icons\ruffle.ico");
        let (values, settings) = registry_of(&ruffle(vec![flash()]), &identities(), Some(&ico));
        let class = r"Software\Classes\dev.waos.app.dev.example.ruffle.swf";
        let value = |key: &str, name: Option<&str>, data: &str| RegistryValue {
            key: key.to_owned(),
            name: name.map(str::to_owned),
            data: data.to_owned(),
        };
        assert_eq!(
            values,
            [
                value(class, None, "Flash movie"),
                value(
                    &format!(r"{class}\DefaultIcon"),
                    None,
                    r"C:\Users\me\AppData\Local\WaOS\icons\ruffle.ico,0"
                ),
                value(
                    &format!(r"{class}\shell\open\command"),
                    None,
                    r#""C:\Program Files\WaOS\waos.exe" open "%1" --program dev.example.ruffle"#
                ),
                value(
                    r"Software\Classes\.swf\OpenWithProgids",
                    Some("dev.waos.app.dev.example.ruffle.swf"),
                    ""
                ),
                value(
                    r"Software\Classes\.spl\OpenWithProgids",
                    Some("dev.waos.app.dev.example.ruffle.swf"),
                    ""
                ),
            ]
        );
        assert_eq!(
            settings,
            [
                RegistrySetting::Key(class.to_owned()),
                RegistrySetting::Value {
                    key: r"Software\Classes\.swf\OpenWithProgids".to_owned(),
                    name: "dev.waos.app.dev.example.ruffle.swf".to_owned(),
                },
                RegistrySetting::Value {
                    key: r"Software\Classes\.spl\OpenWithProgids".to_owned(),
                    name: "dev.waos.app.dev.example.ruffle.swf".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn no_extensions_default_is_set_and_a_program_that_opens_nothing_sets_nothing() {
        let (values, _) = registry_of(&ruffle(vec![flash()]), &identities(), None);
        assert!(
            !values
                .iter()
                .any(|value| value.key == r"Software\Classes\.swf" && value.name.is_none()),
            "{values:?}"
        );
        assert_eq!(
            registry_of(&ruffle(Vec::new()), &identities(), None),
            (Vec::new(), Vec::new())
        );
    }
}
