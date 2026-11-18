# waft-io

A Waft app that uses Waft's own file system and HTTP client, over `wasdk-adapter-waft`: it fetches a page with `app.platform().http()` and prints its status and how many bytes came; shows a picture from its URL with `NetworkImage`, which fetches through the same client; writes, reads and lists files in `/data` with `app.platform().files()`, as a program's `std::fs` would; and, on a press, asks its user for a file and reads it, a path through `Files::read` and a handle or a stream by itself.

Its answers are printed as `what: result`, so `cargo waos run -p waft-io` shows them; the window shows the picture and the button. WaOS serves `wasi:http` and `wasi:filesystem` under the program's network and file settings.

It fetches from `httpbin.org`, a public service with a redirect and a picture at fixed URLs, rather than a server of its own: the point is the path a real app's request takes, HTTPS with the system's certificates and proxy under the program's network settings, which a plain-HTTP server on the same machine would skip. Without the network, it prints the error each request failed with.

- **Takes:** `waft` (its app, widgets, `NetworkImage` and the platform's `http()` and `files()`), `http` (the request it hands the client), `wasdk-adapter-waft` and `wasdk`.
- **Provides:** a component, `target/waos/debug/waft-io.wasm`, from `cargo waos build -p waft-io`.
- **Used by:** a person who runs it to see Waft's HTTP and files at work in WaOS; the adapter's README names it.
