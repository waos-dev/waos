# wasdk-wgpu

A `wgpu` backend over the engine's `wasi:webgpu`, through wgpu's own extension point for another WebGPU implementation (`custom`), so code written against `wgpu` (valo, and Waft through it) draws in a component unchanged (the design's decision 14), and draws on a view through wgpu's own surface, as a browser's canvas is drawn on, so a framework's renderer runs as it does on any system (decision 52). What wgpu has and WebGPU lacks, such as a surface on a window that is not a view, ray tracing, or shaders other than WGSL, panics with a message naming the gap; the crate's docs list all of it. Ported from the experiment.

- **Takes:** `wasi:webgpu`, as `wasdk::webgpu` generates it, and `wasdk:platform`'s views and `gpu-interop`; `wgpu`.
- **Provides:** `instance()`, a `wgpu::Instance` on wgpu's custom backend; `SharedView`, a program's view shared with the surfaces made on it, the window `Instance::create_surface` takes, its handle a `WebCanvasWindowHandle` pointing at it, whose surface hands out the view's canvas texture each frame and whose frame `Queue::present` presents on the view, with the layers the view holds, the one way the crate draws on a view; `import_texture`, a hardware buffer the program holds, one it allocated, a decoded image's or a video frame's, imported as a `wgpu::Texture` over its memory through `wasdk:platform/gpu-interop`, for what the buffer's usage names; and `convert`, the conversions between wgpu's vocabulary and WebGPU's.
- **Used by:** programs that draw with wgpu, such as `examples/triangle`; `waft-embedder-wasdk`, which draws an Waft app's views with valo over it.

```rust
let instance = wasdk_wgpu::instance();
let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await?;
let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await?;
let view = wasdk_wgpu::SharedView::new(view);
let surface = instance.create_surface(view.clone())?;
let metrics = view.metrics();
surface.configure(&device, &surface.get_default_config(&adapter, metrics.width, metrics.height).unwrap());
// Each frame:
let wgpu::CurrentSurfaceTexture::Success(frame) = surface.get_current_texture() else { return };
// ...draw into `frame.texture` and submit...
view.set_layers(&layers); // a program with composition layers
queue.present(frame);
```

`configure` sets the view's canvas to the size configured (`view.set-canvas-size`), so a frame's texture is always that size, whatever the view's own; a program configures again as its view's size changes, as it would a window's surface anywhere.

The crate compiles natively too, so `cargo clippy --workspace` reaches it, but only a component's imports mean anything.
