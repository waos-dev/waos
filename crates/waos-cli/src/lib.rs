//! `waos` / `cargo waos`: writes a new program, builds a program as a component, runs it in the
//! WaOS on this machine, and packages it as a signed `.wapk` for a package source; while it runs
//! there, takes screenshots of its windows, gives them input and sizes them (`ui`), reads and sets
//! its settings (`settings`), inspects what it does (`inspect`), and reads or installs WaOS's docs
//! (`docs`).
//!
//! This crate is engine-free and builds on stable Rust. Both the standalone `cargo-waos` binary
//! and the unified `waos` app binary call into [`run_from_args`] with their own [`WaOSStarter`].
//!
//! The invariant: every command is about a program and the WaOS it runs in. A run goes to the
//! WaOS already running, starting one through the caller's [`WaOSStarter`] when none answers,
//! never to an engine of this crate's own; a program's settings go to the WaOS keeping them,
//! started the same way; a screenshot, resize, input, or live inspection goes to the WaOS running
//! the program, never starting one.

mod asked;
mod asking;
mod build;
mod connecting;
// Linux's alone; its tests run everywhere.
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod desktop_entries;
mod docs;
mod icon_rule;
mod input;
mod inspect;
mod metadata;
mod new;
mod os;
mod package;
mod resize;
mod run;
mod screenshot;
mod settings;
mod signing_key;
mod skill;
mod variables;
pub mod waos_app;
mod workspace;

use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use waos_dev_protocol::{Input, Inspection, WindowId};
use waos_package::ProgramId;

use crate::build::{BuiltProgram, Profile};
use crate::input::Pair;
use crate::metadata::WaosMetadata;
use crate::new::Template;
use crate::resize::Points;

pub use crate::connecting::{DefaultStarter, WaOSStarter};

/// Top-level developer CLI subcommands.
pub const CLI_COMMANDS: &[&str] = &[
    "new", "build", "run", "package", "ui", "settings", "inspect", "docs", "help",
];

/// Whether `word` is a developer CLI subcommand.
pub fn is_cli_command(word: &str) -> bool {
    CLI_COMMANDS.contains(&word)
}

/// Whether `argv0`'s basename is `cargo-waos` (or `cargo-waos.exe`), meaning cargo invoked this
/// binary as the `cargo waos` subcommand.
pub fn is_cargo_subcommand_invocation(argv0: &OsStr) -> bool {
    let Some(name) = Path::new(argv0).file_name().and_then(OsStr::to_str) else {
        return false;
    };
    name == "cargo-waos" || name.eq_ignore_ascii_case("cargo-waos.exe")
}

/// Normalizes process `argv` for CLI parsing: strips a leading `waos` argument **only** when
/// `argv[0]`'s basename is `cargo-waos` (so a `cargo-waos` symlink works for `cargo waos ...`
/// without affecting `waos <file>`).
pub fn normalize_argv(argv: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args: Vec<OsString> = argv.into_iter().collect();
    if args.len() >= 2 && is_cargo_subcommand_invocation(&args[0]) && args[1] == OsStr::new("waos")
    {
        args.remove(1);
    }
    args
}

/// Runs the developer CLI using `std::env::args_os()` and `starter`.
pub fn run_from_env(starter: &dyn WaOSStarter) -> ExitCode {
    run_from_args(std::env::args_os(), starter)
}

/// Runs the developer CLI using the given full `argv` (including `argv[0]`) and `starter`.
pub fn run_from_args(
    argv: impl IntoIterator<Item = OsString>,
    starter: &dyn WaOSStarter,
) -> ExitCode {
    let raw: Vec<OsString> = argv.into_iter().collect();
    let is_cargo = raw
        .first()
        .is_some_and(|argv0| is_cargo_subcommand_invocation(argv0));
    let bin_name = if is_cargo { "cargo waos" } else { "waos" };
    let normalized = normalize_argv(raw);

    let matches = match build_clap_command(bin_name).try_get_matches_from(&normalized) {
        Ok(matches) => matches,
        Err(error) => {
            let _ = error.print();
            return if error.exit_code() == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(error.exit_code() as u8)
            };
        }
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::FAILURE;
        }
    };
    dispatch(cli.command, bin_name, starter).unwrap_or_else(|error| {
        eprintln!("{bin_name}: {error:#}");
        ExitCode::FAILURE
    })
}

fn build_clap_command(bin_name: &str) -> clap::Command {
    let mut cmd = Cli::command().bin_name(bin_name.to_owned());
    if bin_name == "waos" {
        cmd = cmd.after_help(
            "App commands: `waos` opens the desktop launcher, `waos --headless [--idle-timeout <SECS>]` runs the headless daemon, `waos phone` opens in a phone-sized window, `waos open <FILE>... --program <ID>` opens files with a program, and `waos <FILE>...` opens packages or files with WaOS.
The docs and guides are at https://waos.app/docs; `waos docs` prints this version's.",
        );
    }
    cmd
}

