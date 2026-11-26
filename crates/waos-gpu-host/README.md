# waos-gpu-host

The host the engine serves `wasdk:gpu` with: wgpu's API over wgpu-core, each call of the interface one call of wgpu-core's, as wgpu's own wgpu-core backend makes it.

- **Takes**: wgpu-core and wgpu-types, re-exported (`reexports`) so the engine works at the same version; wasmtime, whose bindings of `wit/deps/wasdk-gpu-1.0.0` (a copy of the repository's, made by `cargo xtask sync-wit`) it implements.
- **Provides**: `add_to_linker` and `GpuCtx`, what a store serves the package from; the embedder's say in a program's GPU (`AdapterChooser`, `DeviceOpener`, `SubmissionGate`, `DeviceListener`) and where a view's frames come from (`SurfaceTarget`); the objects the embedder hands a program or keeps of one (`Device`, `WeakDevice`, `Texture`, `Surface`, `GpuObject`).
- **Used by**: the engine (`crates/waos`), which links it into every program's store, implements the embedder's traits, and makes a view's surface over its canvas.
