//! A program's identity on the systems that tell a window's app by the window rather than by its
//! process, Windows and Linux: `dev.waos.app.<program id>`, its windows' AppUserModelID on Windows
//! and their app id on Linux, which names its Start menu shortcut's ID and its desktop entry; and
//! what a pin of its window starts, `waos open --program <id>`, with its name and its icon. Only an
//! installed program has one: a development run shares the installed program's, and a program never
//! installed has none, its windows WaOS's.
//!
//! The invariant: the identity is the same string on both systems and in every place it is written
//! (a window, a shortcut, a desktop entry, a notification), and never longer than an
//! AppUserModelID may be; a program whose id would make it longer has none.

use std::path::{Path, PathBuf};

use waos_package::ProgramId;
use waos_program_window::ProgramIdentity;
use waos_services::packages::PackageManager;

/// What every program's identity starts with: WaOS's own identifier and a dot, as Flatpak
/// requires of the ids an app's entries take, so the programs are WaOS's apps to the system.
const PREFIX: &str = "dev.waos.app.";

/// The longest an AppUserModelID may be.
const LONGEST: usize = 128;

/// The identity of the program `id`; none where it would be longer than an AppUserModelID may be.
pub(crate) fn app_id_of(id: &ProgramId) -> Option<String> {
    let app_id = format!("{PREFIX}{id}");
    (app_id.len() <= LONGEST).then_some(app_id)
}

/// The identity of the program that runs as `runs_as`, where it is installed in `packages`; none for
/// a program never installed, whose windows and notifications are WaOS's.
pub(crate) fn installed_app_id(
    packages: &PackageManager,
    runs_as: &str,
) -> Option<(ProgramId, String)> {
    let id: ProgramId = runs_as.parse().ok()?;
    packages.installed_program(&id)?;
    let app_id = app_id_of(&id)?;
    Some((id, app_id))
}

/// The arguments with which WaOS opens the program `id`, or brings it forward, as its pin does.
pub(crate) fn relaunch_arguments(id: &ProgramId) -> [String; 3] {
    ["open".to_owned(), "--program".to_owned(), id.to_string()]
}

/// What the identities of this WaOS's programs are made of: the WaOS a pin starts, and where each
/// program's icon is kept as the system's lists of apps take it.
#[derive(Clone, Debug)]
pub(crate) struct Identities {
    /// The WaOS a pin of a program's window, its shortcut or its desktop entry starts.
    pub(crate) waos: PathBuf,
    /// Where each program's icon is kept for the system: its `.ico` on Windows, its PNG on Linux.
    pub(crate) icons: PathBuf,
}

impl Identities {
    /// The identities of the programs this WaOS runs, their icons in `icons`, on a system whose
    /// windows are apps of their own; none elsewhere, or where WaOS cannot say where it is.
    pub(crate) fn of_this_waos(icons: PathBuf) -> Option<Identities> {
        if !cfg!(any(windows, target_os = "linux")) {
            return None;
        }
        match waos_executable() {
            Ok(waos) => Some(Identities { waos, icons }),
            Err(error) => {
                tracing::warn!(%error, "WaOS cannot say where it is, so its programs have no identities");
                None
            }
        }
    }

    /// The identity `app_id` of the installed program `id`, which its user knows as `name`: on
    /// Windows with what a pin of its window starts, with its icon once it is written.
    pub(crate) fn identity_of(
        &self,
        id: &ProgramId,
        app_id: String,
        name: &str,
    ) -> ProgramIdentity {
        let relaunch = cfg!(windows).then(|| self.relaunch_of(id, name, &app_id));
        ProgramIdentity { app_id, relaunch }
    }

    /// The `.ico` of the program whose identity is `app_id`.
    pub(crate) fn ico_of(&self, app_id: &str) -> PathBuf {
        self.icons.join(format!("{app_id}.ico"))
    }

    /// The PNG of the program whose identity is `app_id`.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn png_of(&self, app_id: &str) -> PathBuf {
        self.icons.join(format!("{app_id}.png"))
    }

    /// What a pin of a window of the program `id` starts on Windows, named `name`, with its icon
    /// where it is written: an icon Windows cannot load drops the whole relaunch.
    fn relaunch_of(&self, id: &ProgramId, name: &str, app_id: &str) -> waft::WindowsRelaunch {
        let ico = self.ico_of(app_id);
        waft::WindowsRelaunch {
            command: windows_command_line(&self.waos, &relaunch_arguments(id)),
            display_name: name.to_owned(),
            icon: ico.is_file().then_some(waft::WindowsIconResource {
                path: ico,
                index: 0,
            }),
        }
    }
}

