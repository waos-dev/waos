# notifications

A program, written against the WIT itself, that shows notifications, in order: it says where its permission stands, posts a greeting, which is denied without asking while its user has not allowed it, asks its user, and posts the greeting again, with a button; where the system has replies, it posts the greeting a third time, replacing it, with an action answered by typing; where the system has scheduling, it schedules a silent reminder for the first moment of 2100; and it removes a notification of an id it no longer needs. Then it listens for what its user does to its notifications, on two streams, until it is stopped: the first says each activation, its id, the action chosen and what the user typed, and the second each id and action again. It prints each step as `what: result`. What a reader learns the notifications interface from.

- **Takes:** `wasdk`, the generated bindings.
- **Provides:** a component, `target/wasm32-wasip2/debug/notifications.wasm`, from `cargo build -p notifications --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/notifications.rs`), through `crates/waos/tests/programs`; whoever runs it in WaOS.