#[derive(Parser)]
#[command(
    name = "waos",
    about = "waos: writes a new program, builds a program as a component, runs it in WaOS, packages it, drives its UI, manages its settings, inspects its activity, and reads WaOS's docs. `waos docs` (`cargo waos docs`) is the map of WaOS's docs for writing an app.",
    after_help = "The docs and guides are at https://waos.app/docs; `cargo waos docs` prints the app's version's.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Writes a new program in a folder of its name: a Waft app that `waos run` runs and
    /// `waos package` packages as it is.
    ///
    /// Inside a cargo workspace, the program is made a member of it as `cargo new` makes one: it
    /// inherits the workspace's package keys, lints and the dependencies the workspace lists, and
    /// is added to the workspace's members unless a pattern there takes it in already.
    New(NewArgs),
    /// Builds the program as a component, for wasm32-wasip3, and puts it at
    /// target/waos/<profile>/<package>.wasm.
    Build(BuildArgs),
    /// Builds the program and runs it in the WaOS on this machine, starting WaOS when none
    /// answers, as the program the id in its [package.metadata.waos] names, with that program's
    /// settings and folders. What it writes comes here as it writes it, each line after the
    /// program's name; Ctrl-C stops it; the exit status is 0 for a run that returned success and
    /// 1 otherwise.
    ///
    /// Each --env NAME=VALUE adds a variable the program reads in this run alone, and nothing of
    /// this shell's environment is passed. Installed programs never get them, and TMPDIR, which
    /// WaOS sets, wins over a variable of that name given here. Arguments after -- reach the
    /// program as its own arguments, as `cargo run --` passes them.
    ///
    /// A question WaOS asks while the program runs, to allow something or to pick a file, has a
    /// row among the program's settings: `waos settings` lists them, and `waos settings set`
    /// answers one ahead, so the run does not wait for a person.
    Run(RunArgs),
    /// Builds the program's release component and writes it as a package signed with your
    /// signing key, at target/waos/<id>-<version>.wapk, listed in target/waos/index.json.
    /// The id, name and icon are the package's [package.metadata.waos]; the version is its own.
    Package(Program),
    /// Drives and captures a running program's windows: screenshot, resize, tap, press, drag,
    /// type, key, and scroll.
    Ui(UiArgs),
    /// Reads or changes a program's settings (`list` by default, or `set <row> <value>`).
    ///
    /// The settings are the program's own, kept by WaOS, for a program a dev run on this machine
    /// runs or has run; WaOS is started when none answers.
    Settings(SettingsArgs),
    /// Inspects what WaOS knows of what a program does with what it was allowed, as JSON lines of
    /// wasdk:inspect's values: `summary`, `active-uses`, or `trace`.
    Inspect(InspectArgs),
    /// Reads WaOS's docs of the version the app builds with, searches them, or installs the agent
    /// skill (`docs skill install`).
    ///
    /// Without a page, prints the map of WaOS's docs; with a page, prints that page as Markdown;
    /// with --search, prints every matching line as <page>:<line>: <text>.
    Docs(DocsArgs),
}

#[derive(Args)]
struct UiArgs {
    #[command(subcommand)]
    command: UiCommand,
}

#[derive(Subcommand)]
enum UiCommand {
    /// Writes the last picture of each window of the running program, as
    /// target/waos/screenshots/<package>/window-<id>.png, and prints each with its window's id,
    /// title and size in pixels.
    ///
    /// --out writes them where it names instead: a folder, one that exists or is written with a
    /// `/` at its end, holds every window's picture as window-<id>.png; any other path is one
    /// file, holding --window's picture, the first window's without it. --window asks WaOS for
    /// that window alone.
    Screenshot(ScreenshotArgs),
    /// Sizes a window of the running program, so what the program draws in it is --to, its
    /// top-left where it is, and prints the size it took and its scale. Returns once the window
    /// has shown the program's picture at that size.
    ///
    /// The size is in logical points, which the program lays out in: a screenshot's pixels, and
    /// the points tap and the other input commands take, are points times the window's scale.
    Resize(ResizeArgs),
    /// Taps a window of the running program: a press and release of the mouse there. Returns once
    /// the window has shown its next picture.
    ///
    /// The point is in the window's pixels, as `ui screenshot` writes them, not in its logical
    /// points.
    ///
    /// The press and release are made by WaOS, with a mouse of its own, and handed to the program
    /// as WaOS hands over its user's input, so the program hears them as a user's; the system
    /// never sees them, and the real pointer does not move.
    Tap(TapArgs),
    /// Presses a window of the running program at a point and holds it: the mouse's button down
    /// there, held for --duration, then up. Returns once the window has shown its next picture
    /// after the release.
    ///
    /// The point is in the window's pixels, as `ui screenshot` writes them, not in its logical
    /// points.
    ///
    /// The press and release are made by WaOS, with a mouse of its own, and handed to the program
    /// as WaOS hands over its user's input, so the program hears them as a user's; the system
    /// never sees them, and the real pointer does not move.
    Press(PressArgs),
    /// Drags in a window of the running program: the mouse's button down at --from, held there
    /// for --hold, moved to --to in --steps equal moves spread over --duration, and up at --to.
    /// Returns once the window has shown its next picture after the release.
    ///
    /// Both points are in the window's pixels, as `ui screenshot` writes them, not in its logical
    /// points.
    ///
    /// The press, the moves and the release are made by WaOS, with a mouse of its own, and handed
    /// to the program as WaOS hands over its user's input, so the program hears them as a user's;
    /// the system never sees them, and the real pointer does not move.
    Drag(DragArgs),
    /// Types text into what a window of the running program edits, as an input method enters it.
    /// Returns once the window has shown its next picture.
    ///
    /// The text is committed by WaOS, as an input method commits its text, and handed to the
    /// program as WaOS hands over its user's input, so the program hears it as a user's; the
    /// system never sees it.
    Type(TypeArgs),
    /// Presses a key, or a chord, in a window of the running program: enter, escape, tab, a,
    /// cmd+s, shift+tab. Returns once the window has shown its next picture.
    ///
    /// The key events are made by WaOS, a US keyboard's, and handed to the program as WaOS hands
    /// over its user's input, so the program hears them as a user's; the system and the window's
    /// menu bar never see them. A key the program leaves goes nowhere, where a real one would go
    /// on to the menu bar, choosing the item it is the shortcut of (Quit, Copy, a File > Save of
    /// the program's): a gap known for now.
    Key(KeyArgs),
    /// Scrolls a window of the running program at a point, as a mouse wheel does. Returns once
    /// the window has shown its next picture.
    ///
    /// The point and the distance are in the window's pixels, as `ui screenshot` writes them, not
    /// in its logical points.
    ///
    /// The scroll is made by WaOS, with a mouse of its own, and handed to the program as WaOS
    /// hands over its user's input, so the program hears them as a user's; the system never sees
    /// it, and the real pointer does not move.
    Scroll(ScrollArgs),
}

