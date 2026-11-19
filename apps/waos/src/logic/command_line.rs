//! WaOS's command line, and where it shows its library, its first launch's and each later launch's
//! alike: `waos` shows the library in its launcher, `waos --launcher-closed` there once it is
//! summoned, `waos phone` in a window of a phone's size, `waos open <file>... --program <id>` opens
//! files with a program, `waos open --program <id>` opens the program, or brings it forward, as a
//! pin of it on Windows or Linux does, `waos --headless [--idle-timeout <secs>]` runs WaOS without
//! a windowing system, developer CLI commands (`new`, `build`, `run`, `package`, `ui`, `settings`,
//! `inspect`, `docs`) are dispatched to `waos-cli`, and `waos <file>...` opens files with WaOS
//! itself, as Windows and Linux start an app with the files it opens.
//!
//! The invariant: a word WaOS takes as a command (`phone`, `open`, or any developer CLI command)
//! or an option (`-…`) is never read as a file; a file of such a name is given by a path that is
//! none, such as `./phone` or `./run`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::time::Duration;

use waos_package::ProgramId;

/// The argument that starts WaOS with its launcher closed, as `waos run` starts it.
pub(crate) const LAUNCHER_CLOSED: &str = "--launcher-closed";

/// What WaOS's command line takes.
pub(crate) const USAGE: &str = "usage: `waos` shows WaOS's launcher, `waos --launcher-closed` once it is summoned from WaOS's icon, `waos --headless [--idle-timeout <secs>]` runs the headless daemon, `waos phone` in a window of a phone's size, `waos <file>...` opens files with WaOS, such as a program's package to install, `waos open <file>... --program <id>` opens files with the program the id names, `waos open --program <id>` opens that program, and `waos --help` lists developer CLI commands";

/// Reserved command words that are never interpreted as bare file paths.
const RESERVED_COMMANDS: &[&str] = &[
    "phone",
    "open",
    // Visible developer CLI commands
    "new",
    "build",
    "run",
    "package",
    "ui",
    "settings",
    "inspect",
    "docs",
    "help",
    // Hidden 0.3 transition aliases
    "screenshot",
    "resize",
    "tap",
    "press",
    "drag",
    "type",
    "key",
    "scroll",
    "set",
    "skill",
];

/// Where WaOS shows its library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryPlace {
    /// In the desktop's launcher, shown from launch, as `waos` shows it.
    Launcher,
    /// In the desktop's launcher, closed until it is summoned, from WaOS's icon or by the library
    /// asking to be seen, as `waos --launcher-closed` shows it: how `waos run` starts WaOS,
    /// so a development run shows only the program.
    ClosedLauncher,
    /// Laid out as on a phone: on the screen the system gives WaOS, as Android gives it its
    /// activity's, or, where it gives none, in a window of a phone's size, as `waos phone` shows it.
    Phone,
}

/// What WaOS's desktop/GUI command line asks for (first launch or forwarded later launch).
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

/// Top-level classification of a desktop launch's full `argv` (including `argv[0]`), decided
/// before `claim_the_one_instance()` and `winit`.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Launch {
    /// Hand off directly to `waos-cli` without claiming single-instance or starting `winit`.
    Cli(Vec<OsString>),
    /// Run the headless daemon without `winit` or single-instance.
    Headless { idle_timeout: Option<Duration> },
    /// Run the normal desktop app or `waos open` flow.
    App(Command),
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

/// Classifies a full process `argv` (including `argv[0]`) into CLI dispatch, headless daemon, or
/// desktop app launch.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn classify_launch(
    argv: impl IntoIterator<Item = OsString>,
) -> Result<Launch, &'static str> {
    let argv: Vec<OsString> = argv.into_iter().collect();
    if argv
        .first()
        .is_some_and(|argv0| waos_cli::is_cargo_subcommand_invocation(argv0))
    {
        return Ok(Launch::Cli(argv));
    }
    let args = argv.get(1..).unwrap_or(&[]);
    let words: Vec<Option<&str>> = args.iter().map(|arg| arg.to_str()).collect();

    if let Some(headless) = parse_headless(&words)? {
        return Ok(headless);
    }

    if let Some(Some(first)) = words.first()
        && (waos_cli::is_cli_command(first)
            || matches!(*first, "-h" | "--help" | "-V" | "--version"))
    {
        return Ok(Launch::Cli(argv));
    }

    command(args.iter().cloned()).map(Launch::App)
}

/// Parses `--headless [--idle-timeout <secs>]` (in either flag order). Returns `Ok(None)` when
/// `--headless` is not present.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn parse_headless(words: &[Option<&str>]) -> Result<Option<Launch>, &'static str> {
    if !words.contains(&Some("--headless")) {
        return Ok(None);
    }
    let mut idle_timeout: Option<Duration> = None;
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            Some("--headless") => i += 1,
            Some("--idle-timeout") => {
                let Some(Some(secs_str)) = words.get(i + 1) else {
                    return Err(USAGE);
                };
                let secs: u64 = secs_str.parse().map_err(|_| USAGE)?;
                idle_timeout = (secs > 0).then(|| Duration::from_secs(secs));
                i += 2;
            }
            Some(flag) if flag.starts_with("--idle-timeout=") => {
                let secs_str = &flag["--idle-timeout=".len()..];
                let secs: u64 = secs_str.parse().map_err(|_| USAGE)?;
                idle_timeout = (secs > 0).then(|| Duration::from_secs(secs));
                i += 1;
            }
            _ => return Err(USAGE),
        }
    }
    Ok(Some(Launch::Headless { idle_timeout }))
}

