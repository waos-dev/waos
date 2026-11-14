# Camera

A camera: the camera's picture live in the window, a look chosen for it, a photo taken, and the photos taken kept, viewed, exported and deleted. A Waft app in Fluent's dark look, Waft's WinUI controls, over `waft-embedder-wasdk`. Its package is `camera-app`, since `examples/camera` is the workspace's `camera`.

```sh
cargo waos run -p camera-app --release     # builds the program, and runs it in the WaOS on this machine
cargo test -p camera-app                   # the looks, the library, the format choice, on this computer
```

`--release` because WaOS loads a release component in seconds and a debug one, some 90 MB, in minutes.

## What a person does with it

- **Sees the camera.** The window opens on the camera's live picture, upright however the camera is mounted and the phone held, kept in its own shape on black. A camera that looks at its user shows as a mirror does, as each frame says.
- **Picks a look.** Under the picture, a selector bar: Original, Mono, Warm, Cool, Vivid, Soft; or the left and right arrows. The picture changes at once, and the next photo is taken in the look shown.
- **Takes a photo.** The round shutter, or Space, whatever was clicked last: nothing on the camera's page takes the keyboard from it. The picture blinks, and the photo appears as a small tile at the bottom left.
- **Switches camera,** with the button at the bottom right, where the system lists more than one.
- **Looks at the photos.** A tap on the tile opens them: one large, over a strip of them all, the newest first. Each is named, with when it was taken beneath, on the user's clock (`system`'s `utc-offset`, UTC where the system names no time zone). A tap in the strip or the left and right arrows show another. **Export…** asks where in the system's save dialog and puts a copy of the file there; the bin asks, then deletes the photo for good. Back, or Esc, returns to the camera.

Where there is no picture, the window says why and what to do: WaOS is asking whether the app may use the camera (the first time); the user refused, and allows it in WaOS's settings for the app; there is no camera; or the camera did not start, with the system's reason. It starts by itself once it can: when the user allows the camera, and when a camera is plugged in. While the app is hidden the camera is stopped, and starts again when the app shows.

## How it is made

- **The live picture is drawn by the app, on a WebGPU device of its own.** The camera starts with its frames in a frame reader (`video-frames`), not with a system preview. At each frame the window draws, the app asks the reader for the camera's newest frame, imports the frame's hardware buffer as a texture on its device (`gpu-interop`, no copy), and runs the look's shader (`src/look.wgsl`) over it onto a hardware buffer of its own (`hardware-buffers`) at the layer's size in the screen's pixels, as the system shows a layer's buffer at its own pixels on every system, turned and mirrored as the frame's `rotation` and `flip` say, its sides swapped for a quarter turn, which a composition layer then shows beneath the window's drawing (`views`). So a frame's pixels are copied once, by the look. A buffer is drawn into again only once the compositor is done with it, and a frame never waits for that: the system learns that the compositor let go of a buffer only at the layer's next present, so a frame that finds none free draws into a new one, and the buffers settle at a few (`src/live_picture.rs`).
- **A look is five numbers** (`src/looks.rs`): a white balance, how much colour is kept, a contrast curve, how much of the picture is its own blur, and how dark the corners go. One shader takes them, so the live picture and a photo are drawn the same way.
- **A photo is the system's.** The camera starts with photos at the largest size its format takes them at, and the shutter asks the system for one, captured and encoded by it. In the Original look that file is kept as it is, with what the camera wrote in it. In any other look it is developed (`src/developing.rs`): the system's hardware image codecs decode it upright into a hardware buffer, the same shader draws it onto another of its size, and the hardware codecs encode that as JPEG (PNG where the system encodes no JPEG), the pixels never crossing the program's memory. WaOS has image codecs, so the app carries no encoder of its own.
- **The photos are files in the app's own `/data/photos`**, named as a camera names them, `IMG_0001.jpg`, each one past the highest there, and dated by when its file was written (`src/library.rs`, `src/dates.rs`); a photo is written under a draft's name and renamed, so a listing never shows half of one. Thumbnails and the large picture are decoded from the files by the system's codecs at the size shown.
- **Export** copies the file into the program's temporary folder and renames it onto the place picked, since WaOS mounts that place and nothing else of its folder.

The modules, by layer: the rules as plain Rust (`looks`, `library`, `dates`); what the app holds while it runs (`viewfinder`, the camera and its status; `darkroom`, the app's device and painter; `roll`, the photos with their thumbnails; `camera_app`, all of them with what the window says); what is done with the platform (`look_painter` and `sheet`, the drawing; `live_picture`, the buffers shown in turn; `developing`, `shooting`, `exporting`, `pictures`, `permission`); and the widgets (`camera_page` with `live_view`, `look_bar`, `controls`, `shutter_button` and `status_message`; `photos_page` with `photo_tile`; `notice_bar`).

## What it needs of the system, and says where it is missing

Cameras, frame readers, a compositor, a hardware buffer allocator and a WebGPU device for the live picture: without one of them the window says the camera's picture cannot be drawn, and why. Image codecs with hardware codecs for a photo in a look other than Original, and for showing the photos kept. A save dialog for Export.

## What it leaves out

- **Video.** WaOS offers programs no recording yet.
- **What the camera wrote in a developed photo.** The system's codecs write no metadata, so a photo taken in a look carries no date, camera or orientation of its own (it is stored upright).
- **A fallback through the program's memory** for a system with image codecs but no hardware codecs: nothing here could run it.

- **Takes:** `waft`, `waft-winui`, `waft-services` (the keys) and `waft-embedder-wasdk`; `wasdk-wgpu` and `wgpu`, the app's own device; `wasdk`, for the entry and the camera's permission; `tracing`.
- **Provides:** a component, from `cargo waos build -p camera-app`, and a package, `dev.waos.camera`, from `cargo waos package -p camera-app`.
- **Used by:** whoever opens it in WaOS.
