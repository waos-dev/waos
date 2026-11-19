# camera

An Waft app that shows a camera's preview: the first camera the system lists, opened at once, which asks the user the first time, then started in its largest format up to 1920 by 1080, with its preview on a composition layer the system draws it on (Waft's `CameraSession` and `CameraPreview`) and its photos at the largest size that format takes them at. It prints the cameras the system lists, as `cameras: FaceTime HD Camera (front)`, and again each time one is plugged in or out; the camera's formats, as `format 1920x1080 at 1-30 fps, photos 1920x1080`, the one it runs in, as `runs in 1920x1080 at 1-30 fps, photos 1920x1080`, which is another program's where that program runs the camera already, and its state now and at each change, as `state: running`, `state: interrupted (another app has the camera)`, `state: interrupted` where the system gives no reason, as macOS does not, or `state: ended (the camera is no longer attached)`. A button under the preview takes a photo, encoded by the system, and prints its type and size, as `photo: image/jpeg, 352841 bytes`. Where the system has no cameras, or the user does not allow them, the window says so. Asking to close its window quits it. What the lead looks at to see a program use a camera through WaOS.

To run it, start the WaOS built from this workspace, then send the program to it:

```sh
cargo waft run -p waos-app      # builds WaOS.app with the cameras, and opens it
cargo waos run -p camera         # builds the program, and runs it in that WaOS
```

WaOS.app must be the bundle `cargo waft` builds: its `Info.plist` says why WaOS uses the camera (`NSCameraUsageDescription`, from `apps/waos/Cargo.toml`), without which macOS ends WaOS as the camera opens. `cargo waos run` starts the installed WaOS.app only when no WaOS answers, and a WaOS built before the cameras cannot run the program, which imports `cameras`. The first run asks twice: WaOS asks whether the program may use the camera, in a sheet over its window, and, the first time WaOS itself opens a camera, macOS asks whether WaOS may. While the camera runs, WaOS's library names the program among those using the camera.

- **Takes:** `wasdk-adapter-waft`; Waft's `waft-widgets` (the app, its window, `CameraSession`, `CameraPreview`, `available_cameras`, `available_cameras_changed` and `open_camera`), `waft-cupertino` (the page and the button), `waft-embedder` (a camera, its formats, its outputs and its state) and `waft-foundation`, from crates.io; `wasdk`, for the entry.
- **Provides:** a component, `target/wasm32-wasip3/debug/camera.wasm`, from `cargo build -p camera --target wasm32-wasip3`.
- **Used by:** whoever runs it in WaOS.
