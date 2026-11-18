//! What `cargo waos` and a running WaOS say to each other: the messages of one run, how they
//! are framed on a stream, and what carries them on this machine: a Unix socket, or a named pipe
//! on Windows.
//!
//! The connection is the run (the design's decision 29): `cargo waos run` connects, sends one
//! [`ToWaOS::Run`], the program's id with the variables that run adds to the program's
//! environment (decision 68) and its component's length and digest, then the component itself in
//! [`ToWaOS::Component`] frames until its length is reached, so a component larger
//! than one frame goes over too, and reads what comes back until the run ends: [`FromWaOS::Compiling`] while
//! WaOS compiles the component, then the program's start, its output and its end. Closing the
//! connection, by Ctrl-C or the terminal closing, stops the program; a later run of the same id replaces it. A run is the
//! program its id names (decision 11 of the permissions proposal), with that program's settings and
//! folders, installed or not. `waos open` connects the same way and sends one [`ToWaOS::Open`],
//! files to open with the program an id names, which a WaOS answers once (decision 3 of the opening
//! proposal, until programs register what opens them). `cargo waos screenshot`, `tap`, `press`,
//! `drag`, `type`, `key` and `scroll` connect the same way and send one [`ToWaOS::Screenshot`] or
//! [`ToWaOS::Input`] for the program an id names, which a WaOS answers once, and only for a program
//! a run of `cargo waos run` runs (the design's decision 60). `cargo waos settings` and `set`
//! connect the same way and send one [`ToWaOS::Settings`] or [`ToWaOS::Set`], a program's rows of
//! settings to read or one to set, which a WaOS answers once, for a program a run of `cargo waos
//! run` on this machine runs or has run, or a development program whose settings are set before
//! its first run; and while WaOS asks a program's user something, the run's connection hears
//! [`FromWaOS::Asking`]. `cargo waos inspect` connects the same way and sends
//! one [`ToWaOS::Inspect`], for a program's audit summary, what it holds or its live trace, which a
//! WaOS answers as JSON lines of `wasdk:inspect`'s values, for a program a run of `cargo waos run`
//! on this machine runs, or, for the summary, has run. There is no stop message. On this machine
//! the socket is in the user's own folder, so the system's file permissions decide who may connect
//! (decision 30), and on Windows the pipe's security descriptor gives the user alone access; the
//! network transport for phones carries the same messages.
//!
//! The invariants a later editor will break:
//! - A variant's place among its enum's, and a field's place in its record, is its encoding:
//!   postcard writes a variant by its index and a record as its fields in order, with no names.
//!   Add at the end, and raise [`PROTOCOL`] for anything else.
//! - The first frame each way says [`PROTOCOL`], the number of the build that wrote it, outside
//!   the messages' encoding, and a side reads no message until the numbers agree: `cargo waos`
//!   and the WaOS it reaches are built from the same crate, and find out when they are not.

mod component;
mod connection;
mod messages;
mod os;
#[cfg(windows)]
mod pipe;
mod settings_rows;
#[cfg(unix)]
mod socket;

pub use component::{CHUNK_BYTES, component_digest};
pub use connection::{Connection, MAX_FRAME_BYTES, PROTOCOL};
pub use messages::{
    FromWaOS, Input, Inspection, OutputStream, RunEnd, ToWaOS, TracedCapability, Unreached,
    WindowPicture,
};
#[cfg(windows)]
pub use pipe::{ClientStream, Listener, ServerStream, listener_path};
pub use settings_rows::{
    ASK, AUDIO_OUTPUT, Answer, Asked, FileDialog, GLOBAL, LINK_SCHEME, MACHINE, Offered, Question,
    Role, Row, RowError, SIMULATED_MICROPHONE, SettingRow, SimulatedSound, Switch,
};
#[cfg(unix)]
pub use socket::{ClientStream, Listener, ServerStream, listener_path};
