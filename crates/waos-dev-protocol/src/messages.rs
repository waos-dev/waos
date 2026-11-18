//! What `cargo waos` asks a WaOS, and what a WaOS answers, for each of its commands: one
//! connection is one request, and each request has its own answers rather than one enum every
//! command must sort through.
//!
//! The invariant: CBOR names each variant and each field, so a field or a variant added leaves the
//! others as they were, and a side reads what it does not know as `Unknown` where an answer's
//! enum says so, never breaking the connection over an addition. A program's output is bytes, not
//! lines, so a terminal shows what the program wrote as it wrote it, a prompt without its newline
//! among it; bytes travel as CBOR's own byte string (`serde_bytes`), in one copy. A picture's PNG
//! is bytes too. Text typed into a program is left out of what a message's `Debug` writes, since
//! messages are traced and it may be a password, and so are payload bytes.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use waos_package::ProgramId;

use crate::refusal::Refusal;
use crate::settings_rows::{Asked, Row, SettingRow};

/// One of a run's windows, named as it opens: a number the run never gives twice, so a window
/// keeps its name however others close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for WindowId {
    type Err = String;

    fn from_str(text: &str) -> Result<WindowId, String> {
        text.parse::<u64>().map(WindowId).map_err(|_| {
            format!("{text:?} is not a window id: they are numbers, as a screenshot lists them")
        })
    }
}

/// What `cargo waos` asks a WaOS: one request a connection, each answered in its own terms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// Run a component as the program it names, with the program's settings and folders,
    /// installed or not: see [`Run`].
    Run(Run),
    /// The last picture of each window of the program `id` names, or of the one `window` names
    /// alone, while a run of `cargo waos run` runs it.
    Screenshot {
        id: ProgramId,
        window: Option<WindowId>,
    },
    /// `input` to the program `id` names, as its user would give it, in its window `window`, while
    /// a run of `cargo waos run` runs it.
    Input {
        id: ProgramId,
        window: WindowId,
        input: Input,
    },
    /// Sizes the window `window` of the program `id` names, so the view in it is `width` by
    /// `height` logical points, its top-left where it is: as the program's own `set-frame` sizes
    /// it, while a run of `cargo waos run` runs it.
    Resize {
        id: ProgramId,
        window: WindowId,
        width: f64,
        height: f64,
    },
    /// The rows of the settings of the program `id` names, while a run of `cargo waos run` on this
    /// machine runs it or once one has.
    Settings { id: ProgramId },
    /// Sets `row` among the settings of the program `id` names, as its user would on its settings
    /// page, for the same programs.
    Set { id: ProgramId, row: Row },
    /// What `asked` asks of the program `id` names: its audit summary, what it uses now or its
    /// live trace, as JSON of `wasdk:inspect`'s values.
    Inspect { id: ProgramId, asked: Inspection },
    /// Opens these files, absolute paths on this machine, with the program `id` names, as its
    /// user would open them with it: the program running under that id, or the installed one,
    /// started. This machine's alone.
    Open { id: ProgramId, files: Vec<PathBuf> },
}

impl Request {
    /// Its name in WaOS's hello: what `serves` lists.
    pub fn name(&self) -> &'static str {
        match self {
            Request::Run(_) => "run",
            Request::Screenshot { .. } => "screenshot",
            Request::Input { .. } => "input",
            Request::Resize { .. } => "resize",
            Request::Settings { .. } => "settings",
            Request::Set { .. } => "set",
            Request::Inspect { .. } => "inspect",
            Request::Open { .. } => "open",
        }
    }
}

/// A run: the program, the variables this run alone adds to its environment (the design's decision
/// 68), and the component's length and SHA-256 (`component_digest`), which the parts that follow
/// carry. The program running under the id is stopped first. Nothing of the client's own
/// environment is sent, and WaOS's own variables win where both name one.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: ProgramId,
    /// The variables the program reads besides the engine's own, each its name and value.
    pub variables: Vec<(String, String)>,
    /// The component's bytes, which the parts that follow carry together.
    pub length: u64,
    /// The component's SHA-256, checked once its bytes are all there.
    pub digest: [u8; 32],
}

/// A component is megabytes and a variable's value may be a secret: an event that prints the run
/// says how many bytes, the digest shortened, and each variable's name, never its value.
impl fmt::Debug for Run {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self
            .variables
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        f.debug_struct("Run")
            .field("id", &format_args!("{}", self.id))
            .field("component", &format_args!("{} bytes", self.length))
            .field("digest", &format_args!("{}", said_digest(&self.digest)))
            .field("variables", &format_args!("{names:?}"))
            .finish()
    }
}

/// Part of a run's component, which its `Run` announced: at most [`CHUNK_BYTES`](crate::CHUNK_BYTES)
/// of it, the parts in order until the announced length is reached.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}

/// A component is megabytes: an event that prints one says how many.
impl fmt::Debug for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Component")
            .field("bytes", &format_args!("{} bytes", self.bytes.len()))
            .finish()
    }
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

