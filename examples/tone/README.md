# tone

A program that plays a tone while it is out of sight: what the platform keeps from freezing while it is audible. It starts a tone at 440 Hz once it runs, `playing`; plays it on while it is hidden, `hidden, playing`; stops it once it is shown again and waits for its end, `shown, ended`; is hidden again, silent, `hidden, silent`; and once shown starts the same tone again, `shown, starting again`, which the spec forbids, so it traps with the spec's name for the mistake, ``InvalidStateError - Cannot call `start` twice``.

It expects sound devices and a view whose presence changes.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip2/debug/tone.wasm`, from `cargo build -p tone --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/audio.rs`), through `crates/waos/tests/programs`.
