//! `waos`, the command: writes a new program, builds a program as a component, installs it from
//! its source in the WaOS on this machine as its development install, launches it, attaches to
//! it, stops it and uninstalls it, runs it (all of these at once), and packages it as a signed
//! `.wapk` for a package source; while it runs there, takes screenshots of its windows, gives them
//! input and sizes them (`ui`), reads and sets its settings (`settings`), inspects what it does
//! (`inspect`), and reads or installs WaOS's docs (`docs`).
//!
//! This crate is engine-free and builds on stable Rust. Both its own `waos` binary, the command
//! alone, and the `waos` binary that also is WaOS call into [`run_from_args`] with their own [`WaOSStarter`].
//!
//! The invariant: every command is about a program and the WaOS it runs in. An install, a launch, a
//! run and an uninstall go to the WaOS already running, starting one through the caller's
//! [`WaOSStarter`] when none answers, never to an engine of this crate's own; a program's settings
//! go to the WaOS keeping them, started the same way; an attach, a stop, a screenshot, resize,
//! input, or live inspection goes to the WaOS running the program, never starting one. Inside a
//! program's project, a command with no id names the project's development id, its
//! `[package.metadata.waos]` id followed by `.development`.

mod asked;
mod asking;
mod attach;
mod build;
mod component_imports;
mod configuration;
mod connecting;
// Linux's alone; its tests run everywhere.
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod desktop_entries;
mod destination;
mod devices;
mod docs;
mod ext;
mod found;
mod icon_rule;
mod input;
mod inspect;
mod install;
mod launch;
mod metadata;
mod new;
mod os;
mod package;
mod pair;
mod resize;
mod run;
mod screenshot;
mod settings;
mod signing_key;
mod skill;
mod stop;
mod uninstall;
mod variables;
mod verify;
pub mod waos_app;
mod workspace;

use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use waos_dev_protocol::{Input, Inspection, WindowId};
use waos_package::{ProgramId, PublisherKey};

use crate::attach::OnInterrupt;
use crate::build::{BuiltProgram, Features, Profile};
use crate::destination::Destination;
use crate::input::Pair;
use crate::install::InstallBuild;
use crate::metadata::{InstallIdentity, WaosMetadata};
use crate::new::Template;
use crate::resize::Points;

pub use crate::connecting::{DefaultStarter, WaOSStarter};

/// Top-level developer CLI subcommands.
pub const CLI_COMMANDS: &[&str] = &[
    "new",
    "build",
    "install",
    "launch",
    "attach",
    "stop",
    "uninstall",
    "run",
    "package",
    "ui",
    "settings",
    "inspect",
    "ext",
    "docs",
    "pair",
    "unpair",
    "devices",
    "help",
];

/// Whether `word` is a developer CLI subcommand.
pub fn is_cli_command(word: &str) -> bool {
    CLI_COMMANDS.contains(&word)
}

/// Runs the command alone (no WaOS in this binary) on `std::env::args_os()`.
pub fn run_from_env(starter: &dyn WaOSStarter) -> ExitCode {
    run_from_args(std::env::args_os(), starter, None)
}

