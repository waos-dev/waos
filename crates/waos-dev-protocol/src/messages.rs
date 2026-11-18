//! What `cargo waos` says to a WaOS, and what a WaOS says back, about one run; what `waos open`
//! says, files to open with a program, and the answer; and what `cargo waos` asks of the windows of
//! a program a run of its own runs, a screenshot or an input, and the answer (the design's decision
//! 60); and a program's rows of settings, read or set, for a program a run has run, and the
//! answer; a size for one of a program's windows, and the size it took; and an inspection of a
//! program, its audit summary, what it holds or its live trace, answered as JSON of
//! `wasdk:inspect`'s values, so a reader needs no types of its own.
//!
//! The invariant: a variant's place and a field's place is its encoding (the crate's doc). A
//! program's output is bytes, not lines, so a terminal shows what the program wrote as it wrote it,
//! a prompt without its newline among it. Bytes are encoded as a string of bytes (`serde_bytes`),
//! which postcard writes as it writes a `Vec<u8>`, its length and then its bytes, and reads in one
//! copy rather than one call a byte: a debug build otherwise took about 0.2 s to send forever's 7
//! MB. A picture is a PNG's bytes, encoded the same way. Text typed into a program is left out of
//! what a message's `Debug` writes, since messages are traced and it may be a password.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use waos_package::ProgramId;

use crate::settings_rows::{Asked, Row, SettingRow};

/// What `cargo waos` says to a WaOS.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum ToWaOS {
    /// Run this component as the program `id` names, from its `[package.metadata.waos]`, with
    /// that program's settings and folders, installed or not, and the variables `variables` names,
    /// each its name and value, which this run alone adds to the program's environment (the
    /// design's decision 68); the program running under that id is stopped first. Nothing of the
    /// client's own environment is sent, and WaOS's own variables win where both name one. The
    /// component is not in this message: it follows in `Component` frames, `length` bytes
    /// together whose SHA-256 is `digest`, so a component larger than one frame
    /// goes over too.
    Run {
        id: ProgramId,
        /// The component's bytes, which the `Component` frames that follow carry together.
        length: u64,
        /// The component's SHA-256 (`component_digest`), checked once its bytes are all there.
        digest: [u8; 32],
        variables: Vec<(String, String)>,
    },
    /// Open these files, absolute paths on this machine, with the program `id` names, as its
    /// user would open them with it: the program running under that id, or the installed one,
    /// started. Answered `Opened` or `Refused`, after which the connection ends.
    Open { id: ProgramId, files: Vec<PathBuf> },
    /// The last picture of each window of the program `id` names, while a run of `cargo waos
    /// run` runs it. Answered `Screenshots`, or `Unreached`, after which the connection ends.
    Screenshot { id: ProgramId },
    /// `input` to the program `id` names, as its user would give it, in its window numbered
    /// `window`, from 1 in the order `Screenshots` lists them, while a run of `cargo waos run` runs
    /// it. Answered `InputDone` once the window shows its next picture, or `Unreached`, after
    /// which the connection ends.
    Input {
        id: ProgramId,
        window: u32,
        input: Input,
    },
    /// The rows of the settings of the program `id` names, while a run of `cargo waos run` on this
    /// machine runs it or once one has. Answered `Settings`, or `Unreached`, after which the
    /// connection ends.
    Settings { id: ProgramId },
    /// Sets `row` among the settings of the program `id` names, as its user would on its settings
    /// page, for the same programs. Answered `Settings`, the rows after the change, `Refused` with
    /// why the row's value names nothing now, or `Unreached`, after which the connection ends.
    Set { id: ProgramId, row: Row },
    /// Sizes the window numbered `window` of the program `id` names, as `Input` numbers it, so the
    /// view in it is `width` by `height` logical points, its top-left where it is: as the
    /// program's own `set-frame` sizes it, while a run of `cargo waos run` runs it. Answered
    /// `Resized` once the view reports its size, or `Unreached`, after which the connection ends.
    Resize {
        id: ProgramId,
        window: u32,
        width: f64,
        height: f64,
    },
    /// What `asked` asks of the program `id` names (the inspector proposal's protocol): its audit
    /// summary, for a program a run of `cargo waos run` on this machine runs or has run; or what it
    /// uses now, or its live trace, while a run runs it. Answered `Inspected`, once for a summary,
    /// after which the connection ends; for what it uses, first with what is active, then with
    /// each change; and for a trace with each event as it comes; the last two until the
    /// connection closes or the program ends; or `Unreached`.
    Inspect { id: ProgramId, asked: Inspection },
    /// Part of the component a run's `Run` announced: at most [`CHUNK_BYTES`](crate::CHUNK_BYTES)
    /// of it, the parts in order until the announced length is reached. Only a run's connection
    /// sends these, after its `Run`.
    Component {
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
}