#[derive(Args)]
struct SettingsArgs {
    #[command(flatten)]
    program: Program,
    #[command(subcommand)]
    command: Option<SettingsCommand>,
}

#[derive(Subcommand)]
enum SettingsCommand {
    /// Prints the program's settings, one row each (the default when no subcommand is given).
    List(Program),
    /// Sets one row of the program's settings, as its user would on its settings page in WaOS,
    /// and prints the rows after it. A row on ask is asked of the user when the program asks;
    /// one set is answered without asking, every run, installed or not.
    ///
    ///   camera | microphone                          allow | deny | simulated | ask
    ///   notifications                                allow | deny | ask
    ///   simulated-microphone                         tone | silence | global
    ///   network | local-network | clipboard-read | clipboard-write   on | off | global
    ///   record                                       on | off | global
    ///   link-scheme <scheme>                         allow | deny | ask
    ///   role.inspector                               allow | deny | ask
    ///   machine | audio-output                       <id> | ask
    ///   open-file | open-folder | save-file          <path> | ask
    ///   open-files                                   <path>... | ask
    Set(SetArgs),
}

#[derive(Args)]
struct InspectArgs {
    #[command(flatten)]
    program: Program,
    #[command(subcommand)]
    command: InspectCommand,
}

#[derive(Subcommand)]
enum InspectCommand {
    /// Prints the program's audit summary, a day a line, newest first.
    Summary(InspectSummaryArgs),
    /// Streams what the program uses now, each use that lasts and each open file, until Ctrl-C or
    /// the program's end.
    ActiveUses(Program),
    /// Streams live events for the named capabilities (network, files, clipboard, camera,
    /// microphone, machines, permissions, links, notifications, output, or `all`).
    Trace(InspectTraceArgs),
}

#[derive(Args)]
struct InspectSummaryArgs {
    #[command(flatten)]
    program: Program,
    /// How many of the summary's days, newest first.
    #[arg(long, default_value_t = 7)]
    days: u32,
}