/// What the command line `args` asks for (excluding `argv[0]`). Anything else is a mistake, and
/// the usage says why.
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

/// Whether `word` is one of WaOS's reserved commands, which a file's path is never read as.
fn is_command(word: Option<&str>) -> bool {
    word.is_some_and(|word| RESERVED_COMMANDS.contains(&word))
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

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn launch_of(argv: &[&str]) -> Result<Launch, &'static str> {
        classify_launch(argv.iter().map(OsString::from))
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
        assert_eq!(
            command_of(&["./run"]),
            Ok(Command::OpenWithWaos(vec![PathBuf::from("./run")])),
        );
        assert_eq!(command_of(&["run"]), Err(USAGE));
        assert_eq!(command_of(&["ui"]), Err(USAGE));
        assert_eq!(command_of(&["screenshot"]), Err(USAGE));
        assert_eq!(command_of(&["a.wapk", "--launcher-closed"]), Err(USAGE));
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn classify_launch_routes_cli_headless_cargo_waos_and_gui_correctly() {
        // GUI & open preserved
        assert_eq!(
            launch_of(&["waos"]),
            Ok(Launch::App(Command::Show(LibraryPlace::Launcher)))
        );
        assert_eq!(
            launch_of(&["waos", "--launcher-closed"]),
            Ok(Launch::App(Command::Show(LibraryPlace::ClosedLauncher)))
        );
        assert_eq!(
            launch_of(&["waos", "phone"]),
            Ok(Launch::App(Command::Show(LibraryPlace::Phone)))
        );
        assert_eq!(
            launch_of(&["waos", "open", "a.md", "--program", "dev.example.notes"]),
            Ok(Launch::App(Command::Open {
                id: "dev.example.notes".parse().unwrap(),
                files: vec![PathBuf::from("a.md")],
            }))
        );
        assert_eq!(
            launch_of(&["waos", "triangle.wapk"]),
            Ok(Launch::App(Command::OpenWithWaos(vec![PathBuf::from(
                "triangle.wapk"
            )])))
        );
        assert_eq!(
            launch_of(&["waos", "waos.wapk"]),
            Ok(Launch::App(Command::OpenWithWaos(vec![PathBuf::from(
                "waos.wapk"
            )])))
        );

        // Headless daemon
        assert_eq!(
            launch_of(&["waos", "--headless"]),
            Ok(Launch::Headless { idle_timeout: None })
        );
        assert_eq!(
            launch_of(&["waos", "--headless", "--idle-timeout", "300"]),
            Ok(Launch::Headless {
                idle_timeout: Some(Duration::from_secs(300))
            })
        );
        assert_eq!(
            launch_of(&["waos", "--headless", "--idle-timeout=0"]),
            Ok(Launch::Headless { idle_timeout: None })
        );
        assert_eq!(
            launch_of(&["waos", "--idle-timeout", "60", "--headless"]),
            Ok(Launch::Headless {
                idle_timeout: Some(Duration::from_secs(60))
            })
        );
        assert_eq!(
            launch_of(&["waos", "--headless", "--idle-timeout", "not-a-number"]),
            Err(USAGE)
        );
        assert_eq!(launch_of(&["waos", "--headless", "--bogus"]), Err(USAGE));

        // Developer CLI commands (domain + hidden aliases + help/version)
        for cmd in [
            &["waos", "new", "hello"][..],
            &["waos", "build", "--release"],
            &["waos", "run", "-p", "notes"],
            &["waos", "package"],
            &["waos", "ui", "screenshot"],
            &["waos", "settings", "set", "camera", "allow"],
            &["waos", "inspect", "summary"],
            &["waos", "docs", "skill", "install"],
            &["waos", "screenshot"],
            &["waos", "resize", "--to", "390x844"],
            &["waos", "set", "camera", "allow"],
            &["waos", "skill", "install"],
            &["waos", "--help"],
            &["waos", "-V"],
        ] {
            assert!(
                matches!(launch_of(cmd), Ok(Launch::Cli(_))),
                "expected Cli for {cmd:?}, got {:?}",
                launch_of(cmd)
            );
        }

        // cargo-waos symlink always routes to CLI (even with no args)
        for cargo_argv in [
            &["cargo-waos"][..],
            &["cargo-waos", "waos", "run"],
            &[
                "/usr/local/bin/cargo-waos",
                "waos",
                "ui",
                "tap",
                "--at",
                "1,2",
            ],
        ] {
            assert!(
                matches!(launch_of(cargo_argv), Ok(Launch::Cli(_))),
                "expected Cli for {cargo_argv:?}"
            );
        }
    }
}
