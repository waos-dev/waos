# workers

A program, written against the WIT itself, that runs parts of its work on workers, other instances of its own code that share nothing with it, in order: it says how many threads the machine runs at once; starts a worker at an entry it does not have, and is told it is unknown; sends three words to a worker that reverses each, and reads the three back in order; counts the primes below 200,000 on a worker while its main instance goes on waking every 10 ms, and says how often it woke before the answer came; starts a worker that traps, and says how it ended; starts a worker that passes a word through two workers of its own; asks a worker what it has of the user's side, and is told none of it, each part as a platform without it answers; drops a counting worker as soon as it counts; and last starts a counting worker and, once it is asked to exit, ends with the worker still running. It prints each step as `what: result`, and a counting worker prints `worker count <n>` as it counts. What a reader learns the workers interface from.

- **Takes:** `wasdk`, the generated bindings and `#[wasdk::worker]`.
- **Provides:** a component, `target/wasm32-wasip2/debug/workers.wasm`, from `cargo build -p workers --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/workers.rs`), through `crates/waos/tests/programs`; whoever runs it in WaOS.
