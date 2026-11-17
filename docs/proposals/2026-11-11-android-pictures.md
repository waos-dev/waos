# A program's pictures on Android, composited by Android

**Decisions taken (2026-11-11),** by the lead, on the design below and on the experiment that followed it, on an emulator and an API 30 phone; the design is kept as it was written, and where a decision differs from it, the decision holds.

1. **A program's picture on Android is composited by Android,** as macOS composites it: each layer an `ASurfaceControl` under WaOS's window, its buffers `AHardwareBuffer`s. Not drawn by WaOS into its own picture, a stand-in to throw away.
2. **What belongs in Waft is designed with the lead and built in Waft,** never worked around in WaOS.
3. **Waft's Android view draws into a surface control of its own** (2.1's A), so its picture and its layers land in one transaction, as Chromium's does. Not a swapchain on a translucent window, whose layers land a frame off.
4. **The deck first, tasks of their own after** (question 2): programs as tabs in WaOS's one activity, which `2026-10-29-android-apps.md`'s decision 2 keeps behind a setting.
5. **Android 12 is the minimum, one path** (question 3). The experiment found `ASurfaceTransaction_setGeometry`, the only call that places a layer on Android 10 and 11, right for a single layer but not for a layer holding others: the Pixel 2 XL on Android 11 squeezes a child that reaches outside its container, and Android 17 crops nothing with it. Android 10 and 11 would need a second layout in Waft's host, every layer flat under the window and placed again as its parent moves, which costs more than it gives for now; it can be added later inside the host, with no interface changed.
6. **The two usage flags are added** (question 5), `composer-overlay` and `cpu-read`, to Waft's `HardwareBufferUsage` and the WIT's `hardware-buffer-usage`: the emulator refuses to lock a buffer allocated without a CPU usage.
7. **`present` carries no fence for now** (question 4): waiting for the GPU before handing a frame over costs about 3 ms a frame (3.0 ms on the Pixel, 3.5 ms on the emulator, for a clear), which wgpu 30, able neither to make nor to wait on a fence, cannot yet save. A known gap, with that number. **Amended 2026-11-12: Waft's own view hands Android a fence, as Flutter does.** The 3 ms was a clear's; a real frame on the Pixel (the Cupertino gallery scrolling, 1440 by 2880) kept the loop waiting 35 to 40 ms, so frames reached the screen every 50 ms where the same picture through a swapchain came every refresh, and the swapchain with the same wait added was as slow: the wait on the loop's thread was the cause, not the composition. Flutter's Impeller, whose AHardwareBuffer swapchain has this shape (`impeller/renderer/backend/vulkan/swapchain/ahb`), waits for nothing: the frame's last submission signals a semaphore exported as a sync file descriptor, which is the buffer's fence in the transaction, with at most two presents pending. wgpu 30 can do the same, which was missed: its Vulkan queue takes a semaphore for its next submission to signal (`add_signal_semaphore`), and Waft opens its own device, so it enables the export. Waft's Android host does so for its own view; `present` still carries no fence for other producers, whose wait is theirs: the engine's is on a thread of its own, off the program's loop, and a program's frames came every refresh on the Pixel.
8. **A frame is handed over without the transfer to the compositor's queue family** (question 6), which wgpu-core has no call for: the picture was right both ways on both devices, with no validation layer to say it is safe. A known gap. **Amended 2026-11-12: the handover is done.** A buffer whose last write was a copy showed black on the emulator, and showed right once released to `VK_QUEUE_FAMILY_FOREIGN_EXT`, a change of layout alone not helping (the experiment's section 8); a draw's showing without it is the driver's leniency. So whoever hands a buffer to Android releases its image to the foreign queue family after the frame's work, keeping the layout wgpu tracks, in a raw command encoder of its own (`CommandEncoder::as_hal_mut`), and acquires it back before the buffer is drawn into again: in Waft's Android host and in the engine's Android side, each within its Android code, no interface changed.
9. **The rest as recommended:** a tab's deck shows in `waos phone` on the Mac (question 7), and `import_texture` answers none on a device an app opened itself (question 8).
10. **A buffer answers what it can be used for (2026-11-12).** The usage a buffer is allocated for is the least it is for, and the buffer itself answers all it is for, `hardware-buffer.usage`, in the WIT and in Waft's `HardwareBuffer::usage`. Every check reads that one usage: `read-pixels` is refused (`unsupported`) exactly when it lacks `cpu-read`, on every system, and a texture imported of the buffer allows what it allows. Apple's systems answer every flag for a buffer the program allocates, whatever it asked, since any `IOSurface` can be sampled, drawn into, shown by Core Animation and read by the CPU; a buffer the system hands out, a decoded image's or a video frame's, answers what the system made it for, `gpu-sampled`, with `composer-overlay` and `cpu-read`, which every `IOSurface` has. Android answers what the `AHardwareBuffer` says it was allocated for (`AHardwareBuffer_describe`), `CPU_READ_RARELY` and `CPU_READ_OFTEN` both being `cpu-read`, so Waft's Android buffer reads its usage from the buffer rather than being told it. Other systems answer what was asked. Why: before, `cpu-read` was enforced on Android and ignored on Apple's systems, so one flag meant two things; now one check means the same everywhere, and nothing a system can do is hidden from the program. The precedent: Android's `AHardwareBuffer_describe` answers a buffer's usage, and Linux's GBM (`gbm_bo_create`'s flags), Windows' D3D11 (`D3D11_TEXTURE2D_DESC`'s `BindFlags` and `CPUAccessFlags`) and Chromium's `gfx::BufferUsage` all declare a buffer's usage at allocation; Apple's `IOSurface` is the exception that needs none. What it costs is a portability trap: a program that relies on what Apple gives beyond what it asked works there and is refused on Android. So when a program reads the pixels of a buffer it allocated without `cpu-read`, which works only because the system gives the buffer more, the engine logs one warning under the program's log target, once per program, saying that it works here and is refused on systems that give a buffer only what was asked, such as Android. A texture sampled or drawn into without the flag asked is the same trap and is not warned of: the engine sees a texture imported, not what the program does with it, and a warning at the import would reach programs that ask for all they use.

