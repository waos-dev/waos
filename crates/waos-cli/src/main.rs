//! `cargo waos`: writes a new program, builds a program as a component, runs it in the WaOS on
//! this machine, and packages it as a signed `.wapk` for a package source; while it runs there,
//! takes a screenshot of its windows, gives them input and sizes them, so an agent building the
//! program sees it and uses it, at a phone's width among others (the design's decision 60); and reads and sets the program's settings, a set
//! answer in place of each question WaOS would ask its user, so a run never waits for a person.
//!
//! The invariant: every command is about a program and the WaOS it runs in. A run goes to the
//! WaOS already running, starting the installed app when none answers, never to an engine of
//! this process's own (the design's decision 29); a program's settings go to the WaOS keeping
//! them, started the same way; a screenshot or an input goes to the WaOS running the program,
//! never starting one; and nothing is ever installed on a device: a
//! phone runs programs in the WaOS app already on it, and the WaOS app itself is built with
//! `cargo waft`.

mod asked;
mod asking;
mod build;
mod connecting;
mod icon_rule;
// Linux's alone; its tests run everywhere.
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod desktop_entries;
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
mod unreached;
mod waos_app;
mod workspace;

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::{Args, Parser, Subcommand};
use waos_dev_protocol::Input;
use waos_package::ProgramId;

use crate::build::{BuiltProgram, Profile};
use crate::input::Pair;
use crate::metadata::WaosMetadata;
use crate::new::Template;
use crate::resize::Points;

/// What cargo hands this binary: its own name, then `waos`, then the arguments.
#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    #[command(
        name = "waos",
        about = "cargo waos: writes a new program, and builds a program as a component, runs it in WaOS and packages it, and takes screenshots of its windows, gives them input and sizes them while it runs",
        version
    )]
    Waos(WaosArgs),
}