#[derive(Args)]
struct InspectTraceArgs {
    #[command(flatten)]
    program: Program,
    /// Capabilities to trace (`all` when omitted, or any of network, files, clipboard, camera,
    /// microphone, machines, permissions, links, notifications, output).
    #[arg(value_name = "CAPABILITY")]
    capabilities: Vec<String>,
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct DocsArgs {
    /// The page: guide/network, waft/state, network, or its address on waos.app.
    #[arg(conflicts_with = "search")]
    page: Option<String>,
    /// Prints every line of every page that holds this text, whatever its case.
    #[arg(long, value_name = "TEXT")]
    search: Option<String>,
    #[command(subcommand)]
    command: Option<DocsCommand>,
}

#[derive(Subcommand)]
enum DocsCommand {
    /// Writes WaOS's docs, of the version the app builds with, as a skill an agent loads when it
    /// builds a WaOS app.
    Skill(SkillArgs),
}

#[derive(Args)]
struct SkillArgs {
    #[command(subcommand)]
    command: SkillCommand,
}

#[derive(Subcommand)]
enum SkillCommand {
    /// Writes the skill as a folder, waos, in a skills folder, and prints where: SKILL.md, the
    /// playbook under the front matter that says when to use it, and docs/, every page of the
    /// docs as `waos docs <page>` prints it, with docs/index.md their map. SKILL.md and docs/ are
    /// replaced whole; nothing else in the folder is touched.
    ///
    /// Without --dir it goes in ~/.claude/skills, Claude Code's personal skills folder, which
    /// every project sees, as every app you make wants it. For a project's alone, or for
    /// another agent, name that agent's skills folder: --dir .claude/skills.
    Install {
        /// The skills folder the skill's folder, waos, is written in; ~/.claude/skills without it.
        #[arg(long, value_name = "PATH")]
        dir: Option<PathBuf>,
    },
}

#[derive(Args)]
struct ScreenshotArgs {
    #[command(flatten)]
    program: Program,
    /// The window to take a picture of alone, by its id as the pictures list them; every window
    /// without it. With --out a file, this window's picture is the one written, or the first
    /// window's without it.
    #[arg(long, value_name = "ID")]
    window: Option<WindowId>,
    /// Where the pictures go: a folder, one that exists or is written with a `/` at its end, for
    /// every window's picture as window-<id>.png, or a file for one window's picture.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct SetArgs {
    #[command(flatten)]
    program: Program,
    /// The row, then its value: `camera allow`, `save-file /tmp/out.pdf`, `machine ask`. A path is
    /// made whole against this directory, with `.` and `..` resolved as written.
    #[arg(required = true, num_args = 2..)]
    row: Vec<String>,
}

/// What a new program is called, and the widget kit it starts with.
#[derive(Args)]
struct NewArgs {
    /// The program's package name, and its folder's: lowercase letters, digits, `-` and `_`,
    /// starting with a letter.
    name: String,
    /// The widget kit the program starts with.
    #[arg(long, value_enum, default_value_t)]
    template: Template,
}

/// Which program.
#[derive(Args, Clone, Default)]
struct Program {
    /// The program's package; without it, the package of this directory.
    #[arg(short, long, value_name = "PACKAGE")]
    package: Option<String>,
}

impl Program {
    fn or(self, fallback: &Program) -> Program {
        Program {
            package: self.package.or_else(|| fallback.package.clone()),
        }
    }
}

/// Which program, and which build of it.
#[derive(Args)]
struct BuildArgs {
    #[command(flatten)]
    program: Program,
    /// An optimized build.
    #[arg(long)]
    release: bool,
}

/// Which program, which build of it, and the variables its run adds.
#[derive(Args)]
struct RunArgs {
    #[command(flatten)]
    build: BuildArgs,
    /// A variable the program reads in this run alone, as NAME=VALUE, repeatable: --env
    /// RUST_LOG=debug. Nothing of this shell's environment is passed, installed programs never
    /// get these, and TMPDIR, which WaOS sets, wins over a variable of that name.
    #[arg(long, value_name = "NAME=VALUE", value_parser = variables::parse)]
    env: Vec<(String, String)>,
    /// The program's arguments, after `--`: `waos run -- --port 8080`.
    #[arg(last = true, value_name = "ARG")]
    args: Vec<String>,
}

/// Which program, and which of its windows.
#[derive(Args)]
struct WindowArgs {
    #[command(flatten)]
    program: Program,
    /// The window's id, as `ui screenshot` lists it: its picture is in pixels, as the
    /// screenshot's, and its logical points are those divided by its scale. Without it, acts on
    /// the program's first open window.
    #[arg(long)]
    window: Option<WindowId>,
}

#[derive(Args)]
struct TapArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Where, in the window's pixels from its top-left, as `ui screenshot` writes them,
    /// not in its logical points: <x>,<y>.
    #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
    at: Pair,
}

#[derive(Args)]
struct TypeArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// The text.
    #[arg(allow_hyphen_values = true)]
    text: String,
}

#[derive(Args)]
struct KeyArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// The key, or modifiers and a key joined by +: cmd, ctrl, alt and shift, then a letter, a
    /// digit, a symbol, enter, escape, tab, space, backspace, delete, up, down, left, right,
    /// home, end, pageup, pagedown or f1 to f12.
    #[arg(allow_hyphen_values = true)]
    chord: String,
}

#[derive(Args)]
struct ScrollArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Where, in the window's pixels from its top-left, as `ui screenshot` writes them,
    /// not in its logical points: <x>,<y>.
    #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
    at: Pair,
    /// How far, in pixels: <dx>,<dy>, a positive dy scrolling down, towards the content's end.
    #[arg(long, value_name = "DX,DY", allow_hyphen_values = true)]
    by: Pair,
}

#[derive(Args)]
struct PressArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Where, in the window's pixels from its top-left, as `ui screenshot` writes them,
    /// not in its logical points: <x>,<y>.
    #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
    at: Pair,
    /// How long the button is held down, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 1000)]
    duration: u64,
}

#[derive(Args)]
struct DragArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Where the button goes down, in the window's pixels from its top-left, as `ui screenshot`
    /// writes them, not in its logical points: <x>,<y>.
    #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
    from: Pair,
    /// Where the button comes up, in the window's pixels from its top-left, as `ui screenshot`
    /// writes them, not in its logical points: <x>,<y>.
    #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
    to: Pair,
    /// How many equal moves the way from --from to --to is made in.
    #[arg(long, value_name = "N", default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
    steps: u32,
    /// How long the moves take together, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 300)]
    duration: u64,
    /// How long the button is held down at --from before the first move, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 0)]
    hold: u64,
}

#[derive(Args)]
struct ResizeArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// The size of what the window shows, in logical points, not pixels: <width>x<height>, such
    /// as 390x844, which a Retina window's screenshot is 780x1688 pixels.
    #[arg(long, value_name = "WIDTHxHEIGHT")]
    to: Points,
}

impl BuildArgs {
    fn profile(&self) -> Profile {
        if self.release {
            Profile::Release
        } else {
            Profile::Debug
        }
    }
}

