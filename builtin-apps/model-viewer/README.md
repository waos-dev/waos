# model-viewer

Model Viewer: a glTF viewer, and the built-in app that is not a Waft app. It shows that a WaOS program can bring any framework that draws with wgpu: the scene is drawn with wgpu directly, over `wasdk-wgpu`, and the panel with egui, fed from the view's events by glue of the app's own, since there is no winit here.

A person opens a `.glb` or a self-contained `.gltf` with **Open…** (⌘O) or by opening the file with the app, and sees it lit and shaded; drags to orbit, Shift-drags or right-drags to pan, scrolls or pinches to zoom (the arrow keys and `+`/`-` do the same); picks one of the model's animations, pauses it and scrubs through it; and switches between shaded, wireframe and normals, with the ground's grid, the light, its direction and the exposure. The app opens with a sample model made in code (`src/model/sample.rs`), so the first launch shows something.

```sh
waos run -p model-viewer --release
```

Release, because a debug build is 44 MB of wasm, which WaOS takes long to compile; the release build is 7 MB.

- **Takes:** `wasdk`, for the view, its events, the open dialog, the files opened with the app, the clipboard and the system's light or dark; `wasdk-wgpu` and `wgpu`, for everything drawn; `egui` and `egui-wgpu`, for the panel; `gltf`, which reads a model's file and decodes its images; `glam` and `bytemuck`.
- **Provides:** a program, `dev.waos.modelviewer`.
- **Used by:** whoever opens it; whoever reads it for how a program is written without Waft, or how a UI framework is put on a view.

## How it is made

- `src/egui_view/` is egui on a view: what `egui-winit` is to a winit window. It turns a view's pointer events, keys, text input, focus, copy, cut and paste into egui's input, and carries out what egui asks of the platform after a pass: the cursor's shape, a copy, the input method's start and stop, and the next frame. It knows nothing of the app, so another egui program could take the folder as it is.
- `src/model/` is a model as the app holds it, glTF's shape cut down to what is drawn; `import.rs` fills it from a file, `sample.rs` in code.
- `src/render/` draws: `canvas.rs` is the view's surface with the multisampled colour and depth the scene is drawn into, `scene.rs` and `shaders/` the scene, `gpu_model.rs` a model's buffers, textures and draws, `panel_painter.rs` egui's output over the scene.
- `src/ui/` is the window's layout in egui: the panel beside the stage, or under it in a window narrower than 620 points.
- `src/app.rs` is the loop. Nothing is drawn until something asks the view for a frame: egui's glue for input, the app for a new size, a new model, or a running animation. An idle window draws nothing.

## What is drawn, and how honestly

- **Materials:** glTF's metal and roughness model, with base colour, metal and roughness, normal, occlusion and emissive textures, vertex colours, alpha masks and blending (blended surfaces sorted by their middles, farthest first), double-sided materials, unlit materials (`KHR_materials_unlit`) and `KHR_materials_emissive_strength`. A normal map on a mesh with no tangents is read along the directions its texture coordinates grow in on screen.
- **Light:** one directional light, with GGX's distribution, Smith's visibility and Schlick's Fresnel, and no shadows; and an environment that is a formula rather than a picture (a pale sky over a dark floor with one wide soft light), blurred by a surface's roughness, with Karis's fit of the environment's response. It reads as a studio, and it is not image-based lighting: no model reflects another, or itself.
- **Colour:** Khronos's PBR Neutral tone mapper, then sRGB's curve in the shader when the view's surface is not an sRGB format.
- **Wireframe:** the triangles' edges as a list of lines over the surfaces in one matte colour, since WebGPU has no polygon mode that outlines triangles.
- **Animations:** the nodes' translation, rotation and scale, with step, linear and cubic-spline keys, and skins (four joints a vertex, the joints' matrices in a storage buffer).
- **Textures:** uploaded with every smaller copy, each made on the CPU as the average of four pixels, and sampled as the file's sampler says.

## What is left out

- A `.gltf` whose buffers and images are files beside it: a program is given the file its user picked, not its folder, so the app says to export a single `.glb`.
- Morph targets, lines and points, the file's cameras and lights, a second set of texture coordinates, and the material extensions beyond unlit and emissive strength (clearcoat, transmission, sheen and the rest): a file that has them is shown without them.
- KTX2 and WebP images, Draco and meshopt compression, quantized meshes and texture transforms, where a file requires them: the app says which one the file needs, and keeps showing what it showed.
- Opening a `.glb` from the system's "Open with": a package declares an id, a name and an icon, and has nowhere to say which files it opens. A file WaOS hands the app (`lifecycle.open-requests`) is shown.
- Reading in the background: a file is read, its images decoded and its textures made on the app's one thread, so the window does not answer while a large one loads. "Opening…" is put on screen first.

## What was checked, and how

In WaOS on macOS, with `waos ui screenshot`, `tap`, `type`, `key` and `scroll`: the sample; five `.glb` files of up to 13 MB, with textures, blending and animations, and a small skinned `.gltf`, each opened from the app's `/data` by a hook since removed; a file refused for its compression and one for the files beside it; the panel's buttons, lists, sliders and a typed number; zoom by the wheel and orbit by the keys; the layout at 390 points wide; light and dark; and that no frame is drawn while nothing changes. Not checked, for want of a way to drive them: dragging, a trackpad's and a touch screen's gestures, copy and paste, an input method's composing, the open dialog beyond asking for it, and a file opened with the app.