/// The command line that runs `program` with `arguments`, each quoted where it has to be, as
/// Windows' `CommandLineToArgvW` reads one back.
pub(crate) fn windows_command_line(program: &Path, arguments: &[String]) -> String {
    let program = windows_argument(&program.to_string_lossy());
    if arguments.is_empty() {
        return program;
    }
    format!("{program} {}", windows_arguments(arguments))
}

/// `arguments` as a command line holds them after its program, as a shortcut's arguments are.
pub(crate) fn windows_arguments(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| windows_argument(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `argument` as a command line holds it: bare where it has no space, tab or quote, else quoted,
/// the backslashes before a quote, its own or the closing one, doubled.
fn windows_argument(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => {
                backslashes += 1;
                continue;
            }
            '"' => quoted.push_str(&"\\".repeat(backslashes * 2 + 1)),
            _ => quoted.push_str(&"\\".repeat(backslashes)),
        }
        quoted.push(character);
        backslashes = 0;
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Where this WaOS is, which a pin starts: the AppImage it runs from on Linux, as the AppImage's
/// runtime names it, since the executable inside is mounted only while it runs; its executable
/// otherwise, through any link it was started by.
fn waos_executable() -> std::io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(app_image) if cfg!(target_os = "linux") && !app_image.is_empty() => {
            Ok(PathBuf::from(app_image))
        }
        _ => std::env::current_exe().and_then(std::fs::canonicalize),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> ProgramId {
        text.parse().unwrap()
    }

    #[test]
    fn a_programs_identity_is_waos_s_identifier_and_its_id() {
        assert_eq!(
            app_id_of(&id("dev.example.notes")).as_deref(),
            Some("dev.waos.app.dev.example.notes")
        );
    }

    #[test]
    fn an_id_too_long_for_an_app_user_model_id_has_no_identity() {
        let longest = format!("dev.{}", "a".repeat(LONGEST - PREFIX.len() - 4));
        assert_eq!(
            app_id_of(&id(&longest)).map(|app_id| app_id.len()),
            Some(LONGEST)
        );
        assert_eq!(app_id_of(&id(&format!("{longest}b"))), None);
    }

    #[test]
    fn a_pin_runs_waos_open_with_the_program() {
        let identities = Identities {
            waos: PathBuf::from(r"C:\Program Files\WaOS\waos.exe"),
            icons: PathBuf::from(r"C:\Users\me\AppData\Local\WaOS\icons"),
        };
        let relaunch = identities.relaunch_of(
            &id("dev.example.notes"),
            "Notes",
            "dev.waos.app.dev.example.notes",
        );
        assert_eq!(
            relaunch.command,
            r#""C:\Program Files\WaOS\waos.exe" open --program dev.example.notes"#
        );
        assert_eq!(relaunch.display_name, "Notes");
        assert_eq!(relaunch.icon, None, "no icon written yet, so none named");
    }

    #[test]
    fn a_relaunch_names_the_programs_icon_once_it_is_written() {
        let folder = tempfile::tempdir().unwrap();
        let identities = Identities {
            waos: PathBuf::from("waos.exe"),
            icons: folder.path().to_owned(),
        };
        let ico = identities.ico_of("dev.waos.app.dev.example.notes");
        std::fs::write(&ico, b"an icon").unwrap();
        let relaunch = identities.relaunch_of(
            &id("dev.example.notes"),
            "Notes",
            "dev.waos.app.dev.example.notes",
        );
        assert_eq!(
            relaunch.icon,
            Some(waft::WindowsIconResource {
                path: ico,
                index: 0
            })
        );
    }

    #[test]
    fn arguments_are_quoted_as_windows_reads_them_back() {
        let line = |arguments: &[&str]| {
            let arguments: Vec<String> = arguments.iter().map(|&a| a.to_owned()).collect();
            windows_command_line(Path::new("waos.exe"), &arguments)
        };
        assert_eq!(
            line(&["open", "--program", "a.b"]),
            "waos.exe open --program a.b"
        );
        assert_eq!(line(&[""]), r#"waos.exe """#);
        assert_eq!(line(&[r"C:\a b\"]), r#"waos.exe "C:\a b\\""#);
        assert_eq!(line(&[r#"say "hi""#]), r#"waos.exe "say \"hi\"""#);
        assert_eq!(line(&[r#"a\"b c"#]), r#"waos.exe "a\\\"b c""#);
    }
}