#[derive(Args)]
struct WaosArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Writes a new program in a folder of its name: a Waft app that `cargo waos run` runs and
    /// `cargo waos package` packages as it is.
    ///
    /// Inside a cargo workspace, the program is made a member of it as `cargo new` makes one: it
    /// inherits the workspace's package keys, lints and the dependencies the workspace lists, and
    /// is added to the workspace's members unless a pattern there takes it in already.
    New(NewArgs),
    /// Builds the program as a component, for wasm32-wasip3, and puts it at
    /// target/waos/<profile>/<package>.wasm.
    Build(BuildArgs),
    /// Builds the program and runs it in the WaOS on this machine, starting WaOS.app when none
    /// answers, as the program the id in its [package.metadata.waos] names, with that program's
    /// settings and folders. What it writes comes here as it writes it; Ctrl-C stops it; the exit
    /// status is 0 for a run that returned success and 1 otherwise.
    Run(BuildArgs),
    /// Builds the program's release component and writes it as a package signed with your
    /// signing key, at target/waos/<id>-<version>.wapk, listed in target/waos/index.json.
    /// The id, name and icon are the package's [package.metadata.waos]; the version is its own.
    Package(Program),
    /// Writes the last picture of each window of the program `cargo waos run` runs, as
    /// target/waos/screenshots/<package>/window-<n>.png, and prints each with its window's
    /// number, title and size in pixels. Windows are numbered from 1 in the order they opened.
    Screenshot(Program),
    /// Taps a window of the program `cargo waos run` runs: a press and release of the mouse there.
    /// Returns once the window has shown its next picture.
    ///
    /// The press and release are made by WaOS, with a mouse of its own, and handed to the program
    /// as WaOS hands over its user's input, so the program hears them as a user's; the system
    /// never sees them, and the real pointer does not move.
    Tap(TapArgs),
    /// Types text into what a window of the program `cargo waos run` runs edits, as an input
    /// method enters it. Returns once the window has shown its next picture.
    ///
    /// The text is committed by WaOS, as an input method commits its text, and handed to the
    /// program as WaOS hands over its user's input, so the program hears it as a user's; the
    /// system never sees it.
    Type(TypeArgs),
    /// Presses a key, or a chord, in a window of the program `cargo waos run` runs: enter,
    /// escape, tab, a, cmd+s, shift+tab. Returns once the window has shown its next picture.
    ///
    /// The key events are made by WaOS, a US keyboard's, and handed to the program as WaOS hands
    /// over its user's input, so the program hears them as a user's; the system and the window's
    /// menu bar never see them. A key the program leaves goes nowhere, where a real one would go
    /// on to the menu bar, choosing the item it is the shortcut of (Quit, Copy, a File > Save of
    /// the program's): a gap known for now.
    Key(KeyArgs),
    /// Scrolls a window of the program `cargo waos run` runs at a point, as a mouse wheel does.
    /// Returns once the window has shown its next picture.
    ///
    /// The scroll is made by WaOS, with a mouse of its own, and handed to the program as WaOS
    /// hands over its user's input, so the program hears it as a user's; the system never sees
    /// it, and the real pointer does not move.
    Scroll(ScrollArgs),
    /// Presses a window of the program `cargo waos run` runs at a point and holds it: the mouse's
    /// button down there, held for --duration, then up. Returns once the window has shown its
    /// next picture after the release.
    ///
    /// The press and release are made by WaOS, with a mouse of its own, and handed to the program
    /// as WaOS hands over its user's input, so the program hears them as a user's; the system
    /// never sees them, and the real pointer does not move.
    Press(PressArgs),
    /// Drags in a window of the program `cargo waos run` runs: the mouse's button down at --from,
    /// held there for --hold, moved to --to in --steps equal moves spread over --duration, and up
    /// at --to. Returns once the window has shown its next picture after the release.
    ///
    /// The press, the moves and the release are made by WaOS, with a mouse of its own, and handed
    /// to the program as WaOS hands over its user's input, so the program hears them as a user's;
    /// the system never sees them, and the real pointer does not move.
    Drag(DragArgs),
    /// Sizes a window of the program `cargo waos run` runs, so what the program draws in it is
    /// --to, its top-left where it is, and prints the size it took and its scale. Returns once
    /// the window has shown the program's picture at that size.
    ///
    /// The size is in logical points, which the program lays out in: a screenshot's pixels, and
    /// the points tap and the other input commands take, are points times the window's scale, 2
    /// on a Retina screen, so --to 390x844 is a screenshot of 780x1688. The window is sized as the
    /// program's own set-frame sizes it, so its window system may keep it within the screen or
    /// the window's limits, or leave it as it is, as the size printed says; a tab of a phone's
    /// deck is in no window, and is refused.
    Resize(ResizeArgs),
    /// Prints the program's settings, one row each, as its settings page in WaOS shows them: its
    /// questions' answers, its switches, its answers to link schemes and roles, and the answers
    /// set for its pickers and file dialogs, with the machines and outputs they can be set to now.
    ///
    /// The settings are the program's own, kept by WaOS, for a program a `cargo waos run` on this
    /// machine runs or has run; WaOS is started when none answers.
    Settings(Program),
    /// Sets one row of the program's settings, as its user would on its settings page in WaOS,
    /// and prints the rows after it. A row on ask is asked of the user when the program asks;
    /// one set is answered without asking, every run, installed or not.
    ///
    ///   camera | microphone                          allow | deny | simulated | ask
    ///   notifications                                allow | deny | ask
    ///   simulated-microphone                         tone | silence | global
    ///   network | local-network | clipboard-read | clipboard-write   on | off | global
    ///   link-scheme <scheme>                         allow | deny | ask
    ///   role.inspector                               allow | deny | ask
    ///   machine | audio-output                       <id> | ask
    ///   open-file | open-folder | save-file          <path> | ask
    ///   open-files                                   <path>... | ask
    ///
    /// A path is made whole against this directory, and must be one the dialog could have
    /// chosen: a file to open that exists, a folder that exists, a file to save in a folder that
    /// exists. A machine or an output is named by its id, as `cargo waos settings` lists them.
    Set(SetArgs),
    /// Prints what WaOS knows of what the program does with what it was allowed, as JSON lines of
    /// wasdk:inspect's values, one value a line: `summary`, its audit summary, a day a line,
    /// newest first; `active-uses`, what it uses now, each use that lasts and each file it has
    /// open, on one line, then each start and end, a line each, a `resync` of all of it where the
    /// command fell behind; or its live trace of the capabilities named, each event as it comes;
    /// the last two until Ctrl-C or the program's end: network, files, clipboard, camera, microphone, machines, permissions,
    /// links, notifications, output, or `all` of them.
    ///
    /// A summary has a day only where the program used something WaOS guards that day; its output
    /// is not one. The summary is read for a program `cargo waos run` on this machine runs or has
    /// run, the rest for one it runs; WaOS is never started.
    Inspect(InspectArgs),
}

