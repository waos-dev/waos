//! What `cargo waos` and a running WaOS say to each other: the messages of one command, how they
//! are framed on a stream, and what carries them on this machine: a Unix socket, or a named pipe
//! on Windows.
//!
//! One connection is one command. Each side says its [`Hello`] first, and a `cargo waos` reads
//! WaOS's before it sends, so a different protocol's major or a request WaOS does not serve is
//! refused with both builds named and which one to update, before anything is sent. Then one
//! [`Request`], answered in its own terms: [`ScreenshotAnswer`], [`InputAnswer`],
//! [`ResizeAnswer`], [`SettingsAnswer`], [`InspectAnswer`] or [`OpenAnswer`], or, for a run,
//! [`RunEvent`]s from `Compiling` to `Ended`, with the program's output among them as it comes
//! (the design's decision 29: the connection is the run). A component is megabytes, a debug build
//! hundreds of them, so a [`Run`] carries its length and SHA-256 and the component follows in
//! [`Component`] frames of 8 MiB at most (`component`), which WaOS assembles and checks. A
//! request that cannot be carried out is answered with a [`Refusal`], a kind an agent acts on and
//! a message for a person.
//!
//! The messages are CBOR (`ciborium`), so each variant and field is named in its encoding: a
//! field added with `#[serde(default)]` or a variant added at an answer's end is read by a side
//! that knows it, and an answer enum reads one it does not as `Unknown`, so an addition never
//! breaks the other half. Only a real break raises [`MAJOR`].
//!
//! The invariants a later editor will break:
//! - A field added is `#[serde(default)]`, and an answer enum's `Unknown` catches a variant a
//!   newer WaOS sends; a request is gated by the hello's `serves`, so an older WaOS never meets
//!   one it does not know.
//! - The frames' shape: a four-byte little-endian count, then the message as CBOR. The first
//!   frame each way is the hello, before any request.

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
pub use hello::{Hello, MAJOR, Mismatch, SERVED};
pub use messages::{
    Component, Input, InputAnswer, InputDone, InspectAnswer, Inspection, OpenAnswer, OutputStream,
    Request, ResizeAnswer, Resized, Run, RunEnd, RunEvent, ScreenshotAnswer, SettingsAnswer,
    TracedCapability, WindowId, WindowPicture,
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