/// What an inspection asks of a program.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Inspection {
    /// Its audit summary's days, newest first, `days` at most.
    Summary { days: u32 },
    /// What it uses now, each use that lasts and each file it has open, then each change, live.
    ActiveUses,
    /// Its uses of each of `traced` from now on, live.
    Trace(Vec<TracedCapability>),
}

/// A capability whose uses a trace hears: one of `wasdk:inspect`'s streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TracedCapability {
    Network,
    Files,
    Clipboard,
    Camera,
    Microphone,
    Machines,
    Permissions,
    Links,
    Notifications,
    Output,
}

impl TracedCapability {
    pub const ALL: [TracedCapability; 10] = [
        TracedCapability::Network,
        TracedCapability::Files,
        TracedCapability::Clipboard,
        TracedCapability::Camera,
        TracedCapability::Microphone,
        TracedCapability::Machines,
        TracedCapability::Permissions,
        TracedCapability::Links,
        TracedCapability::Notifications,
        TracedCapability::Output,
    ];

    /// Its name, as `cargo waos inspect` takes it.
    pub fn key(self) -> &'static str {
        match self {
            TracedCapability::Network => "network",
            TracedCapability::Files => "files",
            TracedCapability::Clipboard => "clipboard",
            TracedCapability::Camera => "camera",
            TracedCapability::Microphone => "microphone",
            TracedCapability::Machines => "machines",
            TracedCapability::Permissions => "permissions",
            TracedCapability::Links => "links",
            TracedCapability::Notifications => "notifications",
            TracedCapability::Output => "output",
        }
    }
}

/// Input to a program's window, as its user would give it, which WaOS simulates: it makes the
/// events itself and hands them to the program as it hands over the user's, so the system and the
/// window's menu bar never see them (the design's decision 64). A point is in the window's
/// physical pixels, a screenshot's, from its top-left.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Input {
    /// A press and release at a point: a mouse's primary button, on a desktop.
    Tap { x: f64, y: f64 },
    /// Text, as typing it enters it into what the window edits.
    Text(String),
    /// A key, or a chord of keys pressed together, by name: `enter`, `escape`, `cmd+s`.
    Key(String),
    /// A scroll at a point, by `dx` and `dy` physical pixels, a mouse wheel's: a positive `dy`
    /// scrolls towards the content's end, down.
    Scroll { x: f64, y: f64, dx: f64, dy: f64 },
    /// A press at a point held for `duration`, then released there: a mouse's primary button.
    Press { x: f64, y: f64, duration: Duration },
    /// A drag with a mouse's primary button: pressed at `from`, held there for `hold`, moved in
    /// `steps` equal moves along the line to `to`, one every `duration / steps`, and released at
    /// `to`.
    Drag {
        from: (f64, f64),
        to: (f64, f64),
        hold: Duration,
        steps: u32,
        duration: Duration,
    },
}

