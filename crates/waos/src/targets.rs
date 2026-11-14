//! The `tracing` targets the engine records a program's work under, so a subscriber picks what
//! to show: `RUST_LOG=program=info` writes what every program prints, each line naming its
//! program; `RUST_LOG=frames=trace` every step of every frame, each line naming its view;
//! `RUST_LOG=glyphs=trace` every glyph a program asks the engine to draw.

/// A program's run and what it writes to its standard streams, each event naming the program
/// in its `program` field: what it printed is the event's message, at `info` for stdout and
/// `warn` for stderr.
pub const PROGRAM: &str = "program";

/// The steps of a view's frames: asked for, handed over or held back, and what the engine adds
/// of its own.
pub const FRAMES: &str = "frames";

/// A program's glyph calls, at `trace`, one event each: the call in its `call` field (`alpha`,
/// `stroked`, `sdf`, `color` or `path`), the font's typeface and the glyph, and in `drawn`
/// whether the engine answered with a shape.
pub const GLYPHS: &str = "glyphs";