fn dispatch(command: Command, bin_name: &str, starter: &dyn WaOSStarter) -> Result<ExitCode> {
    match command {
        Command::New(new) => new_program(&new, bin_name),
        Command::Build(build) => build_only(&build),
        Command::Run(run) => build_and_run(&run, starter),
        Command::Package(program) => package(&program, bin_name),
        Command::Ui(ui) => dispatch_ui(ui.command),
        Command::Settings(settings) => dispatch_settings(settings, starter),
        Command::Inspect(inspect) => dispatch_inspect(inspect),
        Command::Docs(docs) => match docs.command {
            Some(DocsCommand::Skill(SkillArgs {
                command: SkillCommand::Install { dir },
            })) => install_skill(dir, bin_name),
            None => show_docs(&docs),
        },
    }
}

fn dispatch_ui(command: UiCommand) -> Result<ExitCode> {
    match command {
        UiCommand::Screenshot(shot) => screenshot(&shot),
        UiCommand::Resize(resize) => resize_window(&resize),
        UiCommand::Tap(tap) => give(
            &tap.window,
            Input::Tap {
                x: tap.at.0,
                y: tap.at.1,
            },
        ),
        UiCommand::Press(press) => give(
            &press.window,
            Input::Press {
                x: press.at.0,
                y: press.at.1,
                duration: Duration::from_millis(press.duration),
            },
        ),
        UiCommand::Drag(drag) => give(
            &drag.window,
            Input::Drag {
                from: (drag.from.0, drag.from.1),
                to: (drag.to.0, drag.to.1),
                hold: Duration::from_millis(drag.hold),
                steps: drag.steps,
                duration: Duration::from_millis(drag.duration),
            },
        ),
        UiCommand::Type(typed) => give(&typed.window, Input::Text(typed.text)),
        UiCommand::Key(key) => give(&key.window, Input::Key(key.chord)),
        UiCommand::Scroll(scroll) => give(
            &scroll.window,
            Input::Scroll {
                x: scroll.at.0,
                y: scroll.at.1,
                dx: scroll.by.0,
                dy: scroll.by.1,
            },
        ),
    }
}

fn dispatch_settings(args: SettingsArgs, starter: &dyn WaOSStarter) -> Result<ExitCode> {
    match args.command {
        None => {
            program_id(&args.program).and_then(|id| on_this_thread(settings::show(id, starter)))
        }
        Some(SettingsCommand::List(program)) => {
            let program = program.or(&args.program);
            program_id(&program).and_then(|id| on_this_thread(settings::show(id, starter)))
        }
        Some(SettingsCommand::Set(set)) => {
            let program = set.program.or(&args.program);
            program_id(&program).and_then(|id| on_this_thread(settings::set(id, &set.row, starter)))
        }
    }
}

fn dispatch_inspect(args: InspectArgs) -> Result<ExitCode> {
    let (program, asked) = match args.command {
        InspectCommand::Summary(summary) => (
            summary.program.or(&args.program),
            Inspection::Summary { days: summary.days },
        ),
        InspectCommand::ActiveUses(program) => (program.or(&args.program), Inspection::ActiveUses),
        InspectCommand::Trace(trace) => {
            let words = if trace.capabilities.is_empty() {
                vec!["all".to_owned()]
            } else {
                trace.capabilities
            };
            let asked = inspect::inspection_of(&words, 7)?;
            (trace.program.or(&args.program), asked)
        }
    };
    let id = program_id(&program)?;
    on_this_thread(inspect::inspect(id, asked))
}

/// Writes a new program in a folder of this directory's, says which workspace's members it was
/// added to and, as `cargo new` warns, when its workspace cannot load with it, and says how to
/// run it.
fn new_program(args: &NewArgs, bin_name: &str) -> Result<ExitCode> {
    let created = new::create(&args.name, args.template, Path::new("."))?;
    if let Some(root) = &created.added_to {
        eprintln!(
            "{bin_name}: added `{}` as member of workspace at `{}`",
            args.name,
            root.display()
        );
    }
    if let Some(problem) = new::workspace_problem(&created.folder) {
        eprintln!(
            "{bin_name}: warning: compiling this new program may not work due to invalid workspace configuration: {problem:#}"
        );
    }
    println!("created {}", created.folder.display());
    println!("  cd {} && {bin_name} run", args.name);
    Ok(ExitCode::SUCCESS)
}

/// Builds the program and says where its component is.
fn build_only(args: &BuildArgs) -> Result<ExitCode> {
    let built = build_chosen(args)?;
    println!("{}", built.component.display());
    Ok(ExitCode::SUCCESS)
}

/// Builds the program and runs it in WaOS as the program its id names until it ends, on a
/// runtime of this thread's. A package with no id is refused before anything is built.
fn build_and_run(args: &RunArgs, starter: &dyn WaOSStarter) -> Result<ExitCode> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, args.build.program.package.as_deref())?;
    let id = WaosMetadata::id_of(package)?;
    let built = build::build(&metadata, package, args.build.profile())?;
    on_this_thread(run::run(id, &built, &args.env, &args.args, starter))
}