The host waits on a buffer's release fence itself: on the Pixel, Android's `OnComplete` arrives 5 to 6 ms before the fence signals.

For the project lead, 2026-11-11. A design only: nothing in `waos` or `waft` was changed. It grounds each statement in the code of `waos` as it was that day and of `waft` at ee95c56 (whose working tree holds an uncommitted Android vsync waiter, `crates/waft-embedder-android/src/vsync_waiter.rs`, which this design builds on), and in the NDK headers as AOSP's `main` has them (`include/android/surface_control.h`, `libs/nativewindow/include/android/hardware_buffer.h`, `include/android/choreographer.h`, which the developer.android.com NDK reference is generated from; the API level of each call is the header's `__INTRODUCED_IN`).

## 1. The mental model: triangle in a tab

| step | macOS today | Android, as designed |
| --- | --- | --- |
| where its view comes from | the program asks; its own app's window answers (`logic/program_apps`) | the deck gives the tab's view at the start (`WaOS::run(…, views)`, `crates/waos/src/engine.rs:178`); a request is refused |
| its GPU device | wgpu-core's default Metal device | Vulkan, opened by the engine's `DeviceOpener` with `VK_ANDROID_external_memory_android_hardware_buffer` |
| the view's buffers | three `IOSurface`s, BGRA8 (`os/apple.rs`) | three `AHardwareBuffer`s, RGBA8 (`os/android.rs`) |
| a frame's texture | a `MTLTexture` over the surface, wrapped by wgpu-hal | a `VkImage` bound to the buffer's imported memory, wrapped by wgpu-hal |
| draw, wait, hand over | the finisher waits for the GPU, then `ShellView::present` → `CanvasLayer` → Waft's `CompositionLayer::present` | the same code, unchanged |
| what shows it | the layer's `CALayer.contents` = the surface, in the program's app's `CAContext` | `ASurfaceTransaction_setBuffer` on the layer's `ASurfaceControl`, a child of WaOS's window |
| where it is placed | the window's scene, in a `CATransaction` on the main thread, beneath the view's `CAMetalLayer` | the deck page's scene, beneath the view's own picture, in the same `ASurfaceTransaction` as that picture |
| when a buffer comes back | `IOSurfaceIsInUse` false, asked at the layer's next present | the release fence `OnComplete` hands over for the previous buffer, once signalled |
| the refreshes | the screen's display link | `AChoreographer`, already served (`vsync_waiter.rs`) |

The same: every trait (Waft's and the engine's shell), the engine's pool, finisher and frame pacing, the program's WIT. What differs: the buffer type and its one format, an extension on the program's device, a release by fence, and the program's view living in WaOS's own window rather than an app of its own.

## 2. Waft on Android: does the API fit as it is?

**Mostly yes.** Every trait maps onto Android's own objects with no change to its shape. Four things are not settled by the API as it stands, and the first is not an API question but the biggest one for the host:

- **How the view's own picture lands with its layers (2.1).** Waft's contract asks a host to place a scene's layers "beneath the picture its slices make … in the same native transaction" (`waft-embedder/README.md:112`; decision 31 of `2026-11-02-program-media.md` names Android's way of keeping layers with the window's frame). A Vulkan swapchain on the window cannot join an `ASurfaceTransaction`, so the view must draw into buffers of its own that the same transaction sets, as Chromium does on Android 10 and later with its `SurfaceControl` output (`ui/gl/gl_surface_egl_surface_control.cc`, from memory, not read for this). The API is unchanged by it; the host changes inside.
- **Two usage flags (2.3):** a buffer shown through `ASurfaceTransaction_setBuffer` should carry `COMPOSER_OVERLAY`, and `read_pixels` works on Android only for a buffer allocated with a `CPU_READ_*` usage. Both are additions to `HardwareBufferUsage` and to the WIT's `hardware-buffer-usage`.
- **Fences (2.4):** Android hands out an acquire fence and a release fence; the API carries neither. Decision 28 of `2026-10-30-native-surfaces.md` left fences "designed with the second host", which Android is.
- **The minimum API level (2.6):** `ASurfaceControl` is API 29, its position, scale and crop API 31; WaOS builds for API 26 (`waft-cli/src/android.rs:40`).