/// What a WaOS says back, about the run, the opening, the screenshot, the input, the settings or
/// the size the connection asked for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FromWaOS {
    Started,
    /// Bytes the program wrote, as it wrote them: a terminal shows them as they come.
    Output {
        stream: OutputStream,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    Ended(RunEnd),
    /// It could not be loaded or run, or the files could not be opened with it, and why.
    Refused(String),
    /// The files were handed to the program, which hears them once it listens.
    Opened,
    /// The last picture of each of the program's windows, in the order they opened.
    Screenshots(Vec<WindowPicture>),
    /// The program heard the input. `in_sight` says whether its window was in sight when the
    /// input was given, a run of `cargo waos run` counting its windows in sight while it is
    /// attached; `new_picture` says whether the window then showed a picture the program drew
    /// after it, within the time WaOS waits for one: none comes for an input that changed
    /// nothing, nor while the window is out of sight.
    InputDone {
        in_sight: bool,
        new_picture: bool,
    },
    /// The screenshot or the input reached no window, and why.
    Unreached(Unreached),
    /// WaOS compiles the component of the run, which it does before it starts it: `Started`, or
    /// `Refused`, follows once it has, minutes later for a large debug build.
    Compiling,
    /// The rows of the program's settings, in the order its settings page shows them.
    Settings(Vec<SettingRow>),
    /// WaOS asks the run's program's user something now, and the program waits for the answer: a
    /// row on Ask, which `cargo waos set` answers ahead next time.
    Asking(Asked),
    /// The window's view is `width` by `height` physical pixels at `scale` physical pixels a
    /// logical point, as it reported itself after the resize: the size asked for, or the one the
    /// window system clamped it to, or the size it had where the window system left it as it
    /// was. `new_picture` says whether the window then showed a picture the program drew at that
    /// size, as `InputDone`'s does.
    Resized {
        width: u32,
        height: u32,
        scale: f64,
        new_picture: bool,
    },
    /// What an inspection asked for, as JSON of `wasdk:inspect`'s values, one value a line: the
    /// summary's days, newest first, what the program holds, or one event of its trace.
    Inspected(Vec<String>),
}

/// One of a program's windows, as its user sees it: what `cargo waos screenshot` writes.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPicture {
    /// The window's number, from 1 in the order the program's windows opened: what an input's
    /// `window` names.
    pub window: u32,
    /// The title the program gave the window; empty for one it left untitled, which shows the
    /// program's name, and for a view in no window.
    pub title: String,
    /// The size of the window's picture, its view's, in physical pixels: what an input's point is
    /// in, from the top-left.
    pub width: u32,
    pub height: u32,
    /// Whether the window is in sight. One minimized or hidden shows no new picture until it is in
    /// sight again, and keeps showing its last; one other windows cover is in sight while a run of
    /// `cargo waos run` runs its program.
    pub in_sight: bool,
    /// What the window shows, as a PNG: the last picture the program presented in it over what
    /// each composition layer beneath it shows now, a player's video or a camera's picture among
    /// them; none before the program presented a picture.
    #[serde(with = "serde_bytes")]
    pub png: Option<Vec<u8>>,
    /// What WaOS asks the program's user over the window now, which an input does not reach the
    /// program through.
    pub asking: Option<Asked>,
}

/// Why a screenshot or an input reached no window of the program.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Unreached {
    /// No run of `cargo waos run` runs the program: nothing runs under its id, or the program
    /// running was opened otherwise, from WaOS's library, which no tool reads or drives.
    NotRunByCargoWaos,
    /// The program has no window numbered `window`: it has `windows`.
    NoWindow { window: u32, windows: u32 },
    /// The point `x`, `y` is outside the window, which is `width` by `height` physical pixels at
    /// `scale` physical pixels a logical point, as a screenshot's are.
    OutsideWindow {
        width: u32,
        height: u32,
        x: f64,
        y: f64,
        scale: f64,
    },
    /// Nothing in the window is being edited, so text typed there has nowhere to go.
    NotEditing,
    /// No key has this name, or the chord names one no key has.
    UnknownKey(String),
    /// No run of `cargo waos run` on this machine runs the program or has run it, so its settings
    /// are none of a tool's.
    NeverRunByCargoWaos,
    /// WaOS asks the program's user this over the program's windows, so the input was not given:
    /// the user's answer comes first.
    Asking(Asked),
    /// The program's view numbered `window` is in no window, as a phone's deck shows a tab: its
    /// size is the platform's, which no tool sets.
    InNoWindow { window: u32 },
}

