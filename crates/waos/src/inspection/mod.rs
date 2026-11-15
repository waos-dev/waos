//! `wasdk:inspect`, the one schema of what an inspector reads of a program, as the engine's Rust:
//! the types wasmtime makes of the package, each serializable by the WIT's own names
//! (`waos-wit-serialize`), so the development connection sends them as JSON of the WIT values,
//! `dropped-before` and `stdout` as the WIT writes them, and `cargo waos inspect` prints them
//! without types of its own.
//!
//! The engine's own values are made `wasdk:inspect`'s in [`values`]: a day of a program's audit
//! summary ([`day_of`]), and what it holds now (`RunningProgram::holdings`).
//!
//! The invariant: nothing here holds a resource, so every record and variant serializes; the one
//! resource of the package, a device, is the engine's own value.

mod values;

pub use values::day_of;
pub(crate) use values::now_of;

pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "wasdk:inspect/inspection",
        require_store_data_send: true,
        imports: {
            default: async | store | trappable,
        },
        additional_derives: [waos_wit_serialize::WitSerialize, PartialEq],
        with: {
            "wasdk:inspect/devices.device": crate::inspection::Device,
        },
    });
}

/// `devices.device`: a device whose WaOS can be inspected.
pub struct Device;

#[cfg(test)]
mod tests {
    use super::bindings::wasdk::inspect::trace::{
        ClipboardEvent, ClipboardRead, ClipboardUse, OutputChunk, OutputStream, Verdict,
    };
    use super::bindings::wasi::clocks::system_clock::Instant;
    use crate::TraceEvent;

    const AT: Instant = Instant {
        seconds: 1_763_164_860,
        nanoseconds: 5,
    };

    #[test]
    fn an_event_reads_as_json_by_the_wit_names() {
        let read = TraceEvent::Clipboard(ClipboardEvent {
            at: AT,
            instance: 1,
            dropped_before: 2,
            kind: ClipboardUse::Read(ClipboardRead {
                form: "text/plain".to_owned(),
                pasted: false,
            }),
            verdict: Verdict::Refused("clipboard-read".to_owned()),
        });
        assert_eq!(
            serde_json::to_string(&read).unwrap(),
            r#"{"at":{"seconds":1763164860,"nanoseconds":5},"instance":1,"dropped-before":2,"kind":{"read":{"form":"text/plain","pasted":false}},"verdict":{"refused":"clipboard-read"}}"#
        );
        let written = TraceEvent::Output(OutputChunk {
            at: AT,
            instance: 0,
            dropped_before: 0,
            channel: OutputStream::Stdout,
            bytes: b"hi".to_vec(),
        });
        assert_eq!(
            serde_json::to_string(&written).unwrap(),
            r#"{"at":{"seconds":1763164860,"nanoseconds":5},"instance":0,"dropped-before":0,"channel":"stdout","bytes":[104,105]}"#
        );
    }
}