### 2.1 The view's picture and the window's surface

Today `AndroidView` presents through a valo surface on the `ANativeWindow` (`waft-embedder-android/src/view.rs:66`, `surface.rs`). `NativeActivity` makes that window opaque: its `onCreate` calls `getWindow().setFormat(PixelFormat.RGB_565)` (AOSP `core/java/android/app/NativeActivity.java:135`).

- **A (recommended): the view's picture is a surface control of its own, like any layer.** The host makes, under the window, one `ASurfaceControl` for the view's content (`ASurfaceControl_createFromWindow`, API 29), draws the slices' picture with room (`slices(…).picture_with_room()`, as today) into one of a few `AHardwareBuffer`s through a texture imported on Waft's own device, and each frame applies one transaction: the content's buffer (topmost), every scene layer's placement and z-order beneath it, the layers no longer placed reparented to none ("surface controls with a null parent do not appear on the display", `ASurfaceTransaction_reparent`, API 29). Frame and layers land together, as the README asks. The window's own surface is never drawn into, so its format and alpha stop mattering, and the view becomes "a composition layer's content", as on macOS (`os/apple/composition/view_layer.rs`), which later lets Android add views on layers of the app's own (native-surfaces decision 33). Cost: the host renders into its own buffers and keeps them (three, released by fence, as a program's are).
- **B: keep the swapchain, make the window translucent, put the layers beneath it.** `ANativeActivity_setWindowFormat(activity, WINDOW_FORMAT_RGBA_8888)` keeps the window's alpha, its clear holes show children ordered below it. The layers' placements go in a transaction of their own beside `vkQueuePresentKHR`, so a moving layer is a frame off its hole, which breaks Waft's own contract. And whether a child shows below its parent's buffer at all is undocumented: the header says only that z order "is relative to other surfaces which are siblings" (`ASurfaceTransaction_setZOrder`). Not recommended.

On a host whose device is OpenGL ES (`gpu.rs:25`, the fallback when Vulkan gives none), there is no import into Waft's device, so A cannot draw: `compositor()` answers `None` there and the view keeps its swapchain. That is the platform's own absence, not a second path chosen for convenience.

### 2.2 Each trait and method against Android