/// Which of a program's two output streams some bytes came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// How the run ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunEnd {
    /// Its `main` returned; `success` is false for a run that reported failure.
    Returned { success: bool },
    /// It trapped, or could not run: the error.
    Failed(String),
    /// WaOS stopped it, as when the app quits.
    Stopped,
    /// A later run of the same name took its place.
    Replaced,
}

/// A component is megabytes, typed text may be a password, and a variable's value may be a
/// secret: an event that prints the message says how many bytes and characters, and a variable's
/// name, where its value is not said anywhere.
impl fmt::Debug for ToWaOS {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToWaOS::Run {
                id,
                length,
                digest,
                variables,
            } => {
                let names: Vec<&str> = variables.iter().map(|(name, _)| name.as_str()).collect();
                f.debug_struct("Run")
                    .field("id", &format_args!("{id}"))
                    .field("component", &format_args!("{length} bytes"))
                    .field("digest", &format_args!("{}", said_digest(digest)))
                    .field("variables", &format_args!("{names:?}"))
                    .finish()
            }
            ToWaOS::Open { id, files } => f
                .debug_struct("Open")
                .field("id", &format_args!("{id}"))
                .field("files", files)
                .finish(),
            ToWaOS::Screenshot { id } => f
                .debug_struct("Screenshot")
                .field("id", &format_args!("{id}"))
                .finish(),
            ToWaOS::Input { id, window, input } => f
                .debug_struct("Input")
                .field("id", &format_args!("{id}"))
                .field("window", window)
                .field("input", input)
                .finish(),
            ToWaOS::Settings { id } => f
                .debug_struct("Settings")
                .field("id", &format_args!("{id}"))
                .finish(),
            ToWaOS::Set { id, row } => f
                .debug_struct("Set")
                .field("id", &format_args!("{id}"))
                .field("row", row)
                .finish(),
            ToWaOS::Resize {
                id,
                window,
                width,
                height,
            } => f
                .debug_struct("Resize")
                .field("id", &format_args!("{id}"))
                .field("window", window)
                .field("width", width)
                .field("height", height)
                .finish(),
            ToWaOS::Inspect { id, asked } => f
                .debug_struct("Inspect")
                .field("id", &format_args!("{id}"))
                .field("asked", asked)
                .finish(),
            ToWaOS::Component { bytes } => f
                .debug_struct("Component")
                .field("bytes", &format_args!("{} bytes", bytes.len()))
                .finish(),
        }
    }
}

/// The digest as `Debug` writes it: its first bytes in hex, enough to tell digests apart in a
/// traced line, not the whole of it.
fn said_digest(digest: &[u8; 32]) -> String {
    let mut said = String::new();
    for byte in &digest[..4] {
        said.push_str(&format!("{byte:02x}"));
    }
    said.push('…');
    said
}

impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Input::Tap { x, y } => f.debug_struct("Tap").field("x", x).field("y", y).finish(),
            Input::Text(text) => f
                .debug_tuple("Text")
                .field(&format_args!("{} characters", text.chars().count()))
                .finish(),
            Input::Key(key) => f.debug_tuple("Key").field(key).finish(),
            Input::Scroll { x, y, dx, dy } => f
                .debug_struct("Scroll")
                .field("x", x)
                .field("y", y)
                .field("dx", dx)
                .field("dy", dy)
                .finish(),
            Input::Press { x, y, duration } => f
                .debug_struct("Press")
                .field("x", x)
                .field("y", y)
                .field("duration", duration)
                .finish(),
            Input::Drag {
                from,
                to,
                hold,
                steps,
                duration,
            } => f
                .debug_struct("Drag")
                .field("from", from)
                .field("to", to)
                .field("hold", hold)
                .field("steps", steps)
                .field("duration", duration)
                .finish(),
        }
    }
}