/// Runs the command on the full `argv` (including `argv[0]`), starting WaOS through `starter`;
/// `app_help` is what the binary that also is WaOS adds to the help about its own launches.
pub fn run_from_args(
    argv: impl IntoIterator<Item = OsString>,
    starter: &dyn WaOSStarter,
    app_help: Option<&'static str>,
) -> ExitCode {
    let argv: Vec<OsString> = argv.into_iter().collect();

    let matches = match build_clap_command(app_help).try_get_matches_from(&argv) {
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
    dispatch(cli.command, cli.device.as_deref(), starter).unwrap_or_else(|error| {
        eprintln!("waos: {error:#}");
        ExitCode::FAILURE
    })
}

fn build_clap_command(app_help: Option<&'static str>) -> clap::Command {
    let command = Cli::command().bin_name("waos");
    match app_help {
        Some(help) => command.after_help(help),
        None => command,
    }
}

#[derive(Parser)]
#[command(
    name = "waos",
    about = "waos: writes a new program, builds a program as a component, installs it in WaOS from its source, launches, attaches to and stops it, or runs it, all at once, packages it, drives its UI, manages its settings, inspects its activity, and reads WaOS's docs. `waos docs` is the map of WaOS's docs for writing an app.",
    after_help = "The docs and guides are at https://waos.app/docs; `waos docs` prints the app's version's.",
    version
)]
struct Cli {
    /// The device whose WaOS the command goes to, by the name or the address it was paired with
    /// (`waos pair`); the WaOS on this machine without one.
    #[arg(long, global = true, env = "WAOS_DEVICE", value_name = "DEVICE")]
    device: Option<String>,
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
    /// Builds the program and installs it from its source in the WaOS on this machine, starting
    /// WaOS when none answers, as its development install: a program of its own, under the id in
    /// its [package.metadata.waos] followed by .development, with its own data and settings,
    /// beside the program published under the plain id. Installing again replaces its component
    /// and keeps its data and settings. Prints the id it is installed as.
    Install(BuildArgs),
    /// Starts an installed program in the WaOS on this machine, starting WaOS when none answers,
    /// in place of its run if it runs, and returns once it runs: the program runs on until it
    /// ends or `waos stop` stops it, and `waos attach` reads what it writes.
    ///
    /// Without --id, the program is this package's development install. Each --env NAME=VALUE
    /// adds a variable the program reads in this launch alone, and nothing of this shell's
    /// environment is passed; TMPDIR, which WaOS sets, wins over a variable of that name.
    /// Arguments after -- reach the program as its own arguments, as `cargo run --` passes them.
    Launch(LaunchArgs),
    /// Writes what a program running writes, however it was started, here, as it writes it, each
    /// line after the program's name, from what it wrote with nobody attached on, until it ends;
    /// the exit status is 0 for a program that returned success and 1 otherwise. Ctrl-C detaches,
    /// and the program runs on.
    ///
    /// Without --id, the program is this package's development install.
    Attach(Target),
    /// Stops the program running, however it was started. Without --id, the program is this
    /// package's development install.
    Stop(Target),
    /// Uninstalls a program, with its settings and its data, its run stopped first, starting
    /// WaOS when none answers. Without --id, the program is this package's development install,
    /// and the program published under the plain id stays.
    Uninstall(Target),
    /// Builds the program, installs it as its development install, launches it and attaches to
    /// it, as `waos install`, `waos launch` and `waos attach` do one after the other. What it
    /// writes comes here as it writes it, each line after the program's name; Ctrl-C stops it;
    /// the exit status is 0 for a run that returned success and 1 otherwise. `waos install` then
    /// `waos launch` is the same run, detached.
    ///
    /// Each --env NAME=VALUE adds a variable the program reads in this run alone, and nothing of
    /// this shell's environment is passed. A program opened from WaOS never gets them, and
    /// TMPDIR, which WaOS sets, wins over a variable of that name given here. Arguments after --
    /// reach the program as its own arguments, as `cargo run --` passes them.
    ///
    /// A question WaOS asks while the program runs, to allow something or to pick a file, has a
    /// row among the program's settings: `waos settings` lists them, and `waos settings set`
    /// answers one ahead, so the run does not wait for a person.
    Run(RunArgs),
    /// Builds the program's release component and writes it as a package signed with your
    /// signing key, at target/waos/<id>-<version>.wapk, listed in target/waos/index.json.
    /// The id, name and icon are the package's [package.metadata.waos]; the version is its own.
    ///
    /// `waos package verify <file>` verifies a package instead, as WaOS opens it.
    Package(PackageArgs),
    /// Drives and captures a running program's windows: screenshot, resize, tap, press, drag,
    /// type, key, and scroll. Each reaches this package's development install, or, with --id, any
    /// program running, however it was installed or started.
    Ui(UiArgs),
    /// Reads or changes a program's settings (`list` by default, or `set <row> <value>`).
    ///
    /// The settings are the program's own, kept by WaOS, of any program, this package's
    /// development install by default, set ahead of its install too; WaOS is started when none
    /// answers.
    Settings(SettingsArgs),
    /// Inspects what WaOS knows of what a program does with what it was allowed, as JSON lines of
    /// wasdk:inspect's values: `summary`, `active-uses`, or `trace`.
    Inspect(InspectArgs),
    /// Calls a running program's service extensions, as Waft's CLI (`cargo waft ext`) calls an
    /// app's: `waos ext --stdio` relays Waft's service protocol, one JSON message a line, between
    /// stdin and stdout and the program, until stdin closes; `waos ext list` lists the programs
    /// running that serve extensions, as Waft's listing with --json.
    ///
    /// Without --id, the program is this package's development install. A program serves its
    /// extensions in a debug build, as a Waft app's framework serves Waft's.
    Ext(ExtArgs),
    /// Reads WaOS's docs of the version the app builds with, searches them, or installs the agent
    /// skill (`docs skill install`).
    ///
    /// Without a page, prints the map of WaOS's docs; with a page, prints that page as Markdown;
    /// with --search, prints every matching line as <page>:<line>: <text>.
    Docs(DocsArgs),
    /// Pairs this computer with a device's WaOS, by the code the device shows: in its WaOS's
    /// settings, turn on Developer access and choose Pair a computer. Every command then reaches
    /// it with `--device <name>`.
    Pair(PairArgs),
    /// Forgets a device this computer is paired with.
    Unpair(UnpairArgs),
    /// Lists the devices whose WaOS announces Developer access on the local network and those
    /// this computer is paired with, a line each: the name `pair` and `--device` take, its
    /// address, and whether it is paired.
    Devices,
}