| Waft item (`waft-embedder`) | Android's object | verdict |
| --- | --- | --- |
| `Platform::compositor()` (`platform.rs:683`) | the window's `ASurfaceControl` tree | fits: `Some` on Vulkan from the API level 2.6 settles, `None` otherwise, visibly |
| `Compositor::make_layer(opaque)` (`composition.rs:41`) | `ASurfaceControl_create(parent, name)`, API 29, whose `parent` is `_Nonnull` | fits: a layer is a host record first; its surface controls are made at its first placement, under the placer, and moved with `ASurfaceTransaction_reparent` after, including to a new window when Android gives one back after the background. "Made on its own, adopted by whoever places it" is exactly that. |
| `opaque` | `ASurfaceTransaction_setBufferTransparency(OPAQUE / TRANSLUCENT)`, API 29: "whether the content … is completely opaque" | fits: OPAQUE for an opaque layer with no children, TRANSLUCENT while a present gives it children (as `composition.rs:36` already asks). On Android "opaque" lets SurfaceFlinger skip blending and what lies beneath; it does not change the pixels. |
| `CompositionLayer::present(buffer, children, released)` (`composition.rs:68`) | one `ASurfaceTransaction` (API 29, "applied atomically"): `setBuffer`, children's `reparent`, `setZOrder`, `setPosition`/`setScale`/`setCrop` (API 31), `apply` | fits. A layer is two surface controls, as Apple's is three `CALayer`s: a container the placement moves, scales and crops ("bounds the surface and its children", `setCrop`), holding the children (z 0…n-1) and, on top, the buffer's surface control (z n). This keeps "children beneath the buffer" to siblings, which is what the header documents. |
| … the buffer | `setBuffer(…, buffer, acquire_fence_fd)`: the buffer "must be allocated with `GPU_SAMPLED_IMAGE`" | fits with `-1` as the fence: every producer presents after the GPU is done (the engine's finisher, `hardware_buffers/finisher.rs`). See 2.4 for the fence. |
| … `released` | `setOnComplete` (API 29) + `ASurfaceTransactionStats_getPreviousReleaseFenceFd`, "the fence used to signal the release of the PREVIOUS buffer set on this surface"; from API 36 `setBufferWithRelease` with `OnBufferRelease(context, release_fence_fd)` | fits: the host waits for the fence on a thread of its own (a `poll` on the fds) and then calls `released`, for the buffer replaced, which matches "may learn only at the layer's next present". The power hidden is the fence itself (2.4). |
| … children "let go" | `reparent(child, null)` | fits |
| `CompositionLayer::current_picture()` (`composition.rs:78`) | none: Android reads nothing back from a surface control | fits: the host keeps the last buffer presented, as Apple's does. `LayerPicture::Frame` waits for video on Android. |
| `Placement` and its `Mutator`s (`scene/placement.rs:20`) | translation, scale and an axis-aligned crop (API 31); buffer rotations by 90° (`setBufferTransform`); alpha (`setBufferAlpha`) | fits: no arbitrary matrix, rounded or path clip in the NDK, so the host reads `bounds`, `scale` and `visible_bounds`, which the module offers "a compositor that only moves and clips rectangles", and the README's "as much … as the compositor can". A deck's rounded card shows right because the picture above covers the corners (09-26 proposal's table). Coordinates are physical pixels from the top-left, as `Placement`'s are: no scale step as Apple's points need. |
| `HardwareBufferAllocator::allocate` (`hardware_buffers.rs:47`) | `AHardwareBuffer_allocate(&AHardwareBuffer_Desc)`, API 26; `AHardwareBuffer_isSupported`, API 29 | fits: RGBA8 only (`AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM`); the public formats have no BGRA, as the doc says already. Needs 2.3 for its usage. |
| `HardwareBufferUsage` (`hardware_buffers.rs:61`) | `AHARDWAREBUFFER_USAGE_*` | needs a change (2.3) |
| `HardwareBuffer::width/height` | `AHardwareBuffer_describe`, API 26 | fits |
| `HardwareBuffer::read_pixels` (`hardware_buffers.rs:91`) | `AHardwareBuffer_lock(buffer, CPU_READ_*, fence, …)`, API 26, which fails with `-EINVAL` when "the usage flags are not a combination of `AHARDWAREBUFFER_USAGE_CPU_*`" the buffer was allocated with | needs a change (2.3) |
| `Platform::import_texture(buffer, device)` (`platform.rs:665`) | a `VkImage` over imported memory: `vkGetAndroidHardwareBufferPropertiesANDROID`, `VkImportAndroidHardwareBufferInfoANDROID`, a dedicated allocation; the extension needs Vulkan 1.1 (or its KHR parts) and `VK_EXT_queue_family_foreign` (Khronos reference page) | fits: `None` for a device opened without the extension, which today is every wgpu device, Waft's own included (`gpu.rs:48`, through `valo::request_device`; wgpu-hal 30 never enables it). Waft's host opens its device through wgpu-hal's `Adapter::open_with_callback` (`wgpu-hal-30.0.1/src/vulkan/adapter.rs:2995`) with the extension added. Open question 8 for an app's own devices. |
| `Platform::vsync_waiter()` (`platform.rs:690`) | `AChoreographer` | fits, and is built (uncommitted). `AChoreographer_postFrameCallback` is deprecated from API 29 for `postFrameCallback64`; from API 33 `postVsyncCallback` gives `getFrameTimelineExpectedPresentationTimeNanos`, a real `display_time` in place of `at + period` (`vsync_waiter.rs:39`). |

**What Android has that the API cannot say**, each a known gap already named or not needed for this step: a present at a time (`setDesiredPresentTime`, API 29; native-surfaces decision 32); a layer's frame rate (`setFrameRate`, API 30), so a 24 fps video lets the panel slow down; damage (`setDamageRegion`); colour space and HDR (`setBufferDataSpace`, `setHdrMetadata_*`; decision 28); formats beyond RGBA8 (`R16G16B16A16_FLOAT`, `R10G10B10A2_UNORM`), which valo's `PixelFormat` has no case for (`Rgba8`, `Bgra8`); a solid colour layer (`setColor`); back pressure (`setEnableBackPressure`, API 31); the vsync id a transaction targets (`setFrameTimeline`, API 33).

**What the API has that Android lacks:** BGRA buffers; arbitrary transforms and rounded or path clips on a layer; a system layer whose current frame is read back (Apple's video output) until Android's video lands; `import_texture` on any device of the right backend, which Apple allows and Android only on a device opened for it.

### 2.3 Usage: the compositor and the CPU join

`HardwareBufferUsage` says "the CPU, the system's compositor and a video encoder join as they are needed" (`hardware_buffers.rs:58`). Android needs two of them now:

```rust
/// What a buffer is allocated for, named after its users, as Android's `AHardwareBuffer` usage is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HardwareBufferUsage {
    /// The GPU samples it in its shaders: Android's `GPU_SAMPLED_IMAGE`.
    pub gpu_sampled: bool,
    /// The GPU draws into it and copies into it: Android's `GPU_COLOR_OUTPUT`.
    pub gpu_color_output: bool,
    /// The system's compositor may show it without the GPU, as a display overlay: Android's
    /// `COMPOSER_OVERLAY`. Apple's systems show any buffer and ignore it.
    pub composer_overlay: bool,
    /// The CPU reads it, rarely (`read_pixels`): Android's `CPU_READ_RARELY`. A buffer without it
    /// answers `Unsupported` from `read_pixels` there; Apple's systems read any buffer.
    pub cpu_read: bool,
}
```

Why: the header says `COMPOSER_OVERLAY` "is currently only needed when using `ASurfaceTransaction_setBuffer`", since the framework adds it to its own buffers and not to ones from `AHardwareBuffer_allocate`; without it SurfaceFlinger composites the layer on the GPU, which works and costs power. And `AHardwareBuffer_lock` refuses a buffer allocated without a CPU usage, so `read_pixels` either says so (this proposal) or the host copies through the GPU behind the program's back, which the WIT's "every move between them is a call of its own" (`hardware-buffers.wit:19`) rules out. The WIT's `hardware-buffer-usage` gains `composer-overlay` and `cpu-read` first, then Waft's mirror (decision 36 of program-media). The engine's own view buffers are allocated by the engine and take `GPU_COLOR_OUTPUT | GPU_SAMPLED_IMAGE | COMPOSER_OVERLAY` themselves.

### 2.4 Fences

Android carries an acquire fence into `setBuffer` and a release fence out of `OnComplete`; Core Animation takes none (`composition.rs:64`). Two shapes, as optional parts:

```rust
/// A sync fence: Android's sync file descriptor, which signals once GPU work is done.
pub struct SyncFence(std::os::fd::OwnedFd);

fn present(
    &self,
    buffer: Arc<dyn HardwareBuffer>,
    /// Signals once the producer's GPU work on `buffer` is done; `None` for a buffer whose work
    /// is done already. Apple's hosts wait for it before they show the buffer.
    ready: Option<SyncFence>,
    children: Vec<(Arc<dyn CompositionLayer>, Placement)>,
    /// Runs once the compositor no longer needs the buffer, with the fence to wait for before
    /// writing it, where the system gives one; `None` where it is free already.
    released: Box<dyn FnOnce(Option<SyncFence>) + Send + Sync>,
);
```

Neither producer can use them yet: wgpu 30 neither exports a sync file from a submission nor waits on one at submit without hal hooks of its own, and the engine waits for the GPU on the CPU (the finisher) anyway. Recommendation in question 4: keep `present` as it is for now, the host waiting on the release fence itself, and take this shape when a producer can make or consume one.

### 2.5 What Waft's Android host gains (in `crates/waft-embedder-android`, Waft's working tree)

- `surface_control.rs`: the NDK's surface control and transaction calls, which neither `ndk` 0.9 nor `ndk-sys` 0.6 binds, resolved from `libandroid.so` at run time so the library still loads on the app's minimum API (`vsync.rs:7` records why a symbol above it cannot be linked directly).
- `hardware_buffers.rs`: `HardwareBufferRef` behind Waft's `HardwareBuffer` (`pub struct AndroidHardwareBuffer`, what WaOS downcasts to, as `SurfaceBuffer` on Apple) and `GrallocAllocator: HardwareBufferAllocator`.
- `gpu.rs`: the device opened through `open_with_callback` with the import's extensions; `gpu_interop.rs`: `import_texture`.
- `composition/`: `SurfaceControlCompositor: Compositor`, `SurfaceControlLayer: CompositionLayer` (container and buffer surface controls, the buffers presented, the children placed), `releases.rs` (the fence-waiting thread), and `view_layer.rs`, the view's own content drawn into its buffers (2.1 A).

### 2.6 API levels

| call | API |
| --- | --- |
| `AHardwareBuffer_allocate`, `describe`, `lock` | 26 |
| `ASurfaceControl_createFromWindow`, `create`, `ASurfaceTransaction_*` (`setBuffer`, `setZOrder`, `reparent`, `setOnComplete`, `setBufferTransparency`, `setBufferAlpha`), `AHardwareBuffer_isSupported`, `AChoreographer_postFrameCallback64` | 29 |
| `setPosition`, `setScale`, `setCrop`, `setBufferTransform`, `setOnCommit`, `ASurfaceControl_acquire`, `AHardwareBuffer_getId` | 31 |
| `setGeometry`, the only way to move and crop at 29–30 | 29, deprecated: "use setCrop, setPosition, setBufferTransform, and setScale instead" |
| `AChoreographer_postVsyncCallback`, frame timelines, `setFrameTimeline` | 33 |
| `setBufferWithRelease` | 36 |

At 29–30 a layer's children cannot be cropped with their container, so the tree would be flattened into absolute rectangles under the window by the host, a second layout of its own. Question 3.

## 3. The engine's Android side

### 3.1 `crates/waos/src/os/android.rs`, beside `apple.rs`

The same names, so the rest of the crate stays free of `cfg` (`os/mod.rs:1`); `os/mod.rs` picks `android` for `target_os = "android"` and `generic` for what is neither.

```rust
/// The format a hardware buffer is allocated in, and so the only one a canvas is configured with:
/// WebGPU's preferred canvas format here (`waos-webgpu-host/src/lib.rs:48`).
pub(crate) const FORMAT: wgpu_types::TextureFormat = wgpu_types::TextureFormat::Rgba8Unorm;
pub(crate) const ALLOCATES: bool = true;
/// Vulkan's guaranteed `maxImageDimension2D`.
pub(crate) const LARGEST_SIDE: u32 = 4096;

/// One hardware buffer's memory, as the engine holds it: the `AHardwareBuffer`, which a program's
/// device draws into through a texture made over it for each frame, and which the shell is handed
/// as the [`HardwareBuffer`] it shows. Cloning it is an `AHardwareBuffer_acquire`.
#[derive(Clone)]
pub(crate) struct HardwareBufferMemory {
    buffer: HardwareBuffer,
    width: u32,
    height: u32,
    /// The screenshots reading it now, which hold back its release to the pool (`in_use`).
    readers: Arc<Readers>,
}
// new, allocate, of, buffer, width, height, bytes, read_pixels, program_texture,
// sampled_texture, texture_over, in_use: as `apple.rs`, over `ImportedImage`.

/// A `VkImage` bound to a hardware buffer's imported memory on one device, which wgpu-hal takes
/// as a texture whose memory it does not own (`TextureMemory::External`), destroyed with its
/// memory in the texture's drop callback.
struct ImportedImage { /* image, memory, device */ }

/// A hardware buffer read by a screenshot, until this goes. Android has no use count the
/// compositor and the engine share, as `IOSurface`'s, so the engine keeps its own.
pub(crate) struct InUse(Arc<Readers>);

/// A video's frame: none on Android yet (`Infallible`), as `generic.rs` has it.
pub(crate) struct FrameMemory(Infallible);

/// How a program's device is opened here: on Vulkan, with the extensions that import a hardware
/// buffer's memory added through wgpu-hal's `open_with_callback`.
pub(crate) struct HardwareBufferImports;
```

The shell's `HardwareBuffer` (`shell/mod.rs:986`, an alias of the system type on Apple, an empty enum elsewhere) becomes on Android a newtype over `ndk::hardware_buffer::HardwareBufferRef`, which `ndk` 0.9 leaves neither `Send` nor `Sync`, with the `unsafe impl`s and the reason, as `apple.rs:75` vouches for `IOSurface`.

Three things that differ from Apple's and that `apple.rs` does not have to do:

1. **Queue-family ownership.** An image whose memory another user reads is released to `VK_QUEUE_FAMILY_FOREIGN_EXT` after drawing and acquired back before (the reason the extension depends on `VK_EXT_queue_family_foreign`). wgpu-core records no such barrier for a texture it holds, and a raw submit on its queue from outside races its own. wgpu-hal points at the need (`queue_family_index`, `vulkan/device.rs:813`) but offers no call. Question 6.
2. **A screenshot's hold.** On Apple a screenshot's `in_use` keeps the surface from coming back through `IOSurfaceIsInUse` (`screenshots/mod.rs:130`). On Android the release comes from the fence, which knows nothing of the engine's reader, so the engine wraps each `released` it hands the shell to wait until the buffer's readers are gone: one more method, `HardwareBufferMemory::after_reads(released) -> released`, the identity on Apple.
3. **`read_pixels`** locks with `CPU_READ_RARELY` and answers `Unsupported` for a buffer without it (2.3).

### 3.2 The device hook

It exists: `waos-webgpu-host`'s `DeviceOpener` (`device_request.rs:26`), called from `request_device` in place of `adapter_request_device` (`trait_impls.rs:1022`), "an embedder whose frames the system compositor reads needs the device made with backend extensions wgpu-core does not enable". The engine passes none today, and says Android brings one (`gpu/mod.rs:114`). On Android it answers `Some(&HardwareBufferImports)`:

```rust
impl DeviceOpener for HardwareBufferImports {
    fn open(
        &self,
        instance: &wgpu_core::global::Global,
        adapter: wgpu_core::id::AdapterId,
        descriptor: &wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>,
    ) -> Result<(DeviceId, QueueId), RequestDeviceError> {
        // A program on an adapter that is not Vulkan's, or a Vulkan without the extensions, gets
        // wgpu-core's device: its canvas then fails to configure, saying why.
        let Some(vulkan) = vulkan_adapter_with_imports(instance, adapter) else {
            return instance.adapter_request_device(adapter, descriptor, None, None);
        };
        let open = vulkan.open_with_callback(features, &limits, &hints, Some(Box::new(|args| {
            args.extensions.extend([
                ash::android::external_memory_android_hardware_buffer::NAME,
                ash::ext::queue_family_foreign::NAME,
            ]);
        })))?;
        instance.create_device_from_hal(adapter, open.into(), descriptor, None, None)
    }
}
```

`open_with_callback` (`wgpu-hal-30.0.1/src/vulkan/adapter.rs:2995`) lets a callback add extensions; `create_device_from_hal` (`wgpu-core-30.0.1/src/instance.rs:1287`) takes the opened device. The opener refuses a feature the adapter lacks itself, as the trait says.

Alternatives: our own `vkCreateDevice` and wgpu-hal's `device_from_raw` (`adapter.rs:2570`), the same result with more of wgpu-hal's setup copied; a new hook in the WebGPU host, not needed; and upstream wgpu-hal enabling the extension where it is supported and offering `texture_from_android_hardware_buffer` beside its `texture_from_dmabuf_fd` (`vulkan/device.rs:525`), the right end state, offered later, since nothing here waits on it.

## 4. The deck in the WaOS app

### 4.1 Types, one job each

```rust
// logic/tab_placement.rs
/// A phone's placement: a program has the view its tab gave it at the start, and a request for
/// another is refused, as the views WIT says of a phone.
pub(crate) struct TabsOnly;

impl ViewPlacement for TabsOnly {
    fn view_requested(&self, program: &RunningProgram, _request: ViewRequest) -> Result<(), ViewError> {
        tracing::info!(program = program.name(), "a view was asked for in a tab: refused");
        Err(ViewError::Unsupported)
    }
}

// state/deck/tabs.rs
/// The programs open in tabs, in the order opened, and the one in sight: none while the library
/// is.
pub(crate) struct Tabs {
    tabs: Vec<Tab>,
    current: Option<ProgramId>,
}

/// One tab: the program it holds and how far it got.
pub(crate) struct Tab {
    program: ProgramId,
    stage: TabStage,
}

pub(crate) enum TabStage {
    /// The program is loading; the tab shows its icon.
    Starting,
    /// The program runs, its view shown by the tab.
    Running(Entity<ShownView>),
    /// The program ended on its own; the tab says so until the user closes it.
    Ended(ProgramEnd),
}

// state/deck/tab_view.rs
/// The view a tab gives its program as it starts: the canvas's layer, made on Waft's compositor,
/// and the queue its calls arrive on, as a `PlacedView` with no window and no request.
pub(crate) struct TabView { canvas: Arc<CanvasLayer>, calls: UnboundedSender<ViewCall> }
```

Opening a program from the library on a phone opens a tab: `Tabs` makes the `TabView`, with the page's metrics (the deck page's box in physical pixels, the system bars as `padding`), hands its `PlacedView` (built with `waos_program_window::CanvasView`, `window: None`, `request: None`) to the run, and once the run answers its `ProgramView`, shows it. `InstalledRuns::open` gains the views to start with (`installed_runs.rs:45` passes `Vec::new()` today), the desktop passing none. A tab in sight reports `Presence { on_screen: true, .. }`, the others `false`, so the engine stops their frames.

The deck reuses `waos-program-window`'s `ProgramCanvas` and `ShownView`, but `ShownView` holds a `WindowRef` and reads its frame (`state/shown_view.rs:60`). A first commit splits it: the view's part (canvas, cursor, focus, metrics, presence) in `ShownView`, the window's part (frame, maximized, title bar) in `ViewWindows`' `ShownWindow`, so a tab's view has no window to pretend. It is the reshape the 09-26 proposal asked for ("`state/hosted_view.rs` stays, without its window").

### 4.2 Where it sits

The code today is `logic/`, `state/`, `ui/` with `cfg`s (`state/mod.rs:23`, `logic/mod.rs:39`) and a place chosen at the entry, `LibraryPlace::Phone` (`logic/command_line.rs:25`, `android_entry.rs:19`), not the 09-26 proposal's `desktop/` and `phone/` modules. Recommendation: follow today's layout. `state/deck/` and `ui/deck/` compiled on every system, chosen by `LibraryPlace::Phone`, the placement `TabsOnly` then, so `waos phone` on a Mac shows the deck too and its tests run there (the 09-26 decision: "the form factor is a choice made at the entry, not a `cfg`"). On macOS that means a program shown in WaOS's own window, which `state/program_apps.rs:20` rules out ("macOS has no second route"): question 7. The `desktop/`/`phone/` split can come when the phone has more than one module of its own.

### 4.3 The simplest honest switching

One page in sight at a time, no animation: the library is the first page; a bar at the bottom names the tab in sight and has a tabs button; the tabs button shows a plain list of the tabs (no live cards), a tap switches, a close button stops the program without asking (as a phone's switcher ends an app). Only the tab in sight places its canvas's layer, so the others show nothing and draw nothing. Swipes, the overview of live cards and the launcher are the prototype's UI and wait (09-26 "record for the formal version").

## 5. The work, in steps

Each builds and passes on its own. Waft's steps are written in Waft's working tree and left uncommitted for the lead; WaOS's are committed on `main` once this design is settled.

1. **Experiment**, the rows that decide 2.1 and question 6 only: under `NativeActivity` on the emulator, (a) a child surface control with a buffer and a never-drawn window shows; (b) the emulator's Vulkan has `VK_ANDROID_external_memory_android_hardware_buffer` and `VK_EXT_queue_family_foreign`; (c) a wgpu-hal texture over an imported `AHardwareBuffer`, cleared red and set on the child, shows red, with and without the foreign transfer. Checked: three screenshots and the extension list.
2. **Waft:** the device opened through `open_with_callback` with the import extensions, `AndroidHardwareBuffer`, the allocator and `import_texture`. Checked on the emulator: an example allocates a buffer, draws into it and shows it as an image (`import_hardware_buffer`).
3. **Waft:** the view's picture in a surface control of its own (2.1 A), the swapchain kept for GLES. Checked: WaOS's library looks as before; resize and background-return still draw.
4. **Waft:** the compositor, layers, placements, fence releases, `current_picture`. Checked: an example with a `CompositionLayerView` shows a presented buffer beneath its picture, moved and cropped; releases come back (a count in the log).
5. **WaOS's WIT, then Waft's mirror (only if question 5 is taken):** `composer-overlay` and `cpu-read` in `hardware-buffer-usage`, then Waft's `HardwareBufferUsage`.
6. **WaOS:** `waos-program-window`'s `ShownView` without its window (a reshape commit). Checked: desktop tests pass, nothing changes on screen.
7. **WaOS:** the deck: `TabsOnly`, `Tabs`, the tab page and bar, opening from the library, the list. Checked on the emulator before 8: a program opens in a tab, gets its view (in the log, its `view-changes` carries it), and its canvas refuses to configure with `os::generic`'s message; switching and closing work. Can be done beside steps 2–4.
8. **WaOS:** `os/android.rs`, the engine's `HardwareBufferImports` opener, the shell's Android `HardwareBuffer`, and the glue that wraps it for Waft (`waos-program-window/src/logic/composition.rs:83`, `apps/waos/src/logic/hardware_buffers.rs:23`). Checked on the emulator: triangle draws in its tab at the display's rate, a hidden tab draws nothing, a screenshot reads it.

## 6. Open questions for the lead

1. **How Waft's Android view draws (2.1).** A: its picture in buffers it sets on a surface control in the same transaction as the layers; B: its swapchain on a translucent window, the layers in a transaction beside. Recommendation A: it is what Waft's contract and decision 31 ask, needs nothing undocumented, and makes Android's view a layer's content as macOS's is; its cost is the host's own small buffer pool.
2. **Deck or tasks first.** Decision 2 of `2026-10-29-android-apps.md` made a task per program Android's default and the deck a setting; decision 3 there makes a task Waft's window, which needs Waft's loop to outlive an activity first. Options: the deck first, as asked, with tasks after Waft's reshaping; tasks first; both now. Recommendation: the deck first; it needs no change to Waft's activity model and exercises the whole frame path, which tasks then reuse unchanged.
3. **The API level of Android's compositor.** Options: 31 and up, `compositor()` `None` below (Android 10 and 11 draw no program layers, a program's canvas refuses to configure there); 29 and up, the host flattening the tree with the deprecated `setGeometry`; raising WaOS's minimum to 29 or 31 outright. Recommendation: 31 for the compositor, resolved at run time so Waft still loads at 26; WaOS's minimum left as is.
4. **Fences in `present` (2.4).** Options: keep the API, the host waiting on the release fence on its own thread; add `ready` and the fence in `released` now, as optional parts. Recommendation: keep it now, since neither wgpu-core nor valo can make or wait on a sync file in wgpu 30, and record the shape in decision 28's place.
5. **`composer-overlay` and `cpu-read` in the usage (2.3).** Options: the two flags, in the WIT and Waft; the host adding both to every buffer; `read_pixels` copying through the GPU. Recommendation: the two flags: the cost of each is the program's to choose and see, and Android names them so.
6. **Queue-family ownership of a program's frame (3.1).** Options: ask wgpu upstream for a release and acquire of a hal texture to a foreign queue at submit, and meanwhile go without, if the experiment's row (c) shows the emulator correct without it; record the barrier with raw Vulkan on a command buffer of the engine's own; wait for upstream before drawing on Android. Recommendation: the experiment first, then the first option, recording the omission in `os/android.rs` and as a known gap, since without it the image's content after drawing is undefined by Vulkan's rules even where drivers show it right.
7. **The deck on a Mac.** Options: `waos phone` on macOS shows programs in WaOS's own window, so the deck is built and tested on the Mac; the deck only on Android, `waos phone` showing the library alone. Recommendation: on the Mac too, as an exception to "macOS has no second route" for the phone form factor, so the deck is tested where WaOS is developed.
8. **An app's own device and `import_texture` on Android.** Today an app gets a device that can import only if the host made it. Options: leave it (`None`, visibly), the renderer's device being the one that imports (`import_hardware_buffer`); a `Platform` call that opens a device able to import. Recommendation: leave it until an app needs its own; WaOS's programs get theirs from the engine's opener.
