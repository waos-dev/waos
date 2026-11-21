<p align="center"><img src="apps/waos/assets/icon.png" width="96" alt="WaOS"></p>

# WaOS

Apps on demand. Secure. Fast. Efficient.

WaOS runs apps that draw with WebGPU and need nothing of a browser. An app is a WebAssembly component: it runs in its own sandbox with only what its user allows (files, network, camera, microphone), and gets the system's own windows, menus, file dialogs, media, fonts and notifications. One build runs on every platform WaOS runs on.

Apps are written in Rust, usually with [Waft](https://github.com/waos-dev/waft), a Rust port of Flutter with Material, Cupertino and WinUI widgets; anything that draws with wgpu works too.

## Use WaOS

WaOS is at [waos.app](https://waos.app).

## Write an app

```bash
curl -fsSL https://waos.app/install.sh | sh   # WaOS and its command, waos; Rust from rustup.rs
waos new hello          # a Waft app; --template cupertino for one meant for phones
cd hello && waos run    # build it and run it in WaOS
waos package            # a signed .wapk to share
```

Building with an agent? Point it at [waos.app/agents.md](https://waos.app/agents.md).

The docs are at [waos.app/docs](https://waos.app/docs): concepts, a guide per feature, and the full interface.

## Contribute

The toolchain is pinned in `rust-toolchain.toml`, and [Waft](https://github.com/waos-dev/waft), the framework the app is built on, comes from crates.io.

```bash
cargo test --workspace
cargo install waft-cli          # cargo waft, which runs Waft apps
cargo waft run -p waos-app      # the WaOS app
```

### Working on Waft and WaOS together

WaOS builds on released Waft: a Waft change WaOS needs is released first. To try one before it is, point the build at your Waft checkout with Cargo's [`[patch.crates-io]`](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html), outside this repository.

What each folder is:

| Folder | What it is |
|---|---|
| `wit/` | The interface apps see, in WIT |
| `sdk/` | `wasdk`, what apps are written with, and its adapters for Waft and GPUI |
| `crates/waos` | The engine any app can embed to run WaOS apps |
| `crates/waos-cli` | `waos` |
| `apps/waos` | The WaOS app |
| `builtin-apps/` | Apps that come with WaOS: Notes, Reader, Inspector, a video player and more |
| `examples/` | Small apps, one per feature |
| `packages/waos-site` | waos.app |
| `docs/proposals/` | The design and its decisions |

Contributors and agents: read `AGENTS.md` first.

## License

MIT
