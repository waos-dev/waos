# fetch

A program that makes HTTP requests through WASI 0.3's HTTP client, `wasdk::http`, awaiting each, and what a reader learns WaOS's HTTP from. It asks a server on this computer for a page by two names and by address, `get allowed.localhost/hello: 200 hello`, and for a page that has moved, which WaOS tells it of rather than following, `get allowed.localhost/moved: 302 /hello`; and asks a host no one serves, `nowhere.example`, which a rule of only some domains refuses before anything is sent, `HttpRequestDenied`; sends a body as a stream, written in two parts, and reads the one echoed back as a stream, `post 127.0.0.1/echo: 200 streamed both ways`; and gives up on a request the server never answers by dropping it, `get 127.0.0.1/never: dropped`, which closes its connection, as the server then says, `get 127.0.0.1/closed: 200 closed`. It prints each request as `get host/path: status body`, the `location` a redirect names in place of its body, or the error WASI answers.

It expects its test (`crates/waos/tests/all/fetch.rs`) to serve `/hello`, `/moved`, `/echo`, `/never` and `/closed` on this computer, at the port written in `/data/port`.

- **Takes:** `wasdk`; `futures`, whose `select` races a request against the clock.
- **Provides:** a component, `target/wasm32-wasip3/debug/fetch.wasm`, from `cargo build -p fetch --target wasm32-wasip3`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`.
