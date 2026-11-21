//! `wasdk:inspect`, the one schema of what an inspector reads of a program, as the engine's Rust:
//! the types wasmtime makes of the package, each serializable by the WIT's own names
//! (`waos-wit-serialize`), so the development connection sends them as JSON of the WIT values,
//! `dropped-before` and `stdout` as the WIT writes them, and `waos inspect` prints them
//! without types of its own.
//!
//! The engine's own values are made `wasdk:inspect`'s in [`values`]: a day of a program's audit
//! summary ([`day_of`]), and what it uses now and each change to it (`RunningProgram::active_uses`).
//! A program's storage ([`ProgramStorage`]) is its mounts and a folder's entries, resolved through
//! its own mount table. The package is served
//! to a program holding the inspector role in [`host`], over the programs the embedder gives
//! ([`InspectedPrograms`]).
//!
//! The invariant: nothing here holds a resource, so every record and variant serializes; the
//! package's resources, a device and a program held before its start, are the engine's own
//! values.

pub(crate) mod host;
mod programs;
mod storage;
mod values;

pub use programs::InspectedPrograms;
pub use storage::ProgramStorage;
pub use values::day_of;
pub(crate) use values::{active_use_change_of, active_use_of};

pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "wasdk:inspect/inspection",
        require_store_data_send: true,
        imports: {
            default: store | trappable,
            "wasdk:inspect/devices.[method]device.programs": async | store | trappable,
            "wasdk:inspect/devices.[method]device.icon": async | store | trappable,
            "wasdk:inspect/devices.[method]device.start-held": async | store | trappable,
            "wasdk:inspect/devices.[method]device.end": async | store | trappable,
            "wasdk:inspect/audit.days": async | store | trappable,
            "wasdk:inspect/storage.read-folder": async | store | trappable,
        },
        additional_derives: [waos_wit_serialize::WitSerialize, PartialEq],
        with: {
            "wasdk:inspect/devices.device": crate::inspection::Device,
            "wasdk:inspect/devices.held-program": crate::running::HeldProgram,
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
