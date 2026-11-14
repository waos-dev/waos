# network

A program that reaches the network through `std::net`, WASI 0.2's sockets, and through `wasdk::sockets`, WASI 0.3's, which it awaits, and what a reader learns WaOS's network from. It looks up a name its test's domain rule allows and one it does not, `lookup refused.example: PermissionDenied`, and `app.localhost`, which is this computer on every system, under the local-network switch whatever the rule; connects to this computer by name and by address, which the local-network switch allows and the network switch does not; sends a datagram to an address of the internet, refused under a rule of only some domains as a bare address is; does the same lookups, `await lookup refused.example: ErrorCode::AccessDenied`, and a connection to this computer with an echo through WASI 0.3's sockets, `await echo 127.0.0.1: ping`, which answer as `std::net`'s do; and holds a connection to this computer while its user turns the switch off, then finds a new connection refused and the one it holds still echoing. It prints each result as `what: result`, an error by its kind; a lookup the rule lets through prints `not refused`, since no one serves the names under `example`.

It expects its test (`crates/waos/tests/all/network.rs`) to serve an echo on this computer, at the port written in `/data/port`.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip2/debug/network.wasm`, from `cargo build -p network --target wasm32-wasip2`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`.