/// What a screenshot answers: the pictures, in the order the windows opened; or why none was
/// taken. An answer this build does not know is `Unknown`, skipped, never a broken connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", content = "data")]
pub enum ScreenshotAnswer {
    Pictures(Vec<WindowPicture>),
    Refused(Refusal),
    #[serde(other)]
    Unknown,
}

/// What an input answers: `InputDone` once the window shows its next picture, or why it reached
/// none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", content = "data")]
pub enum InputAnswer {
    Done(InputDone),
    Refused(Refusal),
    #[serde(other)]
    Unknown,
}

/// What the program heard, and what its window did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDone {
    /// Whether the window was in sight when the input was given, a run of `cargo waos run`
    /// counting its windows in sight while it is attached.
    pub in_sight: bool,
    /// Whether the window then showed a picture the program drew after it, within the time WaOS
    /// waits for one: none comes for an input that changed nothing, nor while the window is out of
    /// sight.
    pub new_picture: bool,
}

/// What a resize answers: the size the view reported, or why it reached no window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", content = "data")]
pub enum ResizeAnswer {
    Done(Resized),
    Refused(Refusal),
    #[serde(other)]
    Unknown,
}

/// The window's view is `width` by `height` physical pixels at `scale` physical pixels a logical
/// point, as it reported itself after the resize: the size asked for, or the one the window system
/// clamped it to, or the size it had where the window system left it as it was. `new_picture` says
/// whether the window then showed a picture the program drew at that size, as `InputDone`'s does.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resized {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub new_picture: bool,
}

/// What reading or setting a program's settings answers: the rows after the request, as its
/// settings page shows them, or why not.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", content = "data")]
pub enum SettingsAnswer {
    Rows(Vec<SettingRow>),
    Refused(Refusal),
    #[serde(other)]
    Unknown,
}

/// What an inspection answers: its lines as they come, or why not.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", content = "data")]
pub enum InspectAnswer {
    Inspected(Vec<String>),
    Refused(Refusal),
    #[serde(other)]
    Unknown,
}

/// What an opening answers: the files were handed to the program, or why not.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", content = "data")]
pub enum OpenAnswer {
    Opened,
    Refused(Refusal),
    #[serde(other)]
    Unknown,
}

/// What a run's connection hears, from `Compiling` to `Ended`; an event this build does not know
/// is `Unknown`, skipped, never a broken connection.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum RunEvent {
    /// WaOS compiles the component of the run, which it does before it starts it: `Started` or
    /// `Refused` follows once it has, minutes later for a large debug build.
    Compiling,
    Started,
    /// Bytes the program wrote, as it wrote them: a terminal shows them as they come.
    Output {
        stream: OutputStream,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    /// WaOS asks the run's program's user something now, and the program waits for the answer: a
    /// row on Ask, which `cargo waos set` answers ahead next time.
    Asking(Asked),
    /// The component could not be loaded or run, or the run could not start, and why.
    Refused(Refusal),
    Ended(RunEnd),
    #[serde(other)]
    Unknown,
}

/// A program's output is bytes: an event that prints it says how many.
impl fmt::Debug for RunEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunEvent::Compiling => f.write_str("Compiling"),
            RunEvent::Started => f.write_str("Started"),
            RunEvent::Output { stream, bytes } => f
                .debug_struct("Output")
                .field("stream", stream)
                .field("bytes", &format_args!("{} bytes", bytes.len()))
                .finish(),
            RunEvent::Asking(asked) => f.debug_tuple("Asking").field(asked).finish(),
            RunEvent::Refused(refusal) => f.debug_tuple("Refused").field(refusal).finish(),
            RunEvent::Ended(end) => f.debug_tuple("Ended").field(end).finish(),
            RunEvent::Unknown => f.write_str("Unknown"),
        }
    }
}

/// One of a program's windows, as its user sees it: what `cargo waos screenshot` writes.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPicture {
    /// The window's id, as the run gave it when the window opened: what an input's `window` names.
    pub window: WindowId,
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

/// Typed text may be a password: an event that prints an input says how many characters.
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

#[cfg(test)]
mod tests {
    use crate::refusal::RefusalKind;

    use super::*;