#[derive(Args)]
struct PairArgs {
    /// The device: its name as `waos devices` lists it, its name on the network or its IP
    /// address, host:port, or a wss:// URL; port 7351 when none is given. It is the name
    /// `--device` takes after.
    device: String,
    /// The six digits the device shows; asked for when not given.
    #[arg(long)]
    code: Option<String>,
}

#[derive(Args)]
struct UnpairArgs {
    /// The device, by the name or the address it was paired with.
    device: String,
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
    /// top-left where it is, and prints the size it took and its scale, once the window has
    /// reported it.
    ///
    /// The size is in logical points, which the program lays out in: a screenshot's pixels, and
    /// the points tap and the other input commands take, are points times the window's scale.
    Resize(ResizeArgs),
    /// Taps a window of the running program: a press and release of the mouse there. Returns once
    /// it is handed to the program: take a screenshot to see what the program drew.
    ///
    /// The point is in the window's pixels, as `ui screenshot` writes them, not in its logical
    /// points.
    ///
    /// The press and release are made by WaOS, with a mouse of its own, and handed to the program
    /// as WaOS hands over its user's input, so the program hears them as a user's; the system
    /// never sees them, and the real pointer does not move.
    Tap(TapArgs),
    /// Presses a window of the running program at a point and holds it: the mouse's button down
    /// there, held for --duration, then up. Returns once the release is handed to the program.
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
    /// Returns once the release is handed to the program.
    ///
    /// Both points are in the window's pixels, as `ui screenshot` writes them, not in its logical
    /// points.
    ///
    /// The press, the moves and the release are made by WaOS, with a mouse of its own, and handed
    /// to the program as WaOS hands over its user's input, so the program hears them as a user's;
    /// the system never sees them, and the real pointer does not move.
    Drag(DragArgs),
    /// Types text into what a window of the running program edits, as an input method enters it.
    /// Returns once it is handed to the program.
    ///
    /// The text is committed by WaOS, as an input method commits its text, and handed to the
    /// program as WaOS hands over its user's input, so the program hears it as a user's; the
    /// system never sees it.
    Type(TypeArgs),
    /// Presses a key, or a chord, in a window of the running program: enter, escape, tab, a,
    /// cmd+s, shift+tab. Returns once it is handed to the program.
    ///
    /// The key events are made by WaOS, a US keyboard's, and handed to the program as WaOS hands
    /// over its user's input, so the program hears them as a user's; the system and the window's
    /// menu bar never see them. A key the program leaves goes nowhere, where a real one would go
    /// on to the menu bar, choosing the item it is the shortcut of (Quit, Copy, a File > Save of
    /// the program's): a gap known for now.
    Key(KeyArgs),
    /// Scrolls a window of the running program at a point, as a mouse wheel does. Returns once
    /// it is handed to the program.
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
    target: Target,
    #[command(subcommand)]
    command: Option<SettingsCommand>,
}

#[derive(Subcommand)]
enum SettingsCommand {
    /// Prints the program's settings, one row each (the default when no subcommand is given).
    List(Target),
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
    target: Target,
    #[command(subcommand)]
    command: InspectCommand,
}

#[derive(Subcommand)]
enum InspectCommand {
    /// Prints the program's audit summary, a day a line, newest first.
    Summary(InspectSummaryArgs),
    /// Streams what the program uses now, each use that lasts and each open file, until Ctrl-C or
    /// the program's end.
    ActiveUses(Target),
    /// Streams live events for the named capabilities (network, files, clipboard, camera,
    /// microphone, machines, permissions, links, notifications, output, or `all`).
    Trace(InspectTraceArgs),
}

#[derive(Args)]
struct InspectSummaryArgs {
    #[command(flatten)]
    target: Target,
    /// How many of the summary's days, newest first.
    #[arg(long, default_value_t = 7)]
    days: u32,
}