/// Writes the pictures of the program's windows under target/waos/screenshots/<package>, or where
/// --out asks, as the WaOS running it answers.
fn screenshot(args: &ScreenshotArgs) -> Result<ExitCode> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, args.program.package.as_deref())?;
    let id = WaosMetadata::id_of(package)?;
    let default = workspace::waos_folder(&metadata)
        .join("screenshots")
        .join(package.name.as_str());
    on_this_thread(screenshot::take(
        id,
        args.out.as_deref(),
        args.window,
        &default,
    ))
}

/// Gives `input` to the window the arguments choose, and waits for WaOS's answer.
fn give(window: &WindowArgs, input: Input) -> Result<ExitCode> {
    let id = program_id(&window.program)?;
    on_this_thread(input::give(id, window.window, input))
}

/// Sizes the window the arguments choose, and waits for WaOS's answer.
fn resize_window(args: &ResizeArgs) -> Result<ExitCode> {
    let id = program_id(&args.window.program)?;
    on_this_thread(resize::resize(id, args.window.window, args.to))
}

/// The id in the chosen package's [package.metadata.waos], read without building anything.
fn program_id(program: &Program) -> Result<ProgramId> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, program.package.as_deref())?;
    WaosMetadata::id_of(package)
}

/// What `work` answers, on a runtime of this thread's: a command's connection to WaOS, or a
/// download.
fn on_this_thread<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("a runtime for the connection")?;
    runtime.block_on(work)
}

/// Packages the program and says where its package is.
fn package(program: &Program, bin_name: &str) -> Result<ExitCode> {
    let packaged = package::package(program.package.as_deref())?;
    eprintln!("{bin_name}: listed in {}", packaged.index.display());
    println!("{}", packaged.package.display());
    Ok(ExitCode::SUCCESS)
}

/// Prints the map of the docs, the page asked for, or the lines that hold the text searched for;
/// a search that finds nothing fails, saying so.
fn show_docs(args: &DocsArgs) -> Result<ExitCode> {
    let docs = on_this_thread(docs::load())?;
    match (&args.page, &args.search) {
        (_, Some(text)) => {
            let hits = docs.search(text);
            anyhow::ensure!(!hits.is_empty(), "no page holds {text:?}");
            for hit in hits {
                println!("{}:{}: {}", hit.page, hit.line, hit.text);
            }
        }
        (Some(page), None) => print!("{}", docs.find(page)?.markdown),
        (None, None) => print!("{}", docs::terminal_map(&docs)),
    }
    Ok(ExitCode::SUCCESS)
}

/// Writes the skill into `dir`, or the default skills folder, and says where.
fn install_skill(dir: Option<PathBuf>, bin_name: &str) -> Result<ExitCode> {
    let skills = match dir {
        Some(dir) => dir,
        None => skill::default_skills_folder()?,
    };
    let folder = skill::install(&skills, &on_this_thread(docs::load())?)?;
    eprintln!("{bin_name}: wrote the skill, SKILL.md and docs/");
    println!("{}", folder.display());
    Ok(ExitCode::SUCCESS)
}

