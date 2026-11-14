# file-dialogs

A program, written against the WIT itself, that picks in the platform's file dialogs, in order: a file to read, which it reads and tries to write to; a file to change, asked writable, which it writes; where to save, which it writes, then saves again safely, writing in its temporary folder and renaming onto the file picked; and a folder, whose entries it counts. It says where its temporary folder is first, and prints each step as `what: result`. What a reader learns the file dialogs interface from.

- **Takes:** `wasdk`, the generated bindings.
- **Provides:** a component, `target/wasm32-wasip2/debug/file-dialogs.wasm`, from `cargo build -p file-dialogs --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/file_dialogs.rs`), through `crates/waos/tests/programs`; whoever runs it in WaOS.