/// A picture is megabytes: an event that prints it says how many.
impl fmt::Debug for WindowPicture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowPicture")
            .field("window", &self.window)
            .field("title", &self.title)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("in_sight", &self.in_sight)
            .field("png_bytes", &self.png.as_ref().map(Vec::len))
            .field("asking", &self.asking)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every byte, more than once, so the length takes more than one byte of its own.
    fn bytes() -> Vec<u8> {
        (0..=u8::MAX).cycle().take(1000).collect()
    }

    /// The messages as they were encoded before their bytes were a string of bytes: each byte an
    /// element of a sequence.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum FromWaOSAsSequence {
        Started,
        Output {
            stream: OutputStream,
            bytes: Vec<u8>,
        },
    }

    #[test]
    fn output_is_encoded_as_it_was_when_its_bytes_were_a_sequence() {
        let output = FromWaOS::Output {
            stream: OutputStream::Stderr,
            bytes: bytes(),
        };
        let as_sequence = FromWaOSAsSequence::Output {
            stream: OutputStream::Stderr,
            bytes: bytes(),
        };
        let encoded = postcard::to_allocvec(&output).unwrap();
        assert_eq!(encoded, postcard::to_allocvec(&as_sequence).unwrap());
        assert_eq!(postcard::from_bytes::<FromWaOS>(&encoded).unwrap(), output);
    }

    #[test]
    fn an_opening_reads_back_as_written_and_prints_its_files() {
        let open = ToWaOS::Open {
            id: "dev.example.notes".parse().unwrap(),
            files: vec![PathBuf::from("/Users/me/notes.md")],
        };
        let encoded = postcard::to_allocvec(&open).unwrap();
        assert_eq!(postcard::from_bytes::<ToWaOS>(&encoded).unwrap(), open);
        assert_eq!(
            format!("{open:?}"),
            "Open { id: dev.example.notes, files: [\"/Users/me/notes.md\"] }"
        );
        let opened = postcard::to_allocvec(&FromWaOS::Opened).unwrap();
        assert_eq!(
            postcard::from_bytes::<FromWaOS>(&opened).unwrap(),
            FromWaOS::Opened
        );
    }

    #[test]
    fn a_run_prints_its_component_as_a_count_and_its_digest_shortened() {
        let run = ToWaOS::Run {
            id: "dev.example.triangle".parse().unwrap(),
            length: 1024,
            digest: [0xab; 32],
            variables: vec![
                ("RUST_LOG".to_owned(), "debug".to_owned()),
                ("API_TOKEN".to_owned(), "s3cret".to_owned()),
            ],
        };
        assert_eq!(
            format!("{run:?}"),
            "Run { id: dev.example.triangle, component: 1024 bytes, digest: abababab…, variables: [\"RUST_LOG\", \"API_TOKEN\"] }",
            "a variable's value may be a secret, so only its name is said"
        );
    }

    #[test]
    fn a_screenshot_and_each_input_read_back_as_written() {
        let id: ProgramId = "dev.example.notes".parse().unwrap();
        let inputs = [
            Input::Tap { x: 120.5, y: 48.0 },
            Input::Text("milk, eggs".into()),
            Input::Key("cmd+s".into()),
            Input::Scroll {
                x: 10.0,
                y: 20.0,
                dx: 0.0,
                dy: -200.0,
            },
            Input::Press {
                x: 3.0,
                y: 4.0,
                duration: Duration::from_millis(1000),
            },
            Input::Drag {
                from: (10.0, 20.0),
                to: (-5.0, 400.5),
                hold: Duration::from_millis(500),
                steps: 10,
                duration: Duration::from_millis(300),
            },
        ];
        let messages = std::iter::once(ToWaOS::Screenshot { id: id.clone() }).chain(
            inputs.into_iter().map(|input| ToWaOS::Input {
                id: id.clone(),
                window: 2,
                input,
            }),
        );
        for message in messages {
            let encoded = postcard::to_allocvec(&message).unwrap();
            assert_eq!(postcard::from_bytes::<ToWaOS>(&encoded).unwrap(), message);
        }
    }

    #[test]
    fn settings_a_set_and_what_waos_asks_read_back_as_written() {
        use crate::settings_rows::{FileDialog, Offered, Question};

        let id: ProgramId = "dev.example.camera".parse().unwrap();
        let to = [
            ToWaOS::Settings { id: id.clone() },
            ToWaOS::Set {
                id,
                row: Row::FileDialog(FileDialog::SaveFile, vec!["/tmp/out.pdf".into()]),
            },
        ];
        for message in to {
            let encoded = postcard::to_allocvec(&message).unwrap();
            assert_eq!(postcard::from_bytes::<ToWaOS>(&encoded).unwrap(), message);
        }
        let from = [
            FromWaOS::Settings(vec![SettingRow {
                key: "machine".into(),
                values: vec!["this-computer".into()],
                follows_global: false,
                offered: vec![Offered {
                    id: "this-computer".into(),
                    name: "This computer".into(),
                }],
            }]),
            FromWaOS::Asking(Asked::Question(Question::Camera)),
            FromWaOS::Unreached(Unreached::Asking(Asked::Machine)),
            FromWaOS::Unreached(Unreached::NeverRunByCargoWaos),
        ];
        for message in from {
            let encoded = postcard::to_allocvec(&message).unwrap();
            assert_eq!(postcard::from_bytes::<FromWaOS>(&encoded).unwrap(), message);
        }
    }

    #[test]
    fn a_resize_and_its_answers_read_back_as_written() {
        let resize = ToWaOS::Resize {
            id: "dev.example.notes".parse().unwrap(),
            window: 2,
            width: 390.0,
            height: 844.5,
        };
        let encoded = postcard::to_allocvec(&resize).unwrap();
        assert_eq!(postcard::from_bytes::<ToWaOS>(&encoded).unwrap(), resize);
        let from = [
            FromWaOS::Resized {
                width: 780,
                height: 1689,
                scale: 2.0,
                new_picture: true,
            },
            FromWaOS::Unreached(Unreached::InNoWindow { window: 1 }),
        ];
        for message in from {
            let encoded = postcard::to_allocvec(&message).unwrap();
            assert_eq!(postcard::from_bytes::<FromWaOS>(&encoded).unwrap(), message);
        }
    }

    #[test]
    fn typed_text_is_left_out_of_what_a_message_prints() {
        let input = ToWaOS::Input {
            id: "dev.example.notes".parse().unwrap(),
            window: 1,
            input: Input::Text("hunter2".into()),
        };
        let printed = format!("{input:?}");
        assert!(!printed.contains("hunter2"), "{printed}");
        assert!(printed.contains("7 characters"), "{printed}");
    }

    #[test]
    fn a_picture_prints_its_png_as_a_count() {
        let picture = WindowPicture {
            window: 1,
            title: "Notes".into(),
            width: 1600,
            height: 1200,
            in_sight: true,
            png: Some(vec![0; 2048]),
            asking: None,
        };
        let printed = format!("{:?}", FromWaOS::Screenshots(vec![picture]));
        assert!(printed.contains("png_bytes: Some(2048)"), "{printed}");
        assert!(!printed.contains("0, 0"), "{printed}");
    }

    #[test]
    fn an_inspection_and_its_answers_read_back_as_written() {
        let id: ProgramId = "dev.waos.notes".parse().unwrap();
        let asked = [
            Inspection::Summary { days: 7 },
            Inspection::ActiveUses,
            Inspection::Trace(TracedCapability::ALL.to_vec()),
        ];
        for asked in asked {
            let inspect = ToWaOS::Inspect {
                id: id.clone(),
                asked,
            };
            let bytes = postcard::to_stdvec(&inspect).unwrap();
            assert_eq!(postcard::from_bytes::<ToWaOS>(&bytes).unwrap(), inspect);
        }
        let inspected = FromWaOS::Inspected(vec![r#"{"verdict":"Allowed"}"#.to_owned()]);
        let bytes = postcard::to_stdvec(&inspected).unwrap();
        assert_eq!(postcard::from_bytes::<FromWaOS>(&bytes).unwrap(), inspected);
    }
}
