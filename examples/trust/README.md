# trust

A program that asks the system whether it trusts a server's certificate chain, `wasdk::trust`, and what a reader learns `trust` from. It checks the chain its test gave it, a self-signed one, which no system trusts, `verify the chain: untrusted`; then a chain with no certificate, `verify no chain: failed`; and requests a page over HTTPS with `wasdk::http`, WASI 0.3's client, from a server presenting that chain, which WaOS refuses with the same check, `get over https: ErrorCode::TlsCertificateError`.

It expects its test (`crates/waos/tests/all/trust.rs`) to write the chain's one certificate, for `trusted.localhost`, in `/data/chain.der`, and to serve it over TLS on this computer, at the port written in `/data/port`.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip3/debug/trust.wasm`, from `cargo build -p trust --target wasm32-wasip3`; a package, packaged as Trust with its icon, `icon.png`, `target/waos/dev.waos.examples.trust-<version>.wapk`, from `waos package -p trust`, for checking WaOS on a phone.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`; whoever checks the certificate check in WaOS on another system.
