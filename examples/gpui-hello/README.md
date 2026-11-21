# gpui-hello

A GPUI app as a WaOS program: text, a button that counts its clicks, a text field, and the last keys pressed, drawn by GPUI's own wgpu renderer on the program's view, in the system's interface font and Menlo, with CJK and other characters from the system's fonts as the text needs them. It is the program `sdk/wasdk-adapter-gpui`'s example completes.

- **Takes:** `wasdk-adapter-gpui`; GPUI's `gpui`, from the fork the workspace names; `unicode-segmentation`, for the text field's graphemes; `wasdk`, for the entry.
- **Provides:** a component, `target/waos/debug/gpui-hello.wasm`, from `waos build -p gpui-hello`; `waos run -p gpui-hello` runs it in WaOS.
- **Used by:** whoever starts a GPUI app on WaOS.

`src/text_field.rs` is the text field of GPUI's own `input` example, `crates/gpui/examples/input.rs` of Zed at `703b5ac3f4`, Copyright 2022 - 2025 Zed Industries, Inc., under the Apache License 2.0: its `TextInput` and `TextElement` unchanged but for the imports its entry used, with a constructor and its key bindings added at the end. So this example is under the Apache License 2.0, not the workspace's MIT; `LICENSE-APACHE` is Zed's copy of it, with Zed's notice.

GPUI is built for a component only, as the adapter is, so natively the program does nothing.
