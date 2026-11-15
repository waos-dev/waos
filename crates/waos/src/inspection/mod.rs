//! `wasdk:inspect`, the one schema of what an inspector reads of a program, as the engine's Rust:
//! the types wasmtime makes of the package, each serializable, so the development connection sends
//! them as JSON of the WIT values and `cargo waos inspect` prints them without types of its own.
//!
//! The invariant: nothing here holds a resource, so every record and variant serializes; the one
//! resource of the package, a device, is the engine's own value.

pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "wasdk:inspect/inspection",
        require_store_data_send: true,
        imports: {
            default: async | store | trappable,
        },
        additional_derives: [serde::Serialize, serde::Deserialize, PartialEq],
        with: {
            "wasdk:inspect/devices.device": crate::inspection::Device,
        },
    });
}

/// `devices.device`: a device whose WaOS can be inspected.
pub struct Device;
