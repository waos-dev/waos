//! What `cargo waos` and a running WaOS say to each other: the messages of one command, how they
//! are framed on a stream, and what carries them on this machine: a Unix socket, or a named pipe
//! on Windows.
//!
//! One connection is one command. The client speaks its [`Hello`] first and WaOS answers with its
//! own; a `cargo waos` reads WaOS's before it sends any request, so a different protocol's major or
//! a request WaOS's `serves` does not list is refused with both builds named and which one to
//! update, before anything is sent. An older WaOS also answers any request or inner variant it
//! cannot read (`Unknown`) with a [`Refusal`] naming its build rather than breaking the connection.
//! Then one [`Request`], answered in its own terms: [`ScreenshotAnswer`], [`InputAnswer`],
//! [`ResizeAnswer`], [`SettingsAnswer`], [`InspectAnswer`] or [`OpenAnswer`], or, for a run, an
//! [`Upload`] first — `Send`, and the component's parts follow; `Have`, WaOS holds it and nothing
//! is sent; or `Refused` — then [`RunEvent`]s from `Compiling` to `Ended`, with the program's
//! output among them as it comes (the design's decision 29: the connection is the run). A
//! component is megabytes, a debug build hundreds of them, so a [`Run`] carries its length and
//! SHA-256, its arguments and the run's variables, and the component follows in [`Component`]
//! frames of 8 MiB at most (`component`), which WaOS assembles and checks. A
//! request that cannot be carried out is answered with a [`Refusal`], a kind an agent acts on and
//! a message for a person.
//!
//! The messages are CBOR (`ciborium`), so each variant and field is named in its encoding: a
//! field added with `#[serde(default)]` or a variant added at any enum's end is read by a side
//! that knows it, and either side reads a variant it does not know as `Unknown`, so an addition
//! never breaks the connection. Only a real break raises [`MAJOR`].
//!
//! The invariants a later editor will break:
//! - Every enum inside a message uses `#[serde(remote = "Self", tag = "kind", content = "data")]`
//!   and `#[serde(skip)] Unknown` through `adjacent_unknown!`, and any new struct field uses
//!   `#[serde(default)]`: a plain `#[derive(Deserialize)]` or `#[serde(other)]` alone silently
//!   breaks forward compatibility on unknown variants with content.
//! - Every answer enum (and [`Upload`] and [`RunEvent`]) keeps a `Refused(Refusal)` variant of the
//!   same `{"kind": "Refused", "data": ...}` shape as [`RefusedAnswer`], so WaOS can refuse an
//!   unreadable request without naming one command's answer enum.
//! - The frame format is a four-byte little-endian count then CBOR, and the client speaks first
//!   before reading WaOS's [`Hello`], so WaOS can distinguish a legacy 4-byte 0.2 hello from a
//!   CBOR [`Hello`] on the first frame.

mod adjacent;
mod component;
mod connection;
mod hello;
mod messages;
mod os;
#[cfg(windows)]
mod pipe;
mod refusal;
mod settings_rows;
#[cfg(unix)]
mod socket;

pub use component::{CHUNK_BYTES, component_digest};
pub use connection::{Connection, Locality, MAX_FRAME_BYTES};
pub use hello::{Hello, MAJOR, Mismatch, Peer, SERVED};
pub use messages::{
    Component, Input, InputAnswer, InputDone, InspectAnswer, Inspection, OpenAnswer, OutputStream,
    RefusedAnswer, Request, ResizeAnswer, Resized, Run, RunEnd, RunEvent, ScreenshotAnswer,
    SettingsAnswer, TracedCapability, Upload, WindowId, WindowPicture,
};
#[cfg(windows)]
pub use pipe::{ClientStream, Listener, ServerStream, listener_path};
pub use refusal::{Refusal, RefusalKind};
pub use settings_rows::{
    ASK, AUDIO_OUTPUT, Answer, Asked, FileDialog, GLOBAL, LINK_SCHEME, MACHINE, Offered, Question,
    Role, Row, RowError, SIMULATED_MICROPHONE, SettingRow, SimulatedSound, Switch,
};
#[cfg(unix)]
pub use socket::{ClientStream, Listener, ServerStream, listener_path};