/// Builds the program the arguments choose.
fn build_chosen(args: &BuildArgs) -> Result<BuiltProgram> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, args.program.package.as_deref())?;
    build::build(&metadata, package, args.profile())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_argv(argv: &[&str]) -> Result<Command, clap::Error> {
        let normalized = normalize_argv(argv.iter().map(OsString::from));
        Cli::try_parse_from(&normalized).map(|cli| cli.command)
    }

    fn parsed(arguments: &[&str]) -> Command {
        let mut argv = vec!["waos"];
        argv.extend_from_slice(arguments);
        parse_argv(&argv).unwrap_or_else(|error| panic!("{arguments:?}: {error}"))
    }

    fn parsed_cargo(arguments: &[&str]) -> Command {
        let mut argv = vec!["cargo-waos", "waos"];
        argv.extend_from_slice(arguments);
        parse_argv(&argv).unwrap_or_else(|error| panic!("{arguments:?}: {error}"))
    }

    fn refused(arguments: &[&str]) -> bool {
        let mut argv = vec!["waos"];
        argv.extend_from_slice(arguments);
        parse_argv(&argv).is_err()
    }

    #[test]
    fn normalize_argv_strips_leading_waos_only_for_cargo_waos() {
        let norm = |args: &[&str]| -> Vec<String> {
            normalize_argv(args.iter().map(OsString::from))
                .into_iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(norm(&["cargo-waos", "waos", "run"]), ["cargo-waos", "run"]);
        assert_eq!(
            norm(&["/usr/local/bin/cargo-waos", "waos", "ui", "screenshot"]),
            ["/usr/local/bin/cargo-waos", "ui", "screenshot"]
        );
        assert_eq!(
            norm(&["cargo-waos.exe", "waos", "build", "--release"]),
            ["cargo-waos.exe", "build", "--release"]
        );
        assert_eq!(norm(&["cargo-waos", "run"]), ["cargo-waos", "run"]);
        assert_eq!(norm(&["waos", "waos"]), ["waos", "waos"]);
        assert_eq!(norm(&["/opt/bin/waos", "waos"]), ["/opt/bin/waos", "waos"]);
        assert_eq!(norm(&["waos", "triangle.wapk"]), ["waos", "triangle.wapk"]);
    }

    #[test]
    fn domain_ui_screenshot_parses() {
        let Command::Ui(UiArgs {
            command: UiCommand::Screenshot(shot),
        }) = parsed(&[
            "ui",
            "screenshot",
            "-p",
            "notes",
            "--out",
            "shots/",
            "--window",
            "2",
        ])
        else {
            panic!("ui screenshot");
        };
        assert_eq!(shot.program.package.as_deref(), Some("notes"));
        assert_eq!(shot.window, Some(WindowId(2)));
        assert_eq!(shot.out.as_deref(), Some(Path::new("shots/")));

        let Command::Ui(UiArgs {
            command: UiCommand::Screenshot(cargo_shot),
        }) = parsed_cargo(&["ui", "screenshot", "--window", "3"])
        else {
            panic!("cargo waos ui screenshot");
        };
        assert_eq!(cargo_shot.window, Some(WindowId(3)));
        assert!(refused(&["screenshot", "--window", "3"]));
    }

    #[test]
    fn domain_ui_inputs_and_resize_parse_under_ui() {
        let Command::Ui(UiArgs {
            command: UiCommand::Tap(tap),
        }) = parsed(&[
            "ui", "tap", "--at", "120,48", "--window", "2", "-p", "notes",
        ])
        else {
            panic!("ui tap");
        };
        assert_eq!(
            (tap.at, tap.window.window),
            (Pair(120.0, 48.0), Some(WindowId(2)))
        );
        assert_eq!(tap.window.program.package.as_deref(), Some("notes"));
        assert!(refused(&["ui", "tap"]));
        assert!(refused(&["ui", "tap", "--at", "120"]));

        assert!(refused(&["tap", "--at", "1.5,2"]));

        let Command::Ui(UiArgs {
            command: UiCommand::Scroll(scroll),
        }) = parsed(&["ui", "scroll", "--at", "10,20", "--by", "-5,-200"])
        else {
            panic!("ui scroll");
        };
        assert_eq!(
            (scroll.at, scroll.by),
            (Pair(10.0, 20.0), Pair(-5.0, -200.0))
        );
        assert!(refused(&["ui", "scroll", "--at", "10,20"]));

        let Command::Ui(UiArgs {
            command: UiCommand::Press(press),
        }) = parsed(&["ui", "press", "--at", "10,20"])
        else {
            panic!("ui press");
        };
        assert_eq!((press.at, press.duration), (Pair(10.0, 20.0), 1000));

        let Command::Ui(UiArgs {
            command: UiCommand::Drag(drag),
        }) = parsed(&[
            "ui",
            "drag",
            "--from",
            "1,2",
            "--to",
            "3,4",
            "--steps",
            "1",
            "--duration",
            "0",
            "--hold",
            "600",
            "--window",
            "2",
        ])
        else {
            panic!("ui drag");
        };
        assert_eq!((drag.steps, drag.duration, drag.hold), (1, 0, 600));
        assert!(refused(&[
            "ui", "drag", "--from", "1,2", "--to", "3,4", "--steps", "0"
        ]));

        let Command::Ui(UiArgs {
            command: UiCommand::Type(typed),
        }) = parsed(&["ui", "type", "-5 eggs, milk"])
        else {
            panic!("ui type");
        };
        assert_eq!(typed.text, "-5 eggs, milk");

        let Command::Ui(UiArgs {
            command: UiCommand::Key(key),
        }) = parsed(&["ui", "key", "cmd+s", "--window", "3"])
        else {
            panic!("ui key");
        };
        assert_eq!(
            (key.chord.as_str(), key.window.window),
            ("cmd+s", Some(WindowId(3)))
        );

        let Command::Ui(UiArgs {
            command: UiCommand::Resize(resize),
        }) = parsed(&["ui", "resize", "--to", "390x844", "-p", "notes"])
        else {
            panic!("ui resize");
        };
        assert_eq!(
            resize.to,
            Points {
                width: 390.0,
                height: 844.0
            }
        );
        assert_eq!(resize.window.program.package.as_deref(), Some("notes"));
        assert!(refused(&["ui", "resize", "--to", "390,844"]));
        assert!(refused(&["resize", "--to", "320x568"]));
    }

    #[test]
    fn settings_defaults_to_list_and_supports_list_and_set() {
        let Command::Settings(args) = parsed(&["settings"]) else {
            panic!("settings");
        };
        assert!(args.command.is_none());

        let Command::Settings(args) = parsed(&["settings", "-p", "notes"]) else {
            panic!("settings -p");
        };
        assert_eq!(args.program.package.as_deref(), Some("notes"));
        assert!(args.command.is_none());

        let Command::Settings(SettingsArgs {
            command: Some(SettingsCommand::List(program)),
            ..
        }) = parsed(&["settings", "list", "-p", "notes"])
        else {
            panic!("settings list");
        };
        assert_eq!(program.package.as_deref(), Some("notes"));

        let Command::Settings(SettingsArgs {
            command: Some(SettingsCommand::Set(set)),
            ..
        }) = parsed(&["settings", "set", "camera", "allow", "-p", "notes"])
        else {
            panic!("settings set");
        };
        assert_eq!(set.row, ["camera", "allow"]);
        assert_eq!(set.program.package.as_deref(), Some("notes"));

        let Command::Settings(SettingsArgs {
            program: parent,
            command: Some(SettingsCommand::Set(set)),
        }) = parsed(&["settings", "-p", "notes", "set", "open-files", "/a", "/b"])
        else {
            panic!("settings -p set");
        };
        assert_eq!(set.program.or(&parent).package.as_deref(), Some("notes"));
        assert_eq!(set.row, ["open-files", "/a", "/b"]);

        assert!(refused(&["settings", "set", "camera"]));

        assert!(refused(&["set", "camera", "allow"]));
    }

    #[test]
    fn inspect_supports_summary_active_uses_and_trace() {
        let Command::Inspect(InspectArgs {
            command: InspectCommand::Summary(summary),
            ..
        }) = parsed(&["inspect", "summary", "--days", "14", "-p", "notes"])
        else {
            panic!("inspect summary");
        };
        assert_eq!(summary.days, 14);
        assert_eq!(summary.program.package.as_deref(), Some("notes"));

        let Command::Inspect(InspectArgs {
            command: InspectCommand::ActiveUses(program),
            ..
        }) = parsed(&["inspect", "active-uses", "-p", "notes"])
        else {
            panic!("inspect active-uses");
        };
        assert_eq!(program.package.as_deref(), Some("notes"));

        let Command::Inspect(InspectArgs {
            command: InspectCommand::Trace(trace),
            ..
        }) = parsed(&["inspect", "trace", "network", "files"])
        else {
            panic!("inspect trace");
        };
        assert_eq!(trace.capabilities, ["network", "files"]);

        let Command::Inspect(InspectArgs {
            command: InspectCommand::Trace(trace),
            ..
        }) = parsed(&["inspect", "trace"])
        else {
            panic!("inspect trace default all");
        };
        assert!(trace.capabilities.is_empty());

        assert!(refused(&["inspect", "all"]));
    }

    #[test]
    fn docs_supports_map_page_search_and_docs_skill_install() {
        let Command::Docs(docs) = parsed(&["docs"]) else {
            panic!("docs");
        };
        assert_eq!((docs.page, docs.search), (None, None));
        assert!(docs.command.is_none());

        let Command::Docs(docs) = parsed(&["docs", "guide/network"]) else {
            panic!("docs page");
        };
        assert_eq!(docs.page.as_deref(), Some("guide/network"));

        let Command::Docs(docs) = parsed(&["docs", "--search", "Retained"]) else {
            panic!("docs search");
        };
        assert_eq!(docs.search.as_deref(), Some("Retained"));

        assert!(refused(&["docs", "waft", "--search", "x"]));

        let Command::Docs(DocsArgs {
            command:
                Some(DocsCommand::Skill(SkillArgs {
                    command: SkillCommand::Install { dir },
                })),
            ..
        }) = parsed(&["docs", "skill", "install", "--dir", ".claude/skills"])
        else {
            panic!("docs skill install");
        };
        assert_eq!(dir.as_deref(), Some(Path::new(".claude/skills")));

        assert!(refused(&["skill", "install"]));
        assert!(refused(&["skill"]));
        assert!(refused(&["docs", "skill"]));
    }

    #[test]
    fn each_ui_inputs_help_says_waos_makes_it_and_the_system_never_sees_it() {
        let mut cli = Cli::command();
        let ui = cli.find_subcommand_mut("ui").expect("ui");
        for input in ["tap", "type", "key", "scroll", "press", "drag"] {
            let help = ui
                .find_subcommand_mut(input)
                .expect("the input's command")
                .render_long_help()
                .to_string();
            assert!(help.contains("by WaOS"), "{input}: {help}");
            assert!(help.contains("never see"), "{input}: {help}");
        }
        let key = ui.find_subcommand_mut("key").unwrap().render_long_help();
        assert!(key.to_string().contains("menu bar never see them"), "{key}");
        assert!(key.to_string().contains("goes nowhere"), "{key}");
    }

    #[test]
    fn top_level_help_lists_only_domain_commands() {
        let help = build_clap_command("waos").render_help().to_string();
        for visible in [
            "new", "build", "run", "package", "ui", "settings", "inspect", "docs",
        ] {
            assert!(
                help.contains(visible),
                "missing {visible} in:
{help}"
            );
        }
        assert!(help.contains("`waos docs`"), "{help}");
        // Removed flat verbs must not appear as top-level commands.
        for line in help.lines() {
            let trimmed = line.trim_start();
            for removed in [
                "screenshot ",
                "resize ",
                "tap ",
                "press ",
                "drag ",
                "set ",
                "skill ",
            ] {
                assert!(
                    !trimmed.starts_with(removed),
                    "removed flat command {removed:?} appeared in top-level help:
{help}"
                );
            }
        }
    }

    #[test]
    fn a_run_takes_variables_and_arguments_after_double_dash() {
        let Command::Run(run) = parsed(&[
            "run",
            "-p",
            "notes",
            "--env",
            "RUST_LOG=debug",
            "--",
            "--port",
            "8080",
        ]) else {
            panic!("a run");
        };
        assert_eq!(run.build.program.package.as_deref(), Some("notes"));
        assert_eq!(run.env, [("RUST_LOG".to_owned(), "debug".to_owned())]);
        assert_eq!(run.args, ["--port", "8080"]);
    }
}