#[derive(Args)]
struct InspectArgs {
    #[command(flatten)]
    program: Program,
    /// What to inspect: summary, active-uses, all, or the capabilities to trace.
    #[arg(required = true, num_args = 1..)]
    what: Vec<String>,
    /// How many of the summary's days, newest first.
    #[arg(long, default_value_t = 7)]
    days: u32,
}

#[derive(Args)]
struct SetArgs {
    #[command(flatten)]
    program: Program,
    /// The row, then its value: `camera allow`, `save-file /tmp/out.pdf`, `machine ask`.
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
#[derive(Args)]
struct Program {
    /// The program's package; without it, the package of this directory.
    #[arg(short, long, value_name = "PACKAGE")]
    package: Option<String>,
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

/// Which program, and which of its windows.
#[derive(Args)]
struct WindowArgs {
    #[command(flatten)]
    program: Program,
    /// The window, numbered from 1 as `cargo waos screenshot` lists them.
    #[arg(long, default_value_t = 1)]
    window: u32,
}

#[derive(Args)]
struct TapArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Where, in the window's pixels from its top-left, as a screenshot has them: <x>,<y>.
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
    /// Where, in the window's pixels from its top-left, as a screenshot has them: <x>,<y>.
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
    /// Where, in the window's pixels from its top-left, as a screenshot has them: <x>,<y>.
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
    /// Where the button goes down, in the window's pixels from its top-left: <x>,<y>.
    #[arg(long, value_name = "X,Y", allow_hyphen_values = true)]
    from: Pair,
    /// Where the button comes up, in the window's pixels from its top-left: <x>,<y>.
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
    /// The size of what the window shows, in logical points: <width>x<height>, such as 390x844.
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

fn main() -> ExitCode {
    let Cargo::Waos(args) = Cargo::parse();
    let done = match args.command {
        Command::New(new) => new_program(&new),
        Command::Build(build) => build_only(&build),
        Command::Run(build) => build_and_run(&build),
        Command::Package(program) => package(&program),
        Command::Screenshot(program) => screenshot(&program),
        Command::Tap(tap) => give(
            &tap.window,
            Input::Tap {
                x: tap.at.0,
                y: tap.at.1,
            },
        ),
        Command::Type(typed) => give(&typed.window, Input::Text(typed.text)),
        Command::Key(key) => give(&key.window, Input::Key(key.chord)),
        Command::Scroll(scroll) => give(
            &scroll.window,
            Input::Scroll {
                x: scroll.at.0,
                y: scroll.at.1,
                dx: scroll.by.0,
                dy: scroll.by.1,
            },
        ),
        Command::Press(press) => give(
            &press.window,
            Input::Press {
                x: press.at.0,
                y: press.at.1,
                duration: Duration::from_millis(press.duration),
            },
        ),
        Command::Drag(drag) => give(
            &drag.window,
            Input::Drag {
                from: (drag.from.0, drag.from.1),
                to: (drag.to.0, drag.to.1),
                hold: Duration::from_millis(drag.hold),
                steps: drag.steps,
                duration: Duration::from_millis(drag.duration),
            },
        ),
        Command::Resize(resize) => resize_window(&resize),
        Command::Settings(program) => {
            program_id(&program).and_then(|id| on_this_thread(settings::show(id)))
        }
        Command::Set(set) => {
            program_id(&set.program).and_then(|id| on_this_thread(settings::set(id, &set.row)))
        }
        Command::Inspect(inspect) => inspect::inspection_of(&inspect.what, inspect.days)
            .and_then(|asked| Ok((program_id(&inspect.program)?, asked)))
            .and_then(|(id, asked)| on_this_thread(inspect::inspect(id, asked))),
    };
    done.unwrap_or_else(|error| {
        eprintln!("cargo waos: {error:#}");
        ExitCode::FAILURE
    })
}

/// Writes a new program in a folder of this directory's, says which workspace's members it was
/// added to and, as `cargo new` warns, when its workspace cannot load with it, and says how to
/// run it.
fn new_program(args: &NewArgs) -> Result<ExitCode> {
    let created = new::create(&args.name, args.template, Path::new("."))?;
    if let Some(root) = &created.added_to {
        eprintln!(
            "cargo waos: added `{}` as member of workspace at `{}`",
            args.name,
            root.display()
        );
    }
    if let Some(problem) = new::workspace_problem(&created.folder) {
        eprintln!(
            "cargo waos: warning: compiling this new program may not work due to invalid workspace configuration: {problem:#}"
        );
    }
    println!("created {}", created.folder.display());
    println!("  cd {} && cargo waos run", args.name);
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
fn build_and_run(args: &BuildArgs) -> Result<ExitCode> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, args.program.package.as_deref())?;
    let id = WaosMetadata::id_of(package)?;
    let built = build::build(&metadata, package, args.profile())?;
    on_this_thread(run::run(id, &built))
}

/// Writes the pictures of the program's windows under target/waos/screenshots/<package>, as the
/// WaOS running it answers.
fn screenshot(program: &Program) -> Result<ExitCode> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, program.package.as_deref())?;
    let id = WaosMetadata::id_of(package)?;
    let folder = workspace::waos_folder(&metadata)
        .join("screenshots")
        .join(package.name.as_str());
    on_this_thread(screenshot::take(id, &folder))
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