#[derive(Args)]
struct InspectTraceArgs {
    #[command(flatten)]
    target: Target,
    /// Capabilities to trace (`all` when omitted, or any of network, files, clipboard, camera,
    /// microphone, machines, permissions, links, notifications, output).
    #[arg(value_name = "CAPABILITY")]
    capabilities: Vec<String>,
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct ExtArgs {
    #[command(flatten)]
    target: Target,
    /// Relays Waft's service protocol between stdin and stdout and the program, one JSON message
    /// a line, until stdin closes: each request one call of the extension it names.
    #[arg(long)]
    stdio: bool,
    #[command(subcommand)]
    command: Option<ExtCommand>,
}

#[derive(Subcommand)]
enum ExtCommand {
    /// Lists the programs running in WaOS that serve service extensions, a line each: its name
    /// and id, or, with --json, a JSON object in Waft's listing format, its address the command
    /// that relays to it.
    List {
        /// Each as a JSON object a line, as Waft's discovery reads a listing.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct PackageArgs {
    #[command(flatten)]
    program: Program,
    #[command(flatten)]
    features: FeatureArgs,
    #[command(subcommand)]
    command: Option<PackageCommand>,
}

#[derive(Subcommand)]
enum PackageCommand {
    /// Verifies a package as WaOS opens it, its signature, manifest and icon, and prints its id,
    /// its version, its publisher's key in full, 64 hex digits, and that key's fingerprint. The
    /// exit status is 1 for a package that does not verify.
    Verify(VerifyArgs),
}

#[derive(Args)]
struct VerifyArgs {
    /// The package, a .wapk.
    file: PathBuf,
    /// The publisher's key, as 64 hex digits, as this command prints it: the package is refused
    /// unless it is signed with that key.
    #[arg(long, value_name = "KEY")]
    publisher: Option<PublisherKey>,
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
    target: Target,
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
    target: Target,
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

/// Which program a command reaches: the one `--id` names, however it was installed or started,
/// or this package's development install.
#[derive(Args, Clone, Default)]
struct Target {
    /// The program's id, any program's: dev.example.notes, as published, or
    /// dev.example.notes.development, a development install's. Without it, this package's
    /// development install.
    #[arg(long, value_name = "ID", conflicts_with = "package")]
    id: Option<ProgramId>,
    #[command(flatten)]
    program: Program,
}

impl Target {
    /// This target, or `fallback` where this one names nothing, as a subcommand's options fall
    /// back on its command's.
    fn or(self, fallback: &Target) -> Target {
        match (&self.id, &self.program.package) {
            (None, None) => fallback.clone(),
            _ => self,
        }
    }
}

/// Which program a launch starts, and the variables and arguments it gives it.
#[derive(Args)]
struct LaunchArgs {
    #[command(flatten)]
    target: Target,
    /// A variable the program reads in this launch alone, as NAME=VALUE, repeatable: --env
    /// RUST_LOG=debug. Nothing of this shell's environment is passed, and TMPDIR, which WaOS
    /// sets, wins over a variable of that name.
    #[arg(long, value_name = "NAME=VALUE", value_parser = variables::parse)]
    env: Vec<(String, String)>,
    /// The program's arguments, after `--`: `waos launch -- --port 8080`.
    #[arg(last = true, value_name = "ARG")]
    args: Vec<String>,
}

/// Which program, and which build of it.
#[derive(Args)]
struct BuildArgs {
    #[command(flatten)]
    program: Program,
    /// An optimized build.
    #[arg(long)]
    release: bool,
    #[command(flatten)]
    features: FeatureArgs,
}

/// Which of the program's features its build turns on, as cargo's flags of the same names do.
#[derive(Args)]
struct FeatureArgs {
    /// Features to turn on, separated by commas or spaces, or repeated: --features online,update.
    #[arg(short = 'F', long, value_name = "FEATURES", value_delimiter = ',')]
    features: Vec<String>,
    /// Every feature of the program's package.
    #[arg(long)]
    all_features: bool,
    /// None of the package's default features, only those --features names.
    #[arg(long)]
    no_default_features: bool,
}

impl FeatureArgs {
    fn features(&self) -> Features {
        Features {
            chosen: self
                .features
                .iter()
                .flat_map(|named| named.split_whitespace())
                .map(str::to_owned)
                .collect(),
            all: self.all_features,
            without_default: self.no_default_features,
        }
    }
}

/// Which program, which build of it, and the variables its run adds.
#[derive(Args)]
struct RunArgs {
    #[command(flatten)]
    build: BuildArgs,
    /// A variable the program reads in this run alone, as NAME=VALUE, repeatable: --env
    /// RUST_LOG=debug. Nothing of this shell's environment is passed, a program opened from WaOS
    /// never gets these, and TMPDIR, which WaOS sets, wins over a variable of that name.
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
    target: Target,
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

/// Runs `command`, on the WaOS of the paired `device` where it names one, and on this machine's
/// otherwise.
fn dispatch(command: Command, device: Option<&str>, starter: &dyn WaOSStarter) -> Result<ExitCode> {
    let destination = || Destination::named(device);
    match command {
        Command::New(new) => new_program(&new),
        Command::Build(build) => build_only(&build),
        Command::Install(build) => build_and_install(&destination()?, &build, starter),
        Command::Launch(launch) => {
            let destination = destination()?;
            let (id, name) = target_of(&launch.target)?;
            let launching =
                launch::launch(&destination, &id, &name, &launch.args, &launch.env, starter);
            on_this_thread(launching).map(|()| ExitCode::SUCCESS)
        }
        Command::Attach(target) => {
            let destination = destination()?;
            let (id, name) = target_of(&target)?;
            on_this_thread(async {
                let connection = asking::connect(&destination).await?;
                attach::attach_on(connection, &destination, &id, &name, OnInterrupt::Detach).await
            })
        }
        Command::Stop(target) => {
            let destination = destination()?;
            on_this_thread(stop::stop(&destination, target_of(&target)?.0))
        }
        Command::Uninstall(target) => {
            let destination = destination()?;
            let id = target_of(&target)?.0;
            on_this_thread(uninstall::uninstall(&destination, id, starter))
        }
        Command::Run(run) => build_and_run(&destination()?, &run, starter),
        Command::Package(args) => match args.command {
            Some(PackageCommand::Verify(verify)) => verify_package(&verify),
            None => package(&args.program, &args.features),
        },
        Command::Ui(ui) => dispatch_ui(&destination()?, ui.command),
        Command::Settings(settings) => dispatch_settings(&destination()?, settings, starter),
        Command::Inspect(inspect) => dispatch_inspect(&destination()?, inspect),
        Command::Ext(ext) => dispatch_ext(&destination()?, ext),
        Command::Pair(args) => on_this_thread(pair::pair(&args.device, args.code.as_deref())),
        Command::Unpair(args) => pair::unpair(&args.device),
        Command::Devices => pair::devices(),
        Command::Docs(docs) => match docs.command {
            Some(DocsCommand::Skill(SkillArgs {
                command: SkillCommand::Install { dir },
            })) => install_skill(dir),
            None => show_docs(&docs),
        },
    }
}

fn dispatch_ui(destination: &Destination, command: UiCommand) -> Result<ExitCode> {
    let give = |window: &WindowArgs, input: Input| give(destination, window, input);
    match command {
        UiCommand::Screenshot(shot) => screenshot(destination, &shot),
        UiCommand::Resize(resize) => resize_window(destination, &resize),
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

fn dispatch_settings(
    destination: &Destination,
    args: SettingsArgs,
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let show = |id| on_this_thread(settings::show(destination, id, starter));
    match args.command {
        None => target_id(&args.target).and_then(show),
        Some(SettingsCommand::List(target)) => target_id(&target.or(&args.target)).and_then(show),
        Some(SettingsCommand::Set(set)) => {
            let target = set.target.clone().or(&args.target);
            let id = target_id(&target)?;
            on_this_thread(settings::set(destination, id, &set.row, starter))
        }
    }
}

fn dispatch_ext(destination: &Destination, args: ExtArgs) -> Result<ExitCode> {
    match args.command {
        Some(ExtCommand::List { json }) => on_this_thread(ext::list(destination, json)),
        None if args.stdio => {
            on_this_thread(ext::relay_stdio(destination, target_id(&args.target)?))
        }
        None => anyhow::bail!(
            "`waos ext` relays with --stdio (`waos ext --stdio`, `waos ext --id <id> --stdio`), or lists the programs serving extensions (`waos ext list`)"
        ),
    }
}

fn dispatch_inspect(destination: &Destination, args: InspectArgs) -> Result<ExitCode> {
    let (target, asked) = match args.command {
        InspectCommand::Summary(summary) => (
            summary.target.or(&args.target),
            Inspection::Summary { days: summary.days },
        ),
        InspectCommand::ActiveUses(target) => (target.or(&args.target), Inspection::ActiveUses),
        InspectCommand::Trace(trace) => {
            let words = if trace.capabilities.is_empty() {
                vec!["all".to_owned()]
            } else {
                trace.capabilities
            };
            let asked = inspect::inspection_of(&words, 7)?;
            (trace.target.or(&args.target), asked)
        }
    };
    let id = target_id(&target)?;
    on_this_thread(inspect::inspect(destination, id, asked))
}

/// Writes a new program in a folder of this directory's, says which workspace's members it was
/// added to and, as `cargo new` warns, when its workspace cannot load with it, and says how to
/// run it.
fn new_program(args: &NewArgs) -> Result<ExitCode> {
    let created = new::create(&args.name, args.template, Path::new("."))?;
    if let Some(root) = &created.added_to {
        eprintln!(
            "waos: added `{}` as member of workspace at `{}`",
            args.name,
            root.display()
        );
    }
    if let Some(problem) = new::workspace_problem(&created.folder) {
        eprintln!(
            "waos: warning: compiling this new program may not work due to invalid workspace configuration: {problem:#}"
        );
    }
    println!("created {}", created.folder.display());
    println!("  cd {} && waos run", args.name);
    Ok(ExitCode::SUCCESS)
}

/// Builds the program and says where its component is.
fn build_only(args: &BuildArgs) -> Result<ExitCode> {
    let built = build_chosen(args)?;
    println!("{}", built.component.display());
    Ok(ExitCode::SUCCESS)
}

/// Builds the program and installs it as its development install, on a runtime of this
/// thread's. A package with no id is refused before anything is built.
fn build_and_install(
    destination: &Destination,
    args: &BuildArgs,
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, args.program.package.as_deref())?;
    let identity = WaosMetadata::install_identity_of(package)?;
    let built = build::build(
        &metadata,
        package,
        args.profile(),
        &args.features.features(),
    )?;
    let build = install_build(identity, package, &built)?;
    on_this_thread(install::install(destination, &build, starter)).map(|()| ExitCode::SUCCESS)
}

/// Builds the program, installs it as its development install, launches it and attaches to it
/// until it ends, on a runtime of this thread's. A package with no id is refused before anything
/// is built.
fn build_and_run(
    destination: &Destination,
    args: &RunArgs,
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, args.build.program.package.as_deref())?;
    let identity = WaosMetadata::install_identity_of(package)?;
    let built = build::build(
        &metadata,
        package,
        args.build.profile(),
        &args.build.features.features(),
    )?;
    let build = install_build(identity, package, &built)?;
    on_this_thread(run::run(
        &build,
        &built.name,
        &args.env,
        &args.args,
        destination,
        starter,
    ))
}

/// What an install sends of `built`, the build of `package`, which `identity` names: its name
/// people see, the package's where its metadata names none, its version, and its icon where its
/// metadata names one.
fn install_build(
    identity: InstallIdentity,
    package: &cargo_metadata::Package,
    built: &BuiltProgram,
) -> Result<InstallBuild> {
    let read =
        |path: &Path| std::fs::read(path).with_context(|| format!("reading {}", path.display()));
    Ok(InstallBuild {
        id: identity.id,
        kind: identity.kind,
        name: identity.name.unwrap_or_else(|| package.name.to_string()),
        version: package.version.clone(),
        icon: identity.icon.as_deref().map(read).transpose()?,
        category: identity.category,
        file_associations: identity.file_associations,
        component: read(&built.component)?,
    })
}

/// Writes the pictures of the program's windows under target/waos/screenshots/<package>, or
/// <id> for one --id names, or where --out asks, as the WaOS running it answers.
fn screenshot(destination: &Destination, args: &ScreenshotArgs) -> Result<ExitCode> {
    let (id, name) = target_of(&args.target)?;
    let default = screenshots_folder(&name);
    on_this_thread(screenshot::take(
        destination,
        id,
        args.out.as_deref(),
        args.window,
        &default,
    ))
}

/// Gives `input` to the window the arguments choose, and waits for WaOS's answer.
fn give(destination: &Destination, window: &WindowArgs, input: Input) -> Result<ExitCode> {
    let id = target_id(&window.target)?;
    on_this_thread(input::give(destination, id, window.window, input))
}

/// Sizes the window the arguments choose, and waits for WaOS's answer.
fn resize_window(destination: &Destination, args: &ResizeArgs) -> Result<ExitCode> {
    let id = target_id(&args.window.target)?;
    on_this_thread(resize::resize(destination, id, args.window.window, args.to))
}

/// The program `target` names and how its output is named: the id given, by itself, or else the
/// chosen package's development install, by the package's name.
fn target_of(target: &Target) -> Result<(ProgramId, String)> {
    match &target.id {
        Some(id) => Ok((id.clone(), id.to_string())),
        None => {
            let metadata = workspace::metadata()?;
            let package = workspace::chosen_package(&metadata, target.program.package.as_deref())?;
            let id = WaosMetadata::id_of(package)?.development();
            Ok((id, package.name.to_string()))
        }
    }
}

/// The program `target` names: the id given, or else the development id of the chosen package's
/// program, its [package.metadata.waos] id followed by `.development`, read without building
/// anything: the program `waos install` installs and `waos run` runs.
fn target_id(target: &Target) -> Result<ProgramId> {
    Ok(target_of(target)?.0)
}

/// Where a screenshot of the program named `name` goes by default: `target/waos/screenshots/<name>`
/// in this directory's workspace, or `screenshots/<name>` here outside any.
fn screenshots_folder(name: &str) -> PathBuf {
    match workspace::metadata() {
        Ok(metadata) => workspace::waos_folder(&metadata)
            .join("screenshots")
            .join(name),
        Err(_) => PathBuf::from("screenshots").join(name),
    }
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

/// Packages the program, built with the features asked, and says where its package is.
fn package(program: &Program, features: &FeatureArgs) -> Result<ExitCode> {
    let packaged = package::package(program.package.as_deref(), &features.features())?;
    eprintln!("waos: listed in {}", packaged.index.display());
    println!("{}", packaged.package.display());
    Ok(ExitCode::SUCCESS)
}

/// Verifies the package, against the publisher named where one is, and says what it is.
fn verify_package(args: &VerifyArgs) -> Result<ExitCode> {
    let package = verify::verify(&args.file, args.publisher.as_ref())?;
    print!("{}", verify::description(&package));
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
fn install_skill(dir: Option<PathBuf>) -> Result<ExitCode> {
    let skills = match dir {
        Some(dir) => dir,
        None => skill::default_skills_folder()?,
    };
    let folder = skill::install(&skills, &on_this_thread(docs::load())?)?;
    eprintln!("waos: wrote the skill, SKILL.md and docs/");
    println!("{}", folder.display());
    Ok(ExitCode::SUCCESS)
}

/// Builds the program the arguments choose.
fn build_chosen(args: &BuildArgs) -> Result<BuiltProgram> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, args.program.package.as_deref())?;
    build::build(
        &metadata,
        package,
        args.profile(),
        &args.features.features(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_argv(argv: &[&str]) -> Result<Command, clap::Error> {
        Cli::try_parse_from(argv).map(|cli| cli.command)
    }

    fn parsed(arguments: &[&str]) -> Command {
        let mut argv = vec!["waos"];
        argv.extend_from_slice(arguments);
        parse_argv(&argv).unwrap_or_else(|error| panic!("{arguments:?}: {error}"))
    }

    fn refused(arguments: &[&str]) -> bool {
        let mut argv = vec!["waos"];
        argv.extend_from_slice(arguments);
        parse_argv(&argv).is_err()
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
        assert_eq!(shot.target.program.package.as_deref(), Some("notes"));
        assert_eq!(shot.window, Some(WindowId(2)));
        assert_eq!(shot.out.as_deref(), Some(Path::new("shots/")));

        let Command::Ui(UiArgs {
            command: UiCommand::Screenshot(cargo_shot),
        }) = parsed(&["ui", "screenshot", "--window", "3"])
        else {
            panic!("waos ui screenshot");
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
        assert_eq!(tap.window.target.program.package.as_deref(), Some("notes"));
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
        assert_eq!(
            resize.window.target.program.package.as_deref(),
            Some("notes")
        );
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
        assert_eq!(args.target.program.package.as_deref(), Some("notes"));
        assert!(args.command.is_none());

        let Command::Settings(SettingsArgs {
            command: Some(SettingsCommand::List(target)),
            ..
        }) = parsed(&["settings", "list", "-p", "notes"])
        else {
            panic!("settings list");
        };
        assert_eq!(target.program.package.as_deref(), Some("notes"));

        let Command::Settings(SettingsArgs {
            command: Some(SettingsCommand::Set(set)),
            ..
        }) = parsed(&["settings", "set", "camera", "allow", "-p", "notes"])
        else {
            panic!("settings set");
        };
        assert_eq!(set.row, ["camera", "allow"]);
        assert_eq!(set.target.program.package.as_deref(), Some("notes"));

        let Command::Settings(SettingsArgs {
            target: parent,
            command: Some(SettingsCommand::Set(set)),
        }) = parsed(&["settings", "-p", "notes", "set", "open-files", "/a", "/b"])
        else {
            panic!("settings -p set");
        };
        assert_eq!(
            set.target.or(&parent).program.package.as_deref(),
            Some("notes")
        );
        assert_eq!(set.row, ["open-files", "/a", "/b"]);

        assert!(refused(&["settings", "set", "camera"]));

        assert!(refused(&["set", "camera", "allow"]));

        let Command::Settings(SettingsArgs {
            target: parent,
            command: Some(SettingsCommand::Set(set)),
        }) = parsed(&[
            "settings",
            "--id",
            "dev.example.notes",
            "set",
            "camera",
            "deny",
        ])
        else {
            panic!("settings --id set");
        };
        assert_eq!(
            set.target.or(&parent).id.as_ref().map(ProgramId::as_str),
            Some("dev.example.notes"),
            "a published program's, by its id"
        );
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
        assert_eq!(summary.target.program.package.as_deref(), Some("notes"));

        let Command::Inspect(InspectArgs {
            command: InspectCommand::ActiveUses(target),
            ..
        }) = parsed(&["inspect", "active-uses", "-p", "notes"])
        else {
            panic!("inspect active-uses");
        };
        assert_eq!(target.program.package.as_deref(), Some("notes"));

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
    fn build_run_and_package_take_cargos_feature_flags() {
        let Command::Run(run) = parsed(&[
            "run",
            "--features",
            "online,update",
            "-F",
            "extra more",
            "--no-default-features",
        ]) else {
            panic!("run");
        };
        assert_eq!(
            run.build.features.features(),
            Features {
                chosen: ["online", "update", "extra", "more"]
                    .map(str::to_owned)
                    .to_vec(),
                all: false,
                without_default: true,
            }
        );

        let Command::Build(build) = parsed(&["build", "--all-features"]) else {
            panic!("build");
        };
        assert!(build.features.features().all);

        let Command::Package(package) = parsed(&["package", "--no-default-features"]) else {
            panic!("package");
        };
        assert!(package.features.features().without_default);

        let Command::Build(plain) = parsed(&["build"]) else {
            panic!("plain build");
        };
        assert_eq!(plain.features.features(), Features::default());
    }

    #[test]
    fn package_packages_a_program_or_verifies_a_file() {
        let Command::Package(args) = parsed(&["package", "-p", "notes"]) else {
            panic!("package -p");
        };
        assert_eq!(args.program.package.as_deref(), Some("notes"));
        assert!(args.command.is_none());

        let key = waos_package::SigningKey::generate().publisher();
        let hex = key.to_hex();
        let Command::Package(PackageArgs {
            command: Some(PackageCommand::Verify(verify)),
            ..
        }) = parsed(&["package", "verify", "notes.wapk", "--publisher", &hex])
        else {
            panic!("package verify");
        };
        assert_eq!(verify.file, Path::new("notes.wapk"));
        assert_eq!(verify.publisher, Some(key));

        assert!(refused(&["package", "verify"]));
        assert!(refused(&[
            "package",
            "verify",
            "notes.wapk",
            "--publisher",
            "E744 C079 1320 C328"
        ]));
        assert!(refused(&["package", "-p", "notes", "verify", "notes.wapk"]));
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
        let help = build_clap_command(None).render_help().to_string();
        for visible in [
            "new",
            "build",
            "install",
            "launch",
            "attach",
            "stop",
            "uninstall",
            "run",
            "package",
            "ui",
            "settings",
            "inspect",
            "ext",
            "docs",
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

    #[test]
    fn every_command_that_reaches_a_program_takes_an_id_or_the_package() {
        let Command::Install(install) = parsed(&["install", "-p", "notes", "--release"]) else {
            panic!("install");
        };
        assert_eq!(install.program.package.as_deref(), Some("notes"));
        assert!(install.release);

        let Command::Launch(launch) = parsed(&[
            "launch",
            "--id",
            "dev.example.notes.development",
            "--env",
            "RUST_LOG=debug",
            "--",
            "--port",
            "8080",
        ]) else {
            panic!("launch");
        };
        assert_eq!(
            launch.target.id.as_ref().map(ProgramId::as_str),
            Some("dev.example.notes.development")
        );
        assert_eq!(launch.env, [("RUST_LOG".to_owned(), "debug".to_owned())]);
        assert_eq!(launch.args, ["--port", "8080"]);

        let Command::Launch(launch) = parsed(&["launch", "-p", "notes"]) else {
            panic!("launch in a project");
        };
        assert_eq!(launch.target.id, None);
        assert_eq!(launch.target.program.package.as_deref(), Some("notes"));

        let id = ["--id", "dev.example.notes"];
        for command in [
            &["attach"][..],
            &["stop"],
            &["uninstall"],
            &["launch"],
            &["ui", "screenshot"],
            &["ui", "tap", "--at", "1,2"],
            &["ui", "type", "text"],
            &["ui", "key", "enter"],
            &["ui", "resize", "--to", "390x844"],
            &["settings"],
            &["settings", "set", "camera", "allow"],
            &["inspect", "summary"],
            &["inspect", "trace", "network"],
        ] {
            let mut argv = command.to_vec();
            argv.extend(id);
            parsed(&argv);
            let mut both = argv.clone();
            both.extend(["-p", "notes"]);
            assert!(refused(&both), "{command:?}: an id or a package, not both");
            let mut bad = command.to_vec();
            bad.extend(["--id", "triangle"]);
            assert!(refused(&bad), "{command:?}: not an id");
        }
        let (Command::Attach(target) | Command::Stop(target) | Command::Uninstall(target)) =
            parsed(&["stop", "--id", "dev.example.notes"])
        else {
            panic!("stop");
        };
        assert_eq!(
            target.id.as_ref().map(ProgramId::as_str),
            Some("dev.example.notes")
        );
        assert!(refused(&["stop", "dev.example.notes"]), "by --id alone");
        for command in ["install", "launch", "attach", "stop", "uninstall"] {
            assert!(is_cli_command(command), "{command}");
        }
    }

    #[test]
    fn ext_relays_over_stdio_for_an_id_or_the_package_and_lists() {
        let Command::Ext(ext) = parsed(&["ext", "--id", "dev.example.notes", "--stdio"]) else {
            panic!("ext --stdio");
        };
        assert!(ext.stdio && ext.command.is_none());
        assert_eq!(
            ext.target.id.as_ref().map(ProgramId::as_str),
            Some("dev.example.notes")
        );
        let Command::Ext(ext) = parsed(&["ext", "--stdio", "-p", "notes"]) else {
            panic!("ext -p --stdio");
        };
        assert_eq!(ext.target.program.package.as_deref(), Some("notes"));
        let Command::Ext(ExtArgs {
            command: Some(ExtCommand::List { json }),
            ..
        }) = parsed(&["ext", "list", "--json"])
        else {
            panic!("ext list --json");
        };
        assert!(json);
        assert!(refused(&["ext", "--stdio", "list"]));
        assert!(is_cli_command("ext"));
    }
}
