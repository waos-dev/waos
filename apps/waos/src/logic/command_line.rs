//! WaOS's command line, and where it shows its library, its first launch's and each later launch's
//! alike: `waos` shows the library in its launcher, `waos --launcher-closed` there once it is
//! summoned, `waos phone` in a window of a phone's size, `waos open <file>... --program <id>` opens
//! files with a program, `waos open --program <id>` opens the program, or brings it forward, as a
//! pin of it on Windows or Linux does, and `waos <file>...` opens files with WaOS itself, as Windows and Linux
//! start an app with the files it opens. Waft leaves the command line to the app, so which
//! arguments are files is decided here, and a later launch's are read by the same rules, against
//! the folder it was started in.
//!
//! The invariant: a word WaOS takes as a command (`phone`, `open`) or an option (`-…`) is never
//! read as a file; a file of such a name is given by a path that is none, such as `./phone`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use waos_package::ProgramId;

/// The argument that starts WaOS with its launcher closed, as `cargo waos run` starts it.
pub(crate) const LAUNCHER_CLOSED: &str = "--launcher-closed";

/// What WaOS's command line takes.
pub(crate) const USAGE: &str = "usage: `waos` shows WaOS's launcher, `waos --launcher-closed` once it is summoned from WaOS's icon, `waos phone` in a window of a phone's size, `waos <file>...` opens files with WaOS, such as a program's package to install, `waos open <file>... --program <id>` opens files with the program the id names, and `waos open --program <id>` opens that program";

/// Where WaOS shows its library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryPlace {
    /// In the desktop's launcher, shown from launch, as `waos` shows it.
    Launcher,
    /// In the desktop's launcher, closed until it is summoned, from WaOS's icon or by the library
    /// asking to be seen, as `waos --launcher-closed` shows it: how `cargo waos run` starts WaOS,
    /// so a development run shows only the program.
    ClosedLauncher,
    /// Laid out as on a phone: on the screen the system gives WaOS, as Android gives it its
    /// activity's, or, where it gives none, in a window of a phone's size, as `waos phone` shows it.
    Phone,
}

/// What WaOS's command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// The library, shown in `place`.
    Show(LibraryPlace),
    /// `files` opened with WaOS itself: the library, which asks whether to install each package.
    OpenWithWaos(Vec<PathBuf>),
    /// `files` opened with the program `id`, in the WaOS running on this machine, or in this one
    /// when none is.
    Open { id: ProgramId, files: Vec<PathBuf> },
    /// The installed program `id`, opened, or brought forward where it runs, in the WaOS running on
    /// this machine, or in this one when none is: what a pin of the program runs.
    ShowProgram { id: ProgramId },
}

impl Command {
    /// The same command, its files' relative paths made absolute against `current_dir`, the folder
    /// the launch was started in.
    pub(crate) fn in_folder(self, current_dir: &Path) -> Command {
        let absolute =
            |files: Vec<PathBuf>| files.iter().map(|file| current_dir.join(file)).collect();
        match self {
            Command::Show(place) => Command::Show(place),
            Command::OpenWithWaos(files) => Command::OpenWithWaos(absolute(files)),
            Command::Open { id, files } => Command::Open {
                id,
                files: absolute(files),
            },
            Command::ShowProgram { id } => Command::ShowProgram { id },
        }
    }
}