    fn cbor<T: Serialize + serde::de::DeserializeOwned + PartialEq + fmt::Debug>(value: T) {
        let mut bytes = Vec::new();
        ciborium::into_writer(&value, &mut bytes).unwrap();
        let back: T = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn each_requests_answer_reads_back_as_written() {
        let id: ProgramId = "dev.example.notes".parse().unwrap();
        let requests = [
            Request::Run(Run {
                id: id.clone(),
                variables: vec![("RUST_LOG".to_owned(), "debug".to_owned())],
                length: 1024,
                digest: [0xab; 32],
            }),
            Request::Screenshot {
                id: id.clone(),
                window: Some(WindowId(2)),
            },
            Request::Input {
                id: id.clone(),
                window: WindowId(1),
                input: Input::Text("héllo".into()),
            },
            Request::Resize {
                id: id.clone(),
                window: WindowId(1),
                width: 390.0,
                height: 844.5,
            },
            Request::Settings { id: id.clone() },
            Request::Set {
                id: id.clone(),
                row: Row::FileDialog(
                    crate::settings_rows::FileDialog::SaveFile,
                    vec!["/tmp/out.pdf".into()],
                ),
            },
            Request::Inspect {
                id: id.clone(),
                asked: Inspection::Trace(TracedCapability::ALL.to_vec()),
            },
            Request::Open {
                id: id.clone(),
                files: vec!["/Users/me/notes.md".into()],
            },
        ];
        for request in requests {
            cbor(request);
        }
    }

    #[test]
    fn each_answer_reads_back_as_written() {
        cbor(ScreenshotAnswer::Pictures(vec![WindowPicture {
            window: WindowId(1),
            title: "Notes".into(),
            width: 1600,
            height: 1200,
            in_sight: true,
            png: Some(vec![0; 2048]),
            asking: None,
        }]));
        cbor(InputAnswer::Done(InputDone {
            in_sight: true,
            new_picture: false,
        }));
        cbor(ResizeAnswer::Done(Resized {
            width: 780,
            height: 1689,
            scale: 2.0,
            new_picture: true,
        }));
        cbor(SettingsAnswer::Rows(vec![SettingRow {
            key: "machine".into(),
            values: vec!["this-computer".into()],
            follows_global: false,
            offered: vec![crate::settings_rows::Offered {
                id: "this-computer".into(),
                name: "This computer".into(),
            }],
        }]));
        cbor(InspectAnswer::Inspected(vec![
            r#"{"verdict":"Allowed"}"#.into(),
        ]));
        cbor(OpenAnswer::Opened);
        cbor(RunEvent::Output {
            stream: OutputStream::Stderr,
            bytes: (0..=255).cycle().take(1000).collect(),
        });
        cbor(RunEvent::Ended(RunEnd::Failed("trap".into())));
        cbor(RunEvent::Asking(Asked::Question(
            crate::settings_rows::Question::Camera,
        )));
    }

    #[test]
    fn a_kind_or_an_answer_this_build_does_not_know_is_unknown() {
        // A newer WaOS's refusals and answers: CBOR maps for adjacent tags this build has not.
        let newer_refusal = vec![
            (0xa2, "kind"), // placeholder to keep rustfmt from collapsing
        ];
        let _ = newer_refusal;
        let kind: RefusalKind = ciborium::from_reader(
            [
                0xa1, 0x64, b'k', b'i', b'n', b'd', 0x65, b'N', b'e', b'w', b'e', b'r',
            ]
            .as_slice(),
        )
        .unwrap();
        assert_eq!(kind, RefusalKind::Unknown);
        let answer: InputAnswer = ciborium::from_reader(
            [
                0xa1, 0x66, b'a', b'n', b's', b'w', b'e', b'r', 0x65, b'N', b'e', b'w', b'e', b'r',
            ]
            .as_slice(),
        )
        .unwrap();
        assert_eq!(answer, InputAnswer::Unknown);
    }

    #[test]
    fn a_run_prints_its_component_as_a_count_and_its_digest_shortened() {
        let run = Run {
            id: "dev.example.triangle".parse().unwrap(),
            variables: vec![
                ("RUST_LOG".to_owned(), "debug".to_owned()),
                ("API_TOKEN".to_owned(), "s3cret".to_owned()),
            ],
            length: 1024,
            digest: [0xab; 32],
        };
        assert_eq!(
            format!("{run:?}"),
            "Run { id: dev.example.triangle, component: 1024 bytes, digest: abababab…, variables: [\"RUST_LOG\", \"API_TOKEN\"] }",
            "a variable's value may be a secret, so only its name is said"
        );
    }

    #[test]
    fn typed_text_and_payload_bytes_are_left_out_of_what_a_message_prints() {
        let request = Request::Input {
            id: "dev.example.notes".parse().unwrap(),
            window: WindowId(1),
            input: Input::Text("hunter2".into()),
        };
        let printed = format!("{request:?}");
        assert!(!printed.contains("hunter2"), "{printed}");
        assert!(printed.contains("7 characters"), "{printed}");

        let event = RunEvent::Output {
            stream: OutputStream::Stdout,
            bytes: b"a secret".to_vec(),
        };
        let printed = format!("{event:?}");
        assert!(!printed.contains("secret"), "{printed}");
        assert!(printed.contains("8 bytes"), "{printed}");

        let picture = WindowPicture {
            window: WindowId(1),
            title: "Notes".into(),
            width: 1600,
            height: 1200,
            in_sight: true,
            png: Some(vec![0; 2048]),
            asking: None,
        };
        let printed = format!("{picture:?}");
        assert!(printed.contains("png_bytes: Some(2048)"), "{printed}");
        assert!(!printed.contains("0, 0"), "{printed}");
    }
}