/// What `connection` answers, on a runtime of this thread's, for a command's connection to WaOS.
fn on_this_thread(connection: impl Future<Output = Result<ExitCode>>) -> Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("a runtime for the connection")?;
    runtime.block_on(connection)
}

/// Packages the program and says where its package is.
fn package(program: &Program) -> Result<ExitCode> {
    let packaged = package::package(program.package.as_deref())?;
    eprintln!("cargo waos: listed in {}", packaged.index.display());
    println!("{}", packaged.package.display());
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
    use clap::CommandFactory;

    use super::*;

    fn parsed(arguments: &[&str]) -> Command {
        let Cargo::Waos(args) = Cargo::try_parse_from(["cargo", "waos"].iter().chain(arguments))
            .unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
        args.command
    }

    fn refused(arguments: &[&str]) -> bool {
        Cargo::try_parse_from(["cargo", "waos"].iter().chain(arguments)).is_err()
    }

    #[test]
    fn a_screenshot_names_its_package_or_none() {
        let Command::Screenshot(program) = parsed(&["screenshot"]) else {
            panic!("a screenshot");
        };
        assert_eq!(program.package, None);
        let Command::Screenshot(program) = parsed(&["screenshot", "-p", "notes"]) else {
            panic!("a screenshot");
        };
        assert_eq!(program.package.as_deref(), Some("notes"));
    }

    #[test]
    fn a_tap_is_at_a_point_in_the_first_window_unless_another_is_named() {
        let Command::Tap(tap) = parsed(&["tap", "--at", "120,48"]) else {
            panic!("a tap");
        };
        assert_eq!((tap.at, tap.window.window), (Pair(120.0, 48.0), 1));
        let Command::Tap(tap) = parsed(&["tap", "--at", "1.5,2", "--window", "2", "-p", "notes"])
        else {
            panic!("a tap");
        };
        assert_eq!((tap.at, tap.window.window), (Pair(1.5, 2.0), 2));
        assert_eq!(tap.window.program.package.as_deref(), Some("notes"));
        assert!(refused(&["tap"]), "a tap needs a point");
        assert!(refused(&["tap", "--at", "120"]));
    }

    #[test]
    fn a_scroll_is_at_a_point_by_an_amount_either_signed() {
        let Command::Scroll(scroll) = parsed(&["scroll", "--at", "10,20", "--by", "-5,-200"])
        else {
            panic!("a scroll");
        };
        assert_eq!(
            (scroll.at, scroll.by),
            (Pair(10.0, 20.0), Pair(-5.0, -200.0))
        );
        assert!(
            refused(&["scroll", "--at", "10,20"]),
            "a scroll needs an amount"
        );
    }

    #[test]
    fn each_inputs_help_says_waos_makes_it_and_the_system_never_sees_it() {
        let mut cargo = Cargo::command();
        let waos = cargo.find_subcommand_mut("waos").expect("cargo waos");
        for input in ["tap", "type", "key", "scroll", "press", "drag"] {
            let help = waos
                .find_subcommand_mut(input)
                .expect("the input's command")
                .render_long_help()
                .to_string();
            assert!(help.contains("by WaOS"), "{input}: {help}");
            assert!(help.contains("never see"), "{input}: {help}");
        }
        let key = waos.find_subcommand_mut("key").unwrap().render_long_help();
        assert!(key.to_string().contains("menu bar never see them"), "{key}");
        assert!(key.to_string().contains("goes nowhere"), "{key}");
    }

    #[test]
    fn a_press_is_held_a_second_unless_it_says_otherwise() {
        let Command::Press(press) = parsed(&["press", "--at", "10,20"]) else {
            panic!("a press");
        };
        assert_eq!((press.at, press.duration), (Pair(10.0, 20.0), 1000));
        let Command::Press(press) = parsed(&["press", "--at", "10,20", "--duration", "250"]) else {
            panic!("a press");
        };
        assert_eq!(press.duration, 250);
        assert!(refused(&["press"]), "a press needs a point");
    }

    #[test]
    fn a_drag_goes_from_a_point_to_another_in_ten_moves_over_300_ms_by_default() {
        let Command::Drag(drag) = parsed(&["drag", "--from", "10,20", "--to", "-5,400"]) else {
            panic!("a drag");
        };
        assert_eq!((drag.from, drag.to), (Pair(10.0, 20.0), Pair(-5.0, 400.0)));
        assert_eq!((drag.steps, drag.duration, drag.hold), (10, 300, 0));
        let Command::Drag(drag) = parsed(&[
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
        ]) else {
            panic!("a drag");
        };
        assert_eq!((drag.steps, drag.duration, drag.hold), (1, 0, 600));
        assert_eq!(drag.window.window, 2);
        assert!(refused(&["drag", "--from", "1,2"]), "a drag needs its end");
        assert!(
            refused(&["drag", "--from", "1,2", "--to", "3,4", "--steps", "0"]),
            "a drag moves at least once"
        );
    }

    #[test]
    fn a_resize_names_its_size_in_points_and_its_window() {
        let Command::Resize(resize) = parsed(&["resize", "--to", "390x844"]) else {
            panic!("a resize");
        };
        assert_eq!(
            (resize.to, resize.window.window),
            (
                Points {
                    width: 390.0,
                    height: 844.0
                },
                1
            )
        );
        let Command::Resize(resize) =
            parsed(&["resize", "--to", "320x568", "--window", "2", "-p", "notes"])
        else {
            panic!("a resize");
        };
        assert_eq!(resize.window.window, 2);
        assert_eq!(resize.window.program.package.as_deref(), Some("notes"));
        assert!(refused(&["resize"]), "a resize needs a size");
        assert!(
            refused(&["resize", "--to", "390,844"]),
            "written WIDTHxHEIGHT"
        );
    }

    #[test]
    fn a_set_names_its_row_then_its_value_or_values() {
        let Command::Set(set) = parsed(&["set", "camera", "allow"]) else {
            panic!("a set");
        };
        assert_eq!(set.row, ["camera", "allow"]);
        let Command::Set(set) = parsed(&["set", "open-files", "/a", "/b", "-p", "notes"]) else {
            panic!("a set");
        };
        assert_eq!(set.row, ["open-files", "/a", "/b"]);
        assert_eq!(set.program.package.as_deref(), Some("notes"));
        assert!(refused(&["set", "camera"]), "a row needs its value");
        assert!(matches!(parsed(&["settings"]), Command::Settings(_)));
    }

    #[test]
    fn text_and_a_chord_are_taken_as_written() {
        let Command::Type(typed) = parsed(&["type", "-5 eggs, milk"]) else {
            panic!("text");
        };
        assert_eq!(typed.text, "-5 eggs, milk");
        let Command::Key(key) = parsed(&["key", "cmd+s", "--window", "3"]) else {
            panic!("a key");
        };
        assert_eq!((key.chord.as_str(), key.window.window), ("cmd+s", 3));
        let Command::Key(key) = parsed(&["key", "-"]) else {
            panic!("a key");
        };
        assert_eq!(key.chord, "-");
    }
}