/// What the command line `args` asks for. Anything else is a mistake, and the usage says why.
pub(crate) fn command(args: impl IntoIterator<Item = OsString>) -> Result<Command, &'static str> {
    let args: Vec<OsString> = args.into_iter().collect();
    let words: Vec<Option<&str>> = args.iter().map(|arg| arg.to_str()).collect();
    match words.as_slice() {
        [] => Ok(Command::Show(LibraryPlace::Launcher)),
        [Some(LAUNCHER_CLOSED)] => Ok(Command::Show(LibraryPlace::ClosedLauncher)),
        [Some("phone")] => Ok(Command::Show(LibraryPlace::Phone)),
        [Some("open"), Some("--program"), Some(id)] => Ok(Command::ShowProgram {
            id: id.parse().map_err(|_not_an_id| USAGE)?,
        }),
        [Some("open"), _, .., Some("--program"), Some(id)] => Ok(Command::Open {
            id: id.parse().map_err(|_not_an_id| USAGE)?,
            files: paths(&args[1..args.len() - 2]),
        }),
        [first, ..] if !is_command(*first) && words.iter().all(|word| !is_option(*word)) => {
            Ok(Command::OpenWithWaos(paths(&args)))
        }
        _ => Err(USAGE),
    }
}

/// Whether `word` is one of WaOS's commands, which a file's path is never read as.
fn is_command(word: Option<&str>) -> bool {
    matches!(word, Some("phone" | "open"))
}

/// Whether `word` is an option, as no file's path given on a command line is.
fn is_option(word: Option<&str>) -> bool {
    word.is_some_and(|word| word.starts_with('-'))
}

fn paths(args: &[OsString]) -> Vec<PathBuf> {
    args.iter().map(PathBuf::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_of(args: &[&str]) -> Result<Command, &'static str> {
        command(args.iter().map(OsString::from))
    }

    #[test]
    fn the_command_line_says_where_the_library_is_shown() {
        let show = |place| Ok(Command::Show(place));
        assert_eq!(command_of(&[]), show(LibraryPlace::Launcher));
        assert_eq!(
            command_of(&["--launcher-closed"]),
            show(LibraryPlace::ClosedLauncher)
        );
        assert_eq!(command_of(&["phone"]), show(LibraryPlace::Phone));
        assert_eq!(command_of(&["phone", "please"]), Err(USAGE));
        assert_eq!(command_of(&["--tablet"]), Err(USAGE));
    }

    #[test]
    fn the_command_line_opens_files_with_the_program_an_id_names() {
        assert_eq!(
            command_of(&["open", "a.md", "b.md", "--program", "dev.example.notes"]),
            Ok(Command::Open {
                id: "dev.example.notes".parse().unwrap(),
                files: vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
            })
        );
        for wrong in [
            &["open", "a.md"][..],
            &["open", "a.md", "--program"],
            &["open", "a.md", "--program", "not an id"],
        ] {
            assert_eq!(command_of(wrong), Err(USAGE), "{wrong:?}");
        }
    }

    #[test]
    fn the_command_line_opens_the_program_an_id_names_as_its_pin_does() {
        assert_eq!(
            command_of(&["open", "--program", "dev.example.notes"]),
            Ok(Command::ShowProgram {
                id: "dev.example.notes".parse().unwrap(),
            })
        );
        assert_eq!(command_of(&["open", "--program", "not an id"]), Err(USAGE));
        assert_eq!(command_of(&["open", "--program"]), Err(USAGE));
    }

    #[test]
    fn bare_paths_are_files_opened_with_waos_and_a_later_launch_s_are_in_its_folder() {
        let opened = command_of(&["triangle.wapk", "/tmp/notes.wapk"]).unwrap();
        assert_eq!(
            opened,
            Command::OpenWithWaos(vec![
                PathBuf::from("triangle.wapk"),
                PathBuf::from("/tmp/notes.wapk"),
            ])
        );
        assert_eq!(
            opened.in_folder(Path::new("/home/me")),
            Command::OpenWithWaos(vec![
                PathBuf::from("/home/me/triangle.wapk"),
                PathBuf::from("/tmp/notes.wapk"),
            ])
        );
        assert_eq!(
            command_of(&["./phone"]),
            Ok(Command::OpenWithWaos(vec![PathBuf::from("./phone")])),
            "a file named as a command, by a path that is none"
        );
        assert_eq!(command_of(&["a.wapk", "--launcher-closed"]), Err(USAGE));
    }
}
