# Does the media model built on Apple's systems fit Windows, Linux, Android and the web?

Research for `waos/docs/proposals/2026-10-29-media.md`, decisions 1–4. 2026-10-28. Nothing under `/Users/mac/code` was changed.

It builds on two earlier reports and does not repeat them: `experiments/2026-10-17-interface-research/reports/surface-placement.md` (how each system keeps a layer's placement in step with a frame) and `reports/media.md` (sound, cameras, players). Where a claim comes from them it is marked **[read, prior]** or **[measured, prior]**, and their sources apply.

Every claim is marked:

- **[read]**: read in documentation, a header, a specification or source code, cited below;
- **[measured, prior]**: a number from an experiment under `experiments/`;
- **[inferred]**: my conclusion from what was read, not checked by running anything.

## The model under test

1. **Compositor.** `Compositor::open_surface(view, opaque) -> Surface` gives a native layer beneath the window's GPU drawing. The framework paints a clear hole there, and a frame's scene places each surface (position, clip, scale) in the same transaction as the frame. A producer presents buffers to a surface from any thread, and each buffer comes back when the compositor is done with it.
2. **Nesting (WaOS).** A program's picture is itself a surface in WaOS's window, and the program's own surfaces are its children, beneath its buffer, with one clip and scale for the group.
3. **Player.** One object made from a source, with controls and observable state, and two destinations: `show_on(surface)`, where the system draws the video into the surface's layer (needed for DRM and HDR; zero-copy), and `read_frames()`, which hands over the newest picture each time as a GPU-importable buffer. The v1 interface says a player may go to "a surface, a frame reader, or both".
4. **Camera.** A session on one device with outputs, each the system's own: preview (on a surface), frames, photos (encoded by the system) and recording (encoded and written to a file by the system).
5. **The rule.** A platform interface reports only what the system itself provides, and answers none where it provides nothing.

## The answer in short

- **The model holds on all four.** Every system has a native layer that can sit beneath an app's GPU drawing, can hold child layers beneath a parent's picture, and can take a placement with the app's frame. Each also has a system-drawn video layer and a way to hand frames to the GPU. Windows and Android have every camera output as a system output. The web has every camera output except in Firefox. Linux has them only if GStreamer counts as the system.
- **Four things in the model do not hold everywhere:**
  1. A player going to **both** destinations at once.
  2. A player shown on **more than one** surface.
  3. **Clipping** a system-drawn video on Linux.
  4. **Recording to a path** on the web.
- **One thing the model leaves undefined:** what counts as **"the system"** on Android and Linux.
- **Inset's hosts need real work, but not a different model.** Windows needs a DirectComposition tree. Android needs Inset to present its own frames as buffers rather than through the window's swapchain. Wayland needs per-child crop and scale. The web needs DOM elements and a main-thread canvas.
- **Frame-atomic placement is not free on two systems.** On Windows it needs composition textures, which are Direct3D 11 only. On Android it needs Inset's frames on a `SurfaceControl`. Without these, a placement can land a refresh apart from the frame, which is where Inset's macOS host already is outside resizes **[read, prior]**.

---

## 1. Compositor surfaces beneath the app's GPU drawing

### Table

| System | Native layer under the app's drawing | Child layers beneath a parent's picture, one clip and scale for the group | Placement in the frame's transaction, with wgpu presenting | Verdict |
|---|---|---|---|---|
| **Windows** | DirectComposition visuals. wgpu draws into an embedder's visual through `SurfaceTargetUnsafe::CompositionVisual` (DX12 only), with premultiplied alpha **[read]** | A visual's bitmap is "always drawn behind the children of that visual" **[read]**. So a surface is a container visual whose children come first and whose own picture sits in a top child. Clip and transform on the container apply to the group **[read / inferred]** | `Commit` is atomic for the visual tree **[read, prior]**. A DXGI swapchain's presents are "not synchronized with the layer tree" **[read]**. Atomic content needs an `IDCompositionSurface` (a copy) or composition textures, which are D3D11 only **[read]** | **Fits with a change.** Inset's Windows host must own a DComp tree and use DX12. Frame-atomic placement needs composition textures. |
| **Linux, Wayland** | `wl_subsurface` of winit's `wl_surface`, placed below it **[read]**. The host adopts winit's display (`Backend::from_foreign_display`) **[read]**. Mesa offers premultiplied alpha **[read]** | Subsurfaces nest arbitrarily, and `place_below(parent)` is legal **[read]**. There is no group clip ("not automatically clipped to its parent's area") and no group transform **[read / inferred]** | Position and stacking are "double-buffered state on the parent", applied by the parent's commit **[read]**. Mesa commits inside `vkQueuePresentKHR` **[read]**, and winit commits only at creation **[read]**. Crop and scale (`wp_viewport`) are the child's own state **[read]** | **Fits with host work.** Each child gets its own crop and scale, synchronized around geometry changes. |
| **Linux, X11** | Child windows only | — | Nothing joins a window move to another window's swap **[inferred, prior]** | **No surfaces.** Answers none. |
| **Android** | A child `ASurfaceControl` of the window (`ASurfaceControl_createFromWindow`, API 29) **[read]**. "Surfaces with a negative Z will be placed below the parent surface" **[read]**. A child at z −1 composites beneath NativeActivity's window once the window is RGBA_8888 **[measured, prior]** | `setCrop` "bounds the surface and its children" (API 31) **[read]**. `setPosition` is "in the parent's space" (API 31) **[read]** | Inset presents through wgpu's swapchain on the window **[read]**. In a NativeActivity, `applyTransactionOnDraw` applies at once **[read in AOSP]**. Atomic placement needs Inset's frame as an `AHardwareBuffer` on its own `SurfaceControl`, in one `ASurfaceTransaction` with the children **[read API; buffer path measured, prior]** | **Fits with a change** to Inset's Android presentation. Below API 29/31 it answers none. |
| **Web** | An element beneath the canvas. The WebGPU canvas has `alphaMode: "premultiplied"` **[read]** | A container element with `overflow`/`clip-path` and a CSS transform **[inferred]**. Protected video may be limited to axis-aligned rectangles **[read]** | The WebGPU canvas updates at the Document's "update the rendering" **[read]**, the step that also renders DOM changes **[inferred]**. A worker's `OffscreenCanvas` updates at the worker's own step **[read]**, so it is not joined **[inferred]** | **Fits** while the canvas is on the main thread. |

### Windows

- **What Inset does today.** Inset's winit host uses wgpu's default `DxgiFromHwnd` swapchain. That swapchain offers only `CompositeAlphaMode::Opaque`, so no hole can show through **[read: `wgpu-hal-30.0.1/src/dx12/adapter.rs`]**.
- **What the host must do instead.** It owns an `IDCompositionDevice`, a target and a visual tree, and hands wgpu one visual. wgpu documents this route: a user "should create their own device and composition, and pass the relevant visual down via `SurfaceTargetUnsafe::CompositionVisual`" **[read: `wgpu-types-30.0.1/src/backend.rs`]**. Visual targets support `PreMultiplied` alpha **[read]**. wgpu's Vulkan backend has no composition target **[inferred]**, so this means DX12.
- **Order of a visual's bitmap and its children.** A visual's bitmap is "always drawn behind the children of that visual" **[read: `IDCompositionVisual::SetContent`]**. So "child surfaces beneath its own buffer" becomes a container visual. Its children are added first, and its picture is the content of a child visual on top. This is the same three-level shape as the macOS host's clip layer and buffer layer **[read: Inset `composition/mod.rs`]**. Transforms on ancestor visuals apply to their children **[read: `SetContent`]**, so one clip and scale on the container covers the group **[inferred for `SetClip`]**.
- **Atomicity.** DirectComposition applies a whole `Commit` in one frame **[read, prior]**. A DXGI swapchain is different. Chromium found it "can be put into an overlay, saving power, but updates to it don't happen in sync with updates to the layer tree", while an `IDCompositionSurface` stays in sync but cannot be an overlay **[read: codereview 2743663006]**.
  - Composition textures bind a Direct3D texture to a visual with an availability fence. Microsoft states "The composition textures API supports Direct3D 11 only" **[read]**.
  - Chromium reaches D3D12 composition textures only through `EXPERIMENTAL_IDCompositionDevice6` from an experimental header (`experimental-composition/experimental-dcomp.h`), behind a feature flag **[read: Chromium `ui/gl/dc_layer_tree.h`, `direct_composition_support.cc`]**.
  - So with wgpu's DX12 swapchain, a placement can land one refresh apart from the frame. For Inset's own frame to be atomic, wgpu's output would have to reach DComp as a D3D11 composition texture through a shared handle **[inferred]**.
- **How producers present.**
  - The composition swapchain (`IPresentationManager`, Windows 11 22000.194, WDDM 2.0) presents registered buffers into composition surface handles. It updates "one or more presentation surfaces atomically" on its own queue, and signals an "available" event per buffer **[read]**. That matches `Surface::present` and its release callback **[inferred]**.
  - Media Foundation hands out composition surface handles "pre-bound to a presentation surface" **[read]**.
  - wgpu can also render into such a handle (`SurfaceTargetUnsafe::SurfaceHandle` → `CreateSwapChainForCompositionSurfaceHandle`) **[read: wgpu-hal `dx12/mod.rs`]**.
- **WaOS.** A program's picture is a container visual in WaOS's tree. Each program gets its own DComp device, so each program commits on its own. "Independent commits from different threads use one device each over a shared visual tree" **[read, prior]**. The program's frame is atomic with its children's placements only if that frame is a composition texture **[inferred]**.

### Linux, Wayland, under winit

- **Getting hold of winit's surface.** raw-window-handle gives "a pointer to a `wl_surface`" and the `wl_display` **[read: raw-window-handle 0.6 `unix.rs`]**. wayland-backend can adopt a foreign display and wrap a foreign object (`from_foreign_display`, `ObjectId::from_ptr`) **[read: wayland-backend 0.3.17]**. So the host can make subsurfaces of winit's surface on winit's own connection **[inferred]**.
- **Who commits winit's surface.**
  - winit commits its `wl_surface` once at creation ("XXX Do initial commit."). For redraws it only adds a frame callback, with no commit **[read: winit 0.30.13 `wayland/window/mod.rs`, `state.rs`]**.
  - Mesa's Wayland WSI calls `wl_surface_commit` and `wl_display_flush` inside `wsi_wl_swapchain_queue_present` **[read: mesa `src/vulkan/wsi/wsi_common_wayland.c`]**.
  - So `set_position` and `place_below` sent before the present arrive before Mesa's commit, and apply with that frame **[inferred from libwayland sending requests in call order]**.
  - Mesa reports `VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR | VK_COMPOSITE_ALPHA_PRE_MULTIPLIED_BIT_KHR` **[read]**. Other Vulkan drivers' WSI was not checked.
- **What the protocol says.**
  - "Sub-surfaces can be arbitrarily nested and create a tree of sub-surfaces." **[read: `wl_subcompositor`]**
  - The reference for `place_above`/`place_below` "must be one of the sibling surfaces, or the parent surface" **[read]**.
  - Position and z-order are "double-buffered state on the parent surface" **[read]**.
  - A sub-surface "is not automatically clipped to its parent's area" **[read]**.
  - "Destroying a sub-surface takes effect immediately", so removal goes by attaching a NULL buffer and committing the parent **[read]**.
- **Desynchronized children.**
  - A video producer runs on its own clock, so its child must be desynchronized. A synchronized child's commits wait for the parent's commit **[read: `wl_surface.commit`]**.
  - But `wp_viewport`'s crop and destination size are the child's own double-buffered state **[read: `viewporter.xml`]**. In desync mode they apply on the child's commit, not with the frame.
  - GStreamer's waylandsink solves this by switching the child to `set_sync`, changing it, committing, committing the parent, then `set_desync` **[read: `gstwlwindow.c`, `gst_wl_window_update_geometry`]**. Inset would do the same whenever a scene changes a child's clip or scale **[inferred]**.
- **Group clip and scale.** Wayland has no subtree clip and no subtree transform **[inferred from the protocol's per-surface `wp_viewport`, `buffer_scale` and `buffer_transform`]**. So the group's clip becomes each child's viewport source crop, and its scale becomes each child's viewport destination and position. Scaling a program view (WaOS's cards) means recomputing every child **[inferred]**.
- **WaOS.** The program's picture is a desynchronized subsurface P of WaOS's window. Its children are subsurfaces of P placed below P. Their positions apply with P's commit, which is the program's frame. P's own position applies with WaOS's commit **[read semantics; inferred arrangement]**.
  - A subsurface with no buffer is unmapped, and its children are hidden too ("These rules apply recursively") **[read]**. So a container with no picture of its own needs a buffer, for example a transparent one scaled by a viewport **[inferred]**.
  - The stacking rule implies that a child placed below P is still drawn above WaOS's own buffer **[inferred]**: each parent has one stack of itself and its children, and that subtree stacks as a unit.

### Android

- **The layers.** `ASurfaceControl_createFromWindow` "Creates an ASurfaceControl with either ANativeWindow or an ASurfaceControl as its parent" (API 29) **[read: NDK `surface_control.h`]**. The NDK's `setZOrder` speaks of siblings only. The Java `Transaction.setLayer` adds: "Surfaces with a negative Z will be placed below the parent surface" **[read: AOSP `SurfaceControl.java`]**. SurfaceView itself sits at `APPLICATION_MEDIA_SUBLAYER` beneath its window **[read: AOSP `SurfaceView.java`]**.
- **Measured on the emulator.** A child at z −1 under NativeActivity's window is composited only after the window format is RGBA_8888. With NativeActivity's default format the layer is `opaque=1` and the child is left out **[measured, prior: `2026-09-26-android-frame-path`]**. NativeActivity sets `PixelFormat.RGB_565` by default **[read: `NativeActivity.java`]**.
- **Why placement cannot ride on the swapchain.**
  - Inset presents through wgpu's Vulkan swapchain on the window **[read: `inset-embedder-android/src/surface.rs`]**.
  - The NDK has no call that joins an `ASurfaceTransaction` to that swapchain's next buffer **[inferred from `surface_control.h`]**.
  - The Java route fails in a NativeActivity. `ViewRootImpl` skips hardware acceleration when the app owns the surface ("If the application owns the surface, don't enable hardware acceleration"). Then `applyTransactionOnDraw` takes its `!isHardwareEnabled()` branch and calls `t.apply()` at once **[read: AOSP `ViewRootImpl.java`]**.
  - Flutter's HC++ depends on exactly that call, from a hardware-accelerated `FlutterView` (`rootSurfaceControl.applyTransactionOnDraw(tx)` after `invalidate()`) **[read: `PlatformViewsController2.java`]**.
- **The fit.** Inset renders each frame into an `AHardwareBuffer` and sets it on its own child `SurfaceControl` (`ASurfaceTransaction_setBuffer`, API 29). Media children go beneath it, and one `ASurfaceTransaction` carries the buffer and every child's `setPosition`/`setCrop`/`setScale` (API 31) **[read]**. Chromium does the same **[read, prior]**. Nothing else draws into a NativeActivity window, so the host applies the transaction itself **[inferred]**.
- **wgpu's gap.** wgpu-hal 30 has no `AHardwareBuffer` import (a grep finds none) **[read]**. The WaOS experiment imported buffers with raw Vulkan, `VK_ANDROID_external_memory_android_hardware_buffer` and `texture_from_raw` **[measured, prior]**.
- **API levels.** Inset's default `minSdkVersion` is 26 **[read: `inset-cli/src/project.rs`]**. Surfaces need API 29, geometry 31, and `ASurfaceControl_fromJava` 34 **[read]**. Below these, the platform answers none. WaOS's mobile minimum is 36 **[read, prior]**.

### Web

- **The canvas.** Its `alphaMode` is `"opaque"` or `"premultiplied"` **[read: WebGPU §canvas]**. A WebGPU canvas updates "in the following sub-steps of the event loop processing model: 'update the rendering or user interface of that Document' / 'update the rendering of that dedicated worker'" **[read]**.
- **Main thread versus worker.** A DOM change and a canvas frame made in the same main-thread task are rendered in the same update **[inferred from HTML's event loop and the step above]**. A canvas in a worker updates on the worker's own timeline, so nothing joins its frame to a DOM placement **[inferred]**. Inset's web host draws on the main thread today **[read: `inset-embedder-web/src/gpu.rs`]**.
- **Protected video.** "Where media rendering is not performed by the UA, for example in the case of a hardware-based media pipeline, the full set of HTML rendering capabilities, for example CSS Transforms, MAY be unavailable. One likely restriction is that video media MAY be constrained to appear only in rectangular regions with sides parallel to the edges of the window and with normal orientation." **[read: EME §7.6]** The model's placement (position, clip, scale) stays within that.
- **Producer-fed surfaces.** The web has no general way to present a GPU buffer into a DOM layer other than a canvas, so such a surface is another canvas **[inferred]**.

---

## 2. The player's two destinations

### Table

| System | `show_on(surface)`: system-drawn layer, DRM, HDR | `read_frames()`: newest picture as a GPU-importable buffer | Both at once | One player on several surfaces | Verdict |
|---|---|---|---|---|---|
| **Windows** | `IMFMediaEngine` in rendering mode, into a DComp visual (`MF_MEDIA_ENGINE_PLAYBACK_VISUAL`) **[read]**. Or windowless swapchain mode → `GetVideoSwapchainHandle` → `CreateSurfaceFromHandle` → `SetContent`, which is Chromium's PlayReady path **[read]**. HDR: Chromium queries HDR support in the same service **[read]**; that HDR renders in the DComp path is **[inferred]**. WinRT `MediaPlayer.GetSurface` needs a `Windows.UI.Composition` compositor, not DComp **[read]** | Frame-server mode: `OnVideoStreamTick` → `TransferVideoFrame` into a D3D11 texture, scaled and letterboxed **[read]**. Protected content needs a separate call **[read]**. Import: Vulkan `texture_from_d3d11_shared_handle` **[read]**, or D3D12 `OpenSharedHandle` → `texture_from_raw` **[inferred]** | **No.** The modes are distinct, and `OnVideoStreamTick` must not be called in rendering mode **[read]**. One engine cannot do both **[inferred]** | `GetSurface` may be called several times **[read]** | Fits, except "both". |
| **Linux** | GStreamer `waylandsink`, given the surface's `wl_surface`. It adds its own desynchronized subsurfaces above that surface, placed by a render rectangle **[read]**. HDR: "Parse and set the HDR10 metadata" over `wp_color_management_v1`, GStreamer 1.28 **[read]**. DRM: no system path **[inferred; only secondary sources]**. Clip: no **[inferred]** | `appsink` with dmabuf from hardware decoders **[inferred]**. wgpu-hal `texture_from_dmabuf_fd` takes single-plane buffers only **[read]**, so NV12 needs a conversion to RGBA first **[inferred]** | Yes, with a `tee` **[inferred]** | Yes, with a `tee` **[inferred]** | Fits, if GStreamer counts as the system. System-drawn video cannot be clipped. |
| **Android** | A decoder renders into a `Surface` made from the surface's `SurfaceControl` (`new Surface(SurfaceControl)`, Java, API 29) **[read]**. Driven by framework `MediaPlayer.setSurface` **[read]** or Media3 `setVideoSurface` **[read]**. DRM needs secure output to an overlay plane; HDR is kept **[read, prior]** | `AImageReader` with `AIMAGE_FORMAT_PRIVATE`, then `AImage_getHardwareBuffer` **[read]** and the newest image each time. The player's output goes there via `ANativeWindow_toSurface` (API 26) **[read]**. A YCbCr pass is needed for vendor formats **[read, prior]** | **No.** A codec renders into one output window (`AMediaCodec_configure`; `setOutputSurface` replaces it) **[read signatures; inferred meaning]** | No: one output surface **[inferred]** | Fits, except "both" and several surfaces. |
| **Web** | `<video>` is both player and layer **[read, prior]**. DRM through EME; protected media "MAY be unavailable" to `drawImage` and the like **[read]** | `requestVideoFrameCallback` (Chrome 83, Firefox 132, Safari 15.4), `new VideoFrame(video)` (Chrome 94, Firefox 130, Safari 16.4), `importExternalTexture` (Chrome 113 on some platforms, wider from 144; Safari 26; Firefox 144 on Windows only) **[read: MDN browser-compat-data]** | Yes **[inferred]** | No: an element is in one place **[inferred]** | Fits, except several surfaces. |

### Notes

- **Windows' system-drawn layer is a DComp visual, as `AVPlayerLayer` is a `CALayer`.**
  - The Media Engine's rendering mode renders "to a window or Microsoft DirectComposition visual provided by the application". `MF_MEDIA_ENGINE_PLAYBACK_HWND` and `MF_MEDIA_ENGINE_PLAYBACK_VISUAL` "are mutually exclusive. Setting either of these attributes puts the Media Engine into rendering mode." **[read: `IMFMediaEngineClassFactory::CreateInstance`]**
  - Chromium uses the windowless swapchain route: `EnableWindowlessSwapchainMode(true)`, then `GetVideoSwapchainHandle`. The GPU process calls `dcomp_device_->CreateSurfaceFromHandle(surface_handle, &dcomp_surface)` and makes that the content of a visual in its tree **[read: `media/renderers/win/media_foundation_renderer.cc`, `ui/gl/swap_chain_presenter.cc`]**. The renderer keeps "the DRM context (PlayReady trust chain…)" **[read]**.
  - So `show_on` on Windows hangs a visual inside the surface's container visual **[inferred]**.
- **WinRT `MediaPlayer` is the wrong compositor for a DComp host.** `GetSurface(Compositor)` takes `Windows.UI.Composition.Compositor` and returns a `CompositionSurface`. It "allows you to render the player's contents using a Compositor without relying on the XAML framework" **[read]**. A host on raw DirectComposition should use the Media Engine instead **[inferred]**.
- **Linux: waylandsink cannot be clipped by Inset's scene.**
  - `gst_wl_window_new_in_surface(display, parent)` creates `area_subsurface` as a child of the app's surface and calls `wl_subsurface_set_desync`. Inside it, a video subsurface is also desynchronized. The area's position is set from the render rectangle, "needs a parent commit to apply" **[read: `gstwlwindow.c`]**.
  - A new subsurface starts "top-most in the stack of its siblings and parent" **[read]**, and waylandsink never restacks it. So Inset's surface must be a mapped container placed below the window, with waylandsink inside it **[inferred]**.
  - waylandsink's source crop comes from the buffers' crop metadata, not from the app. Subsurfaces are not clipped by their parents. So a video half-scrolled out of a list draws outside its clip **[read for both mechanisms; inferred consequence]**.
- **Android: `show_on` needs a Java `SurfaceControl`.** The NDK cannot make a `Surface`/`ANativeWindow` from an `ASurfaceControl` **[read: no such call in `surface_control.h` or `native_window_jni.h`]**. The route is to build the child `SurfaceControl` in Java through JNI, parented to the window with `AttachedSurfaceControl.buildReparentTransaction` (API 31) **[read: android.jar]**. `new Surface(sc)` then goes to the decoder, and `ASurfaceControl_fromJava` (API 34) **[read]** puts the same layer in the host's NDK transaction with Inset's frame.
- **Apple does both.** `AVPlayerLayer` and `AVPlayerItemVideoOutput` attach to the same player **[read, prior]**. Windows and Android do not, as the table shows.

---

## 3. The camera session and its outputs

### Table

| System | Session | Preview on a surface | Frames | Photo, encoded by the system | Recording, encoded and written by the system |
|---|---|---|---|---|---|
| **Windows** | `IMFCaptureEngine` (Win32), or WinRT `MediaCapture` **[read]** | Yes: the preview sink's `SetRenderSurface` takes "a DirectComposition visual" **[read]** | Yes: the sinks' `SetSampleCallback` **[read]**, and `MediaFrameReader` **[read, prior]** | Yes: the photo sink writes a file or byte stream after `TakePhoto` **[read]**. `CapturePhotoToStreamAsync` with `ImageEncodingProperties.CreateJpeg` **[read]** | Yes: the record sink "creates compressed audio/video files", and "the capture engine then multiplexes" into MP4/ASF **[read]**. `PrepareLowLagRecordToStorageFileAsync` with `MediaEncodingProfile.CreateMp4` **[read]** |
| **Linux: portal, PipeWire, libcamera** | The portal has only `AccessCamera`, `OpenPipeWireRemote` and `IsCameraPresent` **[read]**. libcamera's roles say only what a stream is for **[read]** | No | Yes: PipeWire buffers, often dmabuf **[read, prior]** | No: `StillCapture` yields frames, and nothing in the stack encodes **[inferred]** | No **[inferred]** |
| **Linux: GStreamer `camerabin`** | `camerabin`, in gst-plugins-bad: "Take image snapshots and record movies from camera" **[read]** | Yes, as a `viewfinder-sink`. GNOME's app uses `gtk4paintablesink` **[read]**; `waylandsink` would draw into the surface **[inferred]** | Yes, through the viewfinder sink **[inferred]** | Yes: `mode-image`, `location`, `start-capture` **[read: GNOME Snapshot's Aperture]** | Yes: `mode-video`, `location`, `start-capture`/`stop-capture` **[read]** |
| **Android** | Camera2 in the NDK (`ACameraDevice`, `ACaptureSessionOutput`) **[read]**, or CameraX, an AndroidX library | Yes, into any `ANativeWindow`, such as a `Surface` from the surface's `SurfaceControl` **[read / inferred]** | Yes: `AImageReader` **[read]** | Yes: `AIMAGE_FORMAT_JPEG`, "always supported as an output format for the android Camera2 NDK API" **[read]** | NDK: no recorder. Framework `MediaRecorder` with a surface source (`getSurface`, `setInputSurface`, `setOutputFile(FileDescriptor)`) through JNI **[read: android.jar]**. Or `AMediaCodec_createInputSurface` with `AMediaMuxer`, where the host drives the muxing **[read]** |
| **Web** | `getUserMedia` | Yes: `<video srcObject>` **[inferred]** | `MediaStreamTrackProcessor`: Chrome 94, Safari 18, **not Firefox**. `new VideoFrame(video)` works everywhere, Firefox from 130 **[read: BCD]** | `ImageCapture.takePhoto()`: Chrome 60, Safari 18.4, **Firefox only behind a flag** **[read: BCD]** | `MediaRecorder` everywhere **[read: BCD]**. It hands back encoded chunks, not a file at a path **[inferred]** |

### Notes

- **Windows maps one to one onto `AVCaptureSession`.** Each Capture Engine sink "must specify a single destination. Multiple destinations are not supported" for the photo and record sinks **[read]**. The preview sink's page does not say whether it can render and call back at once **[not found]**. So preview plus frames may take the record sink's callback or a `MediaFrameReader` beside it **[inferred]**. Whether `SetRenderSurface` commits the app's DComp device itself is not documented **[not found]**. It matters for atomicity and needs an experiment.
- **Android's four outputs at once are not guaranteed on every device.** CameraX allows "one instance each of `Preview`, `VideoCapture`, `ImageAnalysis`, and `ImageCapture`". On hardware level `FULL` or lower, the combination "may force CameraX to duplicate the camera's `PRIV` stream", which is CameraX's own work **[read: CameraX architecture]**. Plain Camera2 offers per-level stream combinations **[read, prior]**. A combination the device lacks answers none **[inferred]**.
- **Linux is a definition question.** GNOME's camera app, Snapshot, builds on Aperture. Aperture makes a `camerabin`, sets `mode-image` or `mode-video` with a `location`, and emits `start-capture` **[read: `aperture/src/viewfinder.rs`]**. If GStreamer is Linux's media system, Linux has every output. If only the portal, PipeWire and libcamera count, Linux has frames only.

---

## 4. What would need Java on Android

Inset's Android host already calls framework classes from Rust through the `jni` crate, with no Java source. One Java class forwards `onActivityResult`, which NativeActivity keeps to itself **[read: `inset-embedder-android/src/display.rs`, `InsetActivity.java`]**. `inset-cli` compiles that class with `javac` and `d8` against `android.jar` alone. It has no dependency resolution for AndroidX archives **[read: `inset-cli/src/android.rs`]**.

| Piece | Java source needed? | Why |
|---|---|---|
| Child `ASurfaceControl`s, `ASurfaceTransaction`, `AImageReader`, `AHardwareBuffer`, Camera2 (NDK), `AMediaCodec`, `AMediaMuxer` | No | C APIs with C callbacks **[read: NDK headers]** |
| A `Surface` for a decoder or camera from a layer: `SurfaceControl.Builder`, `new Surface(SurfaceControl)`, `AttachedSurfaceControl.buildReparentTransaction` | No: JNI calls | Plain Java methods **[read: android.jar]** |
| Framework `MediaPlayer` | A small class | `setSurface` and `prepareDrm` are calls **[read]**. State arrives through listener interfaces (`OnPreparedListener` and others), and native code cannot implement a Java interface without a class **[inferred from JNI]**. One class can forward them, as `InsetActivity` forwards results. |
| Framework `MediaRecorder` | No **[inferred]** | Calls only; its listeners are optional |
| Asking for `CAMERA` / `RECORD_AUDIO` | One more override in `InsetActivity` | `requestPermissions` is a call, but the answer arrives in `onRequestPermissionsResult`, which NativeActivity does not override **[read: `NativeActivity.java`]** |
| Media3 `ExoPlayer` | Yes, plus an AndroidX build | It is an AndroidX library (`androidx.media3:media3-exoplayer`) **[read]**. It "must be accessed from a single application thread" with a `Looper` **[read]**. `Player.Listener` needs a class **[inferred]** |
| CameraX | Yes, plus an AndroidX build | Use cases are bound with `bindToLifecycle(lifecycleOwner, …)` **[read]**. NativeActivity is not a `LifecycleOwner`, so a custom one is needed, which the docs allow **[read; inferred for NativeActivity]** |
| Playing on in the background | Yes | A foreground service is a `Service` subclass declared in the manifest **[read, prior: service types; inferred class]** |

**The rule's edge on Android.** Google calls Media3 "the recommended approach for media… To create a new app, use Jetpack Media3 instead of the MediaPlayer APIs" **[read]**. But Media3 and CameraX ship inside the app, not with the system. The system's own pieces are `MediaPlayer`, Camera2 and `MediaRecorder`.

---

## Misfits that would change the model

1. **"Both destinations at once" is not a system capability on Windows or Android.**
   - Windows: the Media Engine's rendering and frame-server modes are distinct, and `OnVideoStreamTick` "Do not call this method in rendering mode" **[read]**.
   - Android: a codec renders into one output surface **[inferred]**.
   - Apple, GStreamer and the web can do both.
   - Under the rule, the host must not copy frames out of the layer to make up for it. So a player's destination should be one or the other, or "both" answers none on Windows and Android.
2. **One player on several surfaces is not portable.** The web's `<video>` is in one place, and an Android codec feeds one surface **[inferred]**. Windows allows several (`GetSurface` "multiple times") **[read]**, and Apple too **[inferred]**. `show_on` should take one surface at a time.
3. **On Linux, a system-drawn video cannot be clipped by the frame's scene.** Subsurfaces are not clipped by their parents, and waylandsink takes no crop from the app **[read; inferred consequence]**. Linux has no system DRM, so the layer's reasons there are HDR and power. Choices for the lead:
   - (a) `show_on` answers none on Linux, and players use frames that Inset's surface presents and crops. Inset then passes colorimetry for HDR itself.
   - (b) Accept unclipped video on Linux.
   - (c) Make clips a request the platform may refuse.
4. **What "the system" is must be decided for Android and Linux.**
   - Android: is Media3 (recommended by Google, bundled in the app) the system, or only `MediaPlayer`, Camera2 and `MediaRecorder`?
   - Linux: is GStreamer (what GNOME's camera app uses) the system? If not, Linux has no player and no photo, recording or preview outputs.
5. **Recording "to a path" is the system's own job on Windows and Android, not on the web.** Windows' record sink writes a file **[read]**, and `MediaRecorder.setOutputFile(FileDescriptor)` does too **[read]**. The web's `MediaRecorder` hands the app encoded chunks **[inferred]**, so WaOS writes the bytes. This is storage plumbing rather than standing in for an encoder, but the rule as written should say so.

Missing outputs simply answer none: Firefox lacks `ImageCapture` and `MediaStreamTrackProcessor` **[read: BCD]**, Android devices lack some stream combinations, X11 has no surfaces, and old Android API levels lack surfaces. Each answer is the rule working, not a misfit.

## What changes in Inset's hosts, leaving the model as it is

- **Windows.**
  - Own a DirectComposition tree. Give wgpu a visual through `CompositionVisual` (DX12, premultiplied alpha).
  - Make each surface a container visual with its own picture in a top child.
  - For frame-atomic placement, present Inset's frame as a composition texture: D3D11 only, so DX12 output goes through a shared handle. Otherwise accept a possible refresh of skew, as macOS has today outside resizes.
  - Producers present through the composition swapchain or composition textures. The Media Engine and the Capture Engine attach to visuals directly.
- **Linux, Wayland.**
  - Make subsurfaces of winit's surface on its connection, with a premultiplied swapchain.
  - Give each child its own viewport crop and size from the group's clip and scale, switching it to `set_sync` around geometry changes.
  - Give empty containers a transparent buffer.
  - X11 answers none.
- **Android.**
  - Stop presenting through the window's swapchain. Render into `AHardwareBuffer`s (raw Vulkan import, since wgpu has none) and set them on Inset's own `SurfaceControl`, with media children at negative z, in one transaction.
  - Make the window RGBA_8888.
  - Gate surfaces on API 29/31, and on API 34 for `fromJava`.
- **Web.** Make surfaces DOM elements under a premultiplied canvas, and keep Inset's canvas on the main thread.
- **macOS, already noted in v1.** Shrink the group with a transform on the container, not with each buffer's `contentsScale` **[read, prior]**.

## Experiments worth running

1. **Wayland.** Does a `set_position` sent before `vkQueuePresentKHR` always land in Mesa's commit? Count misplaced refreshes while scrolling a hole over a child, with and without `set_sync` around viewport changes.
2. **Windows.** Measure the skew between a DXGI swapchain under DComp and a `Commit` that moves a sibling visual. Measure the cost of a D3D11 composition texture fed from wgpu's DX12 through a shared handle.
3. **Windows.** Does `IMFCapturePreviewSink::SetRenderSurface` or rendering mode commit the app's DComp device itself?
4. **Android.** Make a `Surface(SurfaceControl)` reparented under a NativeActivity window through `AttachedSurfaceControl`. Play Widevine L1 content into it, on a phone.

---

## Sources

### Windows
- `IDCompositionVisual::SetContent`: https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nf-dcomp-idcompositionvisual-setcontent
- `IDCompositionTexture` ("supports Direct3D 11 only"): https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nn-dcomp-idcompositiontexture
- Composition swapchain guide: https://learn.microsoft.com/en-us/windows/win32/comp_swapchain/comp-swapchain
- Chromium on swapchains and DComp surfaces: https://codereview.chromium.org/2743663006/
- Media Engine modes: https://learn.microsoft.com/en-us/windows/win32/api/mfmediaengine/nf-mfmediaengine-imfmediaengineclassfactory-createinstance
- `EnableWindowlessSwapchainMode`: https://learn.microsoft.com/en-us/windows/win32/api/mfmediaengine/nf-mfmediaengine-imfmediaengineex-enablewindowlessswapchainmode
- `GetVideoSwapchainHandle`: https://learn.microsoft.com/en-us/windows/win32/api/mfmediaengine/nf-mfmediaengine-imfmediaengineex-getvideoswapchainhandle
- `TransferVideoFrame`: https://learn.microsoft.com/en-us/windows/win32/api/mfmediaengine/nf-mfmediaengine-imfmediaengine-transfervideoframe
- `OnVideoStreamTick`: https://learn.microsoft.com/en-us/windows/win32/api/mfmediaengine/nf-mfmediaengine-imfmediaengine-onvideostreamtick
- `MediaPlayer.GetSurface`: https://learn.microsoft.com/en-us/uwp/api/windows.media.playback.mediaplayer.getsurface
- Capture Engine sinks: https://learn.microsoft.com/en-us/windows/win32/api/mfcaptureengine/nn-mfcaptureengine-imfcapturepreviewsink , https://learn.microsoft.com/en-us/windows/win32/api/mfcaptureengine/nf-mfcaptureengine-imfcapturepreviewsink-setrendersurface , https://learn.microsoft.com/en-us/windows/win32/api/mfcaptureengine/nn-mfcaptureengine-imfcapturephotosink , https://learn.microsoft.com/en-us/windows/win32/api/mfcaptureengine/nn-mfcaptureengine-imfcapturerecordsink
- `MediaCapture` photo and video: https://learn.microsoft.com/en-us/windows/apps/develop/camera/basic-photo-capture
- Chromium (local checkout, 2026-08-15): `media/renderers/win/media_foundation_renderer.cc`, `ui/gl/swap_chain_presenter.cc`, `ui/gl/dc_layer_tree.{h,cc}`, `ui/gl/direct_composition_support.cc`, `media/mojo/services/media_foundation_service.cc`
- wgpu 30.0.1 (cargo registry): `wgpu-hal/src/dx12/{adapter.rs,mod.rs,dcomp.rs,device.rs}`, `wgpu-hal/src/vulkan/device.rs` (`texture_from_d3d11_shared_handle`, `texture_from_dmabuf_fd`), `wgpu-types/src/backend.rs`, `wgpu/src/api/surface.rs`
- Flutter engine (local): `shell/platform/common/public/flutter_texture_registrar.h`, `shell/platform/windows/flutter_windows_internal.h`

### Linux
- `wayland.xml` (`wl_subcompositor`, `wl_subsurface`, `wl_surface.commit`), from wayland-client 0.31.15 in the cargo registry; upstream https://gitlab.freedesktop.org/wayland/wayland/-/blob/main/protocol/wayland.xml
- `viewporter.xml`, `color-management-v1.xml`, from wayland-protocols 0.32.13 in the cargo registry
- Mesa Wayland WSI: https://gitlab.freedesktop.org/mesa/mesa/-/raw/main/src/vulkan/wsi/wsi_common_wayland.c
- winit 0.30.13 (cargo registry): `src/platform_impl/linux/wayland/window/{mod.rs,state.rs}`; raw-window-handle 0.6 `src/unix.rs`; wayland-backend 0.3.17 `src/sys/client_impl/mod.rs`
- GStreamer waylandsink: https://raw.githubusercontent.com/GStreamer/gstreamer/main/subprojects/gst-plugins-bad/gst-libs/gst/wayland/gstwlwindow.c , https://raw.githubusercontent.com/GStreamer/gstreamer/main/subprojects/gst-plugins-bad/ext/wayland/gstwaylandsink.c
- GStreamer 1.28 release notes: https://gstreamer.freedesktop.org/releases/1.28/
- `camerabin`: https://gstreamer.freedesktop.org/documentation/camerabin/index.html
- GNOME Snapshot's Aperture: https://github.com/GNOME/snapshot/blob/main/aperture/src/viewfinder.rs
- Camera portal: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Camera.html
- libcamera `StreamRole`: https://raw.githubusercontent.com/libcamera-org/libcamera/master/src/libcamera/stream.cpp
- Desktop Widevine as L3 (secondary only): https://bunny.net/docs/stream/widevine-security-levels , https://www.wikipedia.org/wiki/Widevine

### Android
- NDK headers (NDK 30.0.16248370, local): `android/surface_control.h`, `android/surface_control_jni.h`, `android/native_window_jni.h`, `android/hardware_buffer.h`, `media/NdkImage.h`, `media/NdkImageReader.h`, `media/NdkMediaCodec.h`, `camera/*.h`; reference https://developer.android.com/ndk/reference/group/native-activity
- AOSP `frameworks/base` main: `core/java/android/view/SurfaceControl.java`, `ViewRootImpl.java`, `SurfaceView.java`, `Surface.java`, `core/java/android/app/NativeActivity.java` (https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/)
- `android.jar` (API 37, local), read with `javap`: `MediaPlayer`, `Surface`, `MediaRecorder`, `AttachedSurfaceControl`
- Media3: https://developer.android.com/media/media3/exoplayer/hello-world ; surfaces https://developer.android.com/media/media3/ui/surface
- MediaPlayer overview: https://developer.android.com/media/platform/mediaplayer
- CameraX architecture: https://developer.android.com/media/camera/camerax/architecture
- Flutter engine (local): `shell/platform/android/io/flutter/plugin/platform/PlatformViewsController2.java`, `io/flutter/embedding/engine/renderer/FlutterRenderer.java`
- Inset: `crates/inset-cli/android/rs/inset/InsetActivity.java`, `crates/inset-cli/src/{android.rs,project.rs}`, `crates/inset-embedder-android/src/{surface.rs,gpu.rs,display.rs}`
- Experiment: `experiments/2026-09-26-android-frame-path/README.md`

### Web
- WebGPU: https://www.w3.org/TR/webgpu/ (canvas alpha modes, "update the rendering", `importExternalTexture`)
- Encrypted Media Extensions §7.6: https://www.w3.org/TR/encrypted-media-2/
- MDN browser-compat-data: https://github.com/mdn/browser-compat-data (`api/ImageCapture.json`, `MediaStreamTrackProcessor.json`, `MediaRecorder.json`, `VideoFrame.json`, `GPUDevice.json`, `HTMLVideoElement.json`)
- Inset: `crates/inset-embedder-web/src/gpu.rs`

### Prior reports
- `experiments/2026-10-17-interface-research/reports/surface-placement.md`
- `experiments/2026-10-17-interface-research/reports/media.md`
