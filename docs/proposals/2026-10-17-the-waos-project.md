# The waos project: an embeddable engine, its capabilities, its shell, and wasdk

**Decisions taken (2026-10-17).** Designed with the lead, one question at a time, starting this project as the production one after the experiment in wasdk, this project's private predecessor, which is frozen as a reference. The lead set the starting points:
- **waos is the project and the platform; wasdk is what programs are written with.** The interface programs import keeps its name, the WIT package `wasdk:platform`.
- **waos is largely a reusable, portable library, with an app built on it.**
- **No native build of programs in the first version.** A program is a WASI component; building one as a native executable was wanted, but costs too much for now.
- **Remote machines are not a priority.**
- **The `.wapk` format is a crate of its own, `waos-package`.**

The lead then took these:
1. **The engine runs programs, capabilities serve them, and the shell is the embedding app.** The engine reaches nothing outside itself. Every service a program uses (files, network, processes, clipboard, sound devices, cameras, media, fonts, notifications, opening links and files) is a capability: a trait in the shape of its WIT interface, with a default for each OS, swappable by an embedder, chosen per program when it starts. What the person sees or decides (where views go, prompts, file dialogs, menus) is the shell's, implemented by the embedding app. Notifications are a capability whose default posts under the embedding app's identity; the WaOS app wraps it to post under each program's own.
2. **Capabilities are one crate per capability per kind of provider**, under `capabilities/` (`local-fs`, `local-clipboard`, later `ssh-fs`), each depending only on `waos-capabilities`, which holds the traits and depends on nothing of ours. `capabilities/local` assembles this computer's defaults. Tauri's plugins are the reference for this cut (`.reference/tauri-plugins`).
3. **The engine makes pictures and the shell shows them.** A picture is GPU memory the system's compositor can show (an `IOSurface`, an `AHardwareBuffer`); the engine creates, draws and recycles them. A layer is a system layer on screen; the shell creates it through its UI framework, places it and knows its display. The one handoff: the shell's layer shows a frame, the picture and the positions of the program's surfaces together, from any thread, and calls back once the compositor releases the picture; it also tells the engine when its display refreshes, and opens child layers for the program's surfaces. This is Inset's surface contract, which the experiment measured.
4. **Views are given by the shell or requested by the program, and one that arrives carries its request's id,** as Inset's views are at every layer: Inset's `Platform::views`, `request_view` and `View::request_id` ↔ `wasdk:platform`'s `views`, `request-view` and `view.request-id` ↔ the views the shell passes with `run` or `add_view`, and `ViewPlacement::view_requested`. A desktop's shell gives none at start and makes windows on request; a phone's gives each program its tab's view and refuses requests; an app bundling one program gives the view it has. `inset-embedder-wasdk` maps one to one.
5. *(Replaced by 19.)* **The engine is two crates:** `waos-session`, each program's state (views and their events, frame pacing and the buffer gate, lifecycle, the quit protocol, grants turned into its environment), which names neither wasmtime nor the GPU; and `waos`, with wasmtime, workers, freezing, WebGPU, shared pictures, the bindings and the public API. The compiler keeps the state logic free of the engine, and its tests run without wasmtime.
6. **The shell is small traits, one per job:** `ViewPlacement`, `Prompts`, `FileDialogs`, `MenuBars`, each replaceable or wrappable on its own, with defaults where one makes sense (the system's file dialogs; prompts that refuse everything, for a headless runner).
7. **Programs write against the generated bindings, passed through.** The SDK adds what generation cannot produce: `#[wasdk::main]`, `#[wasdk::worker]`, and a few builders where Rust has a strong idiom, such as `Command` mirroring `std::process::Command`. The SDK pins one wit-bindgen per release.

**Decisions taken after the skeleton (2026-10-17).** Building step 1 showed that `capabilities/local` could not assemble a `ProgramEnvironment` living in the engine without depending on it. The lead took these:
8. **`ProgramEnvironment` and the trait for a program's saved permissions live in `waos-capabilities`,** beside the traits they hold. `capabilities/local` depends on `waos-capabilities` and the `local-*` crates it assembles, and answers a `ProgramEnvironment`; the engine takes one; `waos-store` implements the settings trait without depending on the engine.
9. **The app's package is `waos-app`, and its executable `waos`,** since the engine crate is `waos`.
10. **The toolchain is pinned to `nightly-2026-09-02`,** Inset's, so builds are reproducible.
11. **The license is MIT.**

**Decisions taken on the core interface (2026-10-17).** The experiment's `display` interface was one root holding views, the user's settings and the OS, named in X11's and GTK's sense of "the connection to the workstation". The lead found it awkward: a system has several monitors, each with its own size, scale and refresh rate, and views do not belong to a display. The lead took these:
12. **A program's views, the screens and the system are three interfaces of plain functions:** `views` (`views()`, `view-changes()`, `request-view()`), `screens` (each monitor, its size, scale and refresh rate, and `view.screen()` for the one a view is on) and `system` (the OS, and what its user set: light or dark, languages), as Flutter, Apple, Android and the web keep them apart. A monitor is a **screen**, as Apple names it. *(This record also said "a getter answering an option stays only where absence is real: the capabilities". That was the proposer's line, not the lead's: the lead approved the split and the name. Withdrawn on 2026-10-20; how a program reaches a capability is decided with each one.)*
13. **A window is reached from its view (`view.window()`), and asked for through `request-view`;** `windows` has no root. A window has no `view()`, since WIT forbids two interfaces using each other and the program holds the view it came from; a view has no `close()`, since closing is the window's.

14. **`sdk/wasdk-wgpu` is a wgpu backend over `wasi:webgpu`,** through wgpu's own extension point for another WebGPU implementation, so programs, valo and Inset use wgpu unchanged inside a component. What wgpu has and WebGPU lacks, such as a surface from a window handle, is refused with a message naming the gap. Ported from the experiment in step 4, when the first program draws.

**Decisions taken on step 2's bindings (2026-10-17).** The generated bindings showed a window's configuration as a record of 11 required fields, a type that could not share its name with its function, and `lifecycle` still behind a root getter. The lead took these:
15. **A window's configuration is a resource with setters** (`window-config`: a constructor giving the platform's ordinary window, and a setter per option), so a program writes two lines for a plain window, and an option added later breaks no program; `request-view` takes the configuration and the initial data.
16. **Where a plain function and its type would share a name, the type keeps the plain noun:** `platform-brightness()` answers a `brightness`, as Flutter names them.
17. **`lifecycle` is plain functions:** `state()`, `state-changes()` and `exit-requests()`. A stream of changes is named `<thing>-changes`, as `view-changes` and `screen-changes` are.

18. **Fonts keep the experiment's shape: the engine rasterizes glyphs**, and a program shapes from each face's tables (wasdk's `wit/fonts.wit`, brought to this project's conventions and shown before step 7 builds it). Giving programs whole font files was measured: it costs a program nothing only by mapping the file over a buffer wasmtime lent the engine, which wasmtime does not promise, and copied it costs the file's size per program, 53 MiB a program for a CJK face. A ranged read of the file gives the program no more than the experiment's tables and outlines did, and rasterizing in the program measured 2.7 to 3.2 times slower than the engine.

19. **The engine is one crate, `waos`, organized by interface** (replaces decision 5). Each interface's module holds its state, plain Rust tested without an engine, beside the code that serves it, as `wasmtime-wasi` keeps each capability's state beside its host code; the runtime, the GPU side, the shell's traits and the public API are modules of their own. `waos-session` was the experiment's `wasdk-gui`, a crate of its own because native builds shared it; without them it had one user and split every interface across two crates.
20. **Host code uses the types wasmtime generates from the WIT directly.** The experiment kept a second set of records only because native builds had no wasmtime; every host here runs the engine. Each interface is generated once, in the lowest crate that needs its types: the core in `waos`, and a capability's beside its provider trait, settled with the first capability in step 7.

21. **Programs are built, run and packaged with `cargo waos`** (`crates/waos-cli`, binary `cargo-waos`), not an `xtask`: the commands are the ones a program's author needs, so they are written once in the shape authors use. Like `cargo inset`, its metadata is in the program's `Cargo.toml` (`[package.metadata.waos]`), but it is named after the platform its commands serve: running in WaOS, packaging for WaOS's sources. It gains only what the work order needs: `build` and `run` at step 5, `package` at step 6. It has no `doctor` and no device commands: a phone runs programs in the WaOS app already on it, which `cargo waos run` will find on the network, as Expo Go loads a project. The WaOS app itself is an Inset app, built and bundled with `cargo inset`.

22. **The shell's traits are the WIT's resources minus what the engine keeps** (`ShellView`, `ShellTextInput`, `ShellWindow`), since every platform has one view object that shows pictures, paces frames and owns its cursor, focus and text input, with the window apart. The engine keeps everything it can, a request's initial data among it, and asks the shell only for what needs the platform.
23. **A view arrives with a starting value for everything the engine copies** (`PlacedView`: metrics, screen, presence, its window's frame), and every change after comes through its `ProgramView`, as Flutter's embedder adds a view with its metrics and sends each change after. Every request between the two sides has exactly one answer.
24. **What happens once is a future**: a program's end (`ended`), an exit request's answer, the app suspending; what happens many times is a report or a stream; what the engine needs done is a shell trait. `ended` is a `JoinHandle`, as Dart's `Process.exitCode` is a future.
25. **The engine owns freezing**, by the experiment's rule configured with `FreezePolicy`; the shell reports only what only it knows, the platform's app-level background and foreground (`app_suspending`, awaited inside the platform's grace period, and `app_resumed`). A frozen program loses nothing: its GPU device and objects stay, and the thaw continues where it stopped.
26. **The machine's facts are set with their whole current value** (`set_screens`, `set_platform_brightness`, `set_locales`, starting from `PlatformState`), and the engine works out what changed, as Flutter's embedder pushes its displays and locales whole. Per-program language, appearance and a permission for the full screen list are left for step 7's permissions.
27. **The engine's type is `WaOS`**, the product's name, with `WaOSConfig`.

28. **Step 5 is two steps, and fonts come forward to the second.** 5a is the desktop shell and `cargo waos`, with the triangle and survey on screen; 5b is fonts and `inset-embedder-wasdk`, with the gallery on screen. An Inset program needs fonts to show anything, and Inset programs will be most programs, so fonts are core, not one of step 7's optional capabilities.
29. **`cargo waos run` sends the program to the WaOS that runs** (starting the installed WaOS.app if none answers), as Expo Go loads a project, rather than starting a WaOS of its own: one way programs reach WaOS, on a desktop now and on a phone later over the network. The connection is the run, as `cargo run`'s process is: the program's output streams back, and closing the connection, by Ctrl-C or the terminal closing, stops it; a later run of the same name replaces it. The messages are `crates/waos-dev-protocol`'s, in length-prefixed postcard frames.
30. **On this machine the connection is a Unix socket in the user's folder,** so the system's file permissions decide who may connect; the network transport, with finding WaOS on the network and pairing, arrives with phones and carries the same messages. Windows uses a named pipe (step 11).
31. **Whoever runs a program chooses where its output goes**, as `Command::stdout` does for a process: `ProgramEnvironment::output`, an `Arc<dyn ProgramOutput>` that takes the bytes the program writes to stdout or stderr as it writes them; with none, the engine writes them to its log. A run from `cargo waos` writes to its connection; the app's log window, later, is another. The engine's log stays for diagnosing the engine.
32. **A program's start is an app's launch, and that comes with apps of their own (step 10).** On macOS a program's own app is started through Launch Services, which activates it as any app's launch does: its first window comes forward, and windows it opens later come up without taking the focus from the app the user is in, unless the program asks with `view.request-focus-change`. Until then nothing is built for it: a program's window opens behind the app the user is in, since winit activates an app only at its own launch and when a window is asked to take the focus, and WaOS is already running when a program's window opens.
33. **Fonts are a capability whose provider is valo's `FontManager`:** `ProgramEnvironment::fonts: Option<Box<dyn FontManager>>`, which `capabilities/local` fills with `valo_fontmgr::platform()`; `waos-capabilities` depends on `valo-fontmgr` for it, and `capabilities/local-fonts` leaves the layout, since it would only forward. The engine rasterizes. A typeface's bytes are the one mapping of its file in the engine's process, shared by every program; a program receives only a face's tables and its glyphs' pixels, so decision 18's memory reason stands.
34. **`fonts` is plain functions** (`families`, `family`, `match-family-style`, `match-character`, `system-font`, `changes`, and the glyph calls): a program given no fonts gets empty answers, as on a machine with none installed. The lead was content with this or a root getter answering an option, and this is the simpler to build.
35. **Step 5b, as the lead took it.** The WIT `fonts` is the experiment's, as plain functions, without `face-count`, outlines as a typed `list<path-element>`. One valo `FontManager` serves every program behind a lock (`ProgramEnvironment::fonts: Option<Arc<Mutex<dyn FontManager>>>`, refining decision 33's `Box`), since it caches its lookups; the engine numbers each font file it hands out, holding its mapping, serves a face's tables without its outlines, and rasterizes with valo's `Rasterizer`. valo gains `Font::remote_id`, Skia's `SkTypefaceProxy::remoteTypefaceID`: the id of a face in the process that renders its glyphs, which `inset-embedder-wasdk` sets on each font it builds from a face's tables and its glyph raster reads back, in place of the experiment's map by the bytes' address. Inset's `HostWindow::is_minimized` and `is_occluded` answer false in a program until the WIT's `window` carries them, as on Wayland.
36. **A program's fonts come from the embedder's own `FontSource`, not valo's `SystemFonts`.** `SystemFonts` adapts a platform's `FontManager`; in a program the engine already answers its questions (`family`, `match-character` with the user's languages, `system-font`), so `inset-embedder-wasdk` implements valo's `FontSource` over the `fonts` functions: it rebuilds a face from its tables with valo's `assemble`, makes its `Font`s with `remote_id` set, and picks a variable face's instance nearest the style asked. That picking is extracted from `SystemFonts` into `valo-text`, after Skia's `SkFontStyleSet::matchStyle`, and both call it. valo's `Typeface` is unchanged. Reusing `SystemFonts` would have meant threading the id through a layer that does not fit a program, to keep six lines of logic.
37. **The engine names each typeface, a face in a file, with one id, and a glyph call names a font: a typeface's id and a point on its axes.** The WIT's `typeface` answers `id()` in place of the experiment's `file()` and `index()` (the engine maps an id to its file's mapping and the face's index; the program, which rebuilds a single face from its tables, needs neither), and the glyph calls take `record font { typeface: u64, variation-coordinates: list<tuple<tag, f32>> }` (one point on the typeface's axes, a value per axis, by valo's name for it), renamed from the experiment's `face`, which held a font's worth; empty coordinates are a face that is not variable, or a variable face's default, as valo's `variation_coordinates` reads it. valo's `Font::remote_id` holds its typeface's id, which valo's own `uid` cannot: one variable typeface gives several fonts, each with its own uid for valo's glyph cache. `match_style` is extracted into `valo-text` with CSS's precedence (width, style, weight), after Skia's `SkFontStyleSet::matchStyle`, replacing `SystemFonts`'s narrower copy.
38. **A program's views on one screen are served by one refresh, and each view's frame names them** (step 5c): the WIT's `frame` gains `views: list<view-id>`, the program's views that refresh serves, and each of them still gets the frame on its own stream, after every event before it. The program learns the group exactly while a view's input still comes before its frame, which a refresh on a stream of the program's own would lose. Inset takes a frame as a screen's refresh of the views on it (`2026-10-20-inset-frames-per-view.md`, decision 13), and the embedder calls it once per group.
39. **The engine groups a refresh's views; the shell only reports a screen's ticks**, as Android's `Choreographer` serves every window of the thread at one vsync: the shell's `VsyncWaiter` calls `WaOS::screen_refreshed(screen, at)` at a screen's next refresh, and the engine, which knows every program's views, their screens, whether they are in sight and what they asked for, serves the due ones. `ShellView::next_refresh` goes.
40. **Inset gains `Platform::vsync_waiter()`, a screen's next refresh as a capability of its own** (Flutter's `VsyncWaiter`, for any screen), which the WaOS app's shell implements the engine's `VsyncWaiter` over; neither `Compositor`, whose job is native surfaces, nor `Screens`, which a program's own host implements without ticks to give. Inset's winit host hands out the one source per screen its own frames already use, `NSScreen`'s display link on macOS; `Surface::next_vsync`, whose users, WaOS's shell and Inset's `native_surface` example, move to the waiter, goes with the per-window vsyncs behind it.
41. **A view's frame never waits for a picture**, as Flutter's `FlutterSurfaceManager` and Firefox's `SurfacePoolCA` never wait for a surface: when a frame is due and no picture is known free, the engine makes another, and spares go back after the view has drawn nothing for a while. The pool's cap and the frame gate go: macOS reports no release, so a frame held for one waited on someone asking `IOSurfaceIsInUse`, and with every picture held no present came to ask, which the per-window tick's check alone prevented. Inset's `present` keeps its `released` callback, the shape for compositors that report releases, which WaOS uses to know which pictures it can reuse; Inset's macOS host works it out at each present, and its per-window ticks go.
42. **A view has at most two frames in flight**, from the program's present until the frame is handed to the shell once its GPU work is done: at a refresh, a view with two is left out and tries again at the next, as Flutter's `Animator` skips a vsync while its `FramePipeline(2)` is full. A picture must not be shown before the GPU has drawn it, so every frame waits for its GPU work anyway; this counts the frames still waiting, which bounds the pictures and the lag a GPU slower than the screen would otherwise pile up, now that no frame waits for a picture (decision 41). Only the full view leaves the refresh; the others on its screen are served.
43. **A package proves its publisher with the publisher's own key**, as Android's apps do: an Ed25519 key the publisher makes once, a program's key remembered at its first install, and an update only with the same key; a lost key means a new id. GitHub's artifact attestations were weighed and left: they spare the publisher a key, but tie identity to GitHub and its Sigstore, need WaOS to verify certificate chains, transparency logs and a root of trust that changes, and leave a package built off GitHub or on the author's machine with nothing.
44. **The signature is inside the package**: `publisher.key`, the public key, and `signature`, over the SHA-256 of `manifest.toml`, `program.wasm` and `icon.png` by name, as Android's first scheme and Java's signed JARs carry theirs, so a package proves itself wherever it comes from, and packing its entries again does not break it.
45. **Every source is an `index.json`** of each program's id, name, version, package and SHA-256: anywhere, at its own URL, and on GitHub as an asset of the latest release (`…/releases/latest/download/index.json`), which needs no GitHub API. The index is not signed, since each package is, and a version not newer than the installed one is refused.
46. **An update is any source's listing of an installed id at a newer version signed with the same key**, so an installed program is its manifest and its publisher's key, with no record of where it came from. Automatic updates wait for their UX, as do per-program settings, which have nothing to set before step 7's permissions.
47. **`cargo waos` keeps the publisher's private key, the signing key, in a file in the user's config folder** (`signing.key`), made on first use and read from `WAOS_SIGNING_KEY` on CI, as Android's debug keystore and `minisign` keep theirs: one way on every machine, which the user can back up. The public key is the publisher's key, the package's `publisher.key`; "signing key" is Android's word for the private one, so the two never share a name.
48. **Step 6 builds a plain library**, as the experiment's WaOS did: state and logic shared, Fluent controls (`inset-winui`) at desktop and tablet sizes, in a panel above WaOS's Dock icon, and Cupertino at phone size; installed programs to open and remove, the sources to add and remove with each one's programs to install or update, and a `.wapk` opened with WaOS installed.
49. **The engine's time is an input, as Inset's `App` clock is** (2026-10-27): what freezes a program, when a view was hidden or shown, and when spare pictures go, read one engine clock, and whatever moves it runs the tick (freezes that came due, then the epoch). `WaOSConfig::time` is `EngineTime::Real`, the engine's own thread every 100 ms, or `EngineTime::Manual(ManualTime)`, which a test moves with `ManualTime::elapse`, as Inset's tests call `AppCell::elapse` and Flutter's `FakeAsync.elapse`; and a test proves nothing happened by waiting until the program is outside its code, which the engine already tracks, rather than by watching. A program's own clock and sleeps stay on real time. Tests that wait on real time are slow and prove absence only by waiting.
50. **A font a program draws with carries valo's synthesis** (2026-10-28): `fonts.wit`'s `font` gains `embolden`, how much fake bold thickens the outline as a fraction of the size, and `skew`, fake italic's horizontal shear, each 0 for none, as Skia's `SkFont` carries `embolden` and `skewX` and valo 0.13's `FontSynthesis` takes them. Inset in the program decides them, as it does natively, and the engine passes them to valo, so a program's text in a family with no bold or italic face looks as a native app's does.
51. **Apps of their own on macOS and Android come next, side by side, and workers wait** (2026-10-29): together they make the story of the first version, a program an AI agent built, used everywhere as an app of its own, and they bring real use soonest; workers (step 8) follow them. On Android a program has an identity of its own as on macOS, its shape decided after research into a task per program in WaOS's one app and a generated app per program, as Chrome's WebAPKs are; on macOS after research into Chrome's app shims, the closest precedent for a process per program showing what another process runs. *This reorders the work: steps 9 and 10 before step 8.*
52. **A program draws on a view through wgpu's own surface** (2026-11-05): `wasdk-wgpu` answers `Instance::create_surface` for a view, its window handle a `WebCanvasWindowHandle` pointing at the program's `View`, as a browser's points at a canvas (a view's id is a `u64`, which the numbered `WebWindowHandle` cannot carry); the surface configures the view's canvas context, hands out its texture, and `Queue::present` presents the view. `configure_canvas`, `wrap_texture` and `preferred_canvas_format` go, so there is one way to draw on a view, and a framework's renderer runs as it does on any system, as the research on Bevy and Rust UI frameworks (`2026-11-05-bevy-on-waos-research.md`, `2026-11-05-rust-ui-frameworks-on-waos-research.md`) found every wgpu framework's does. The texture is the view's size, whatever `configure` names, as it is today. This amends decision 14, which refused a surface from a window handle. Not a render into a texture handed to each framework, which every port would pay for, nor waiting on a surface target of its own in wgpu upstream.
53. **A view's canvas is the size the program sets, and a view's new size shows only with the program's first picture at it** (2026-11-05): `view.set-canvas-size(width, height)`, which `wasdk-wgpu`'s `Surface::configure` calls with the size configured, so the size wgpu reports for a frame is its texture's; the shell never stretches a picture to a size it was not drawn at. On a resize the shell holds the platform's change until the program presents a picture of the new size, through the platform's own wait where it has one: on macOS the fence the program's app commits the new size under, kept until that picture rather than until WaOS laid its layers out; on Wayland the configure acknowledged with that commit; on Android the system's draw sync. WaOS sets no deadline of its own, so a program that does not draw is not resized, as a hung Mac app is not; nothing waits before a view's first picture; where the platform resizes at once, as Windows does, the last picture shows unstretched on the window's background. A window gains the rules the system applies while the user drags, each optional where a platform has none: a minimum and a maximum size, an aspect ratio and resize increments, as macOS's `contentAspectRatio` and `resizeIncrements` and X11's size hints. From the research on resizes (`2026-11-05-resize-research.md`). This amends decision 52, whose texture followed the view's size. Not a deadline, as Flutter's second on macOS or Chrome's none, which show a stretched or stale picture; nor a canvas that follows the view, which hands a framework a frame of another size than the one it configured.
54. **On macOS, a program's app holds back its window's new size itself until the program's picture at that size, as an ordinary Mac app's window does** (2026-11-05): during a resize the app keeps the window at its size, tells WaOS the size asked for, and applies it with the program's first picture at it, a newer size waiting behind the pending one and only the latest kept, as Chrome's app shims do (`components/remote_cocoa/app_shim/native_widget_ns_window_bridge.mm`, `kAsyncLiveResize`: the window's frame kept pending until a frame of its size arrives). Only that window waits, with no deadline: a stuck program's window does not follow the drag, as a stuck Mac app's does not, and while the user drags, the window trails the pointer by the program's time to draw. The fence the app commits a new size under covers WaOS's layout at that size only, as before. This amends decision 53's macOS mechanism, its rule kept. Not a fence held until the program's picture: measured, the window server lets go of a fence 500 ms after the commit carrying it, and while one is held, the windows of every app on the Mac wait.
55. **A program's app waits in the resize itself until WaOS says the program's picture at the new size is shown** (2026-11-05): winit hands the app each step of a resize inside AppKit's step, before macOS shows the new size, so the app sends the size and waits there, with no timeout, and the window and the picture show together; Flutter's macOS embedder waits the same way (`ResizeSynchronizer.swift`, called from `FlutterView`'s `setFrameSize:`). While it waits, the program's other windows and its menus wait too, as a busy Mac app's do. Nothing waits before a view's first picture, and the wait ends when the program or its connection to WaOS does. This amends decision 54's way of holding the size, its rule kept. Not Chrome's frame held pending while the app runs on, which needs a hold on the window's resize that winit does not give.
56. **A window's size rules are its limits for now, an optional part of the window: `window.size-limits()`, none where users do not resize windows, with `set-min-size` and `set-max-size`** (2026-11-05), the content's smallest and largest size, which fits most programs, and which winit carries on macOS, Windows, X11 and Wayland, as Flutter's `WindowController` takes its `constraints`; Inset's `HostWindow::size_limits` the same. A ratio and size steps wait for Inset's own macOS backend: AppKit keeps one of the two at a time, setting one clearing the other, and winit sets the steps at the start of every live resize, which clears a ratio set on the window. Inset's iOS host has no windows, so iPadOS has no limits until it does. This amends decision 53's window rules. Not a patch of winit, which Inset is to replace.
57. **WaOS's crates for programs, for `cargo waos` and for embedding WaOS are published on crates.io** (2026-11-08): the SDK (`wasdk-macros`, `wasdk`, `wasdk-wgpu`, `waft-embedder-wasdk`), the command (`waos-package`, `waos-dev-protocol`, named `waos-dev` until 2026-11-09, as the protocol is what it holds, and `waos-cli`) and the engine (`waos`, with `waos-capabilities`, the traits an embedder implements, and `waos-webgpu-host`), ten crates, while the repository stays private (amended 2026-11-17: the repository is public, at github.com/waos-dev/waos); the `capabilities/local*` crates, the WaOS app's and the engine's tests', stay unpublished (amended 2026-11-08: not the capability crates the engine does not depend on). A crate that generates bindings carries its own copy of the WIT, as wasmtime's crates do theirs (wasmtime's `ci/vendor-wit.sh`): `wit/` stays the one edited, a script copies it into each crate, and a test fails where a copy has drifted, so the repository builds on Windows, where Git checks a symbolic link out as a file. The patched wasi-gfx WebGPU host is published as a crate of ours, `waos-webgpu-host`, its Apache-2.0 licence and notice kept, since crates.io takes no Git dependency and no `[patch]`, and the patches are offered upstream. Not the WIT in a crate of its own, whose bindings each consumer would generate in a build script through wasmtime's internal code generator; nor a symbolic link; nor a WebGPU host written again, the 7,000 lines already ours in all but name.
58. **On macOS, a program's trap is caught by a signal handler on the program's own thread, as on every other Unix, not by a Mach exception port** (2026-11-09): `Config::macos_use_mach_ports(false)` for every engine in the process. wasmtime's Mach exception thread reads a trapping thread's registers with `thread_get_state`, which answers `KERN_ABORTED` when a caught signal lands on that thread, a child's `SIGCHLD` among them, and wasmtime then lets the trap end the whole process (measured: 18 and 31 of 50 runs under a flood of signals, on 50.0.1 and 51.0.0-rc.1 alike). V8, Wasmer, WasmEdge and WAMR catch traps with signal handlers on the faulting thread, and JavaScriptCore receives the registers in the exception message; SpiderMonkey and .NET read them as wasmtime does. Mach ports exist so a process's crash reporter (Breakpad, Crashpad) can share faults with the engine: WaOS has none, and one added later would see each program's trap first. lldb stops at each trap unless told to pass `EXC_BAD_ACCESS` and `EXC_BAD_INSTRUCTION` on. The experiment measured traps about 6 times faster this way.
59. **`cargo waos new` writes a WinUI app by default** (2026-11-09), since WinUI's kit has more controls than Cupertino's and its app works on a phone too; `--template cupertino` is for an app meant for phones alone. Not Cupertino by default, as `new` first wrote.
60. **An agent reads and drives a program through one semantics tree, Flutter's, the one a screen reader reads, and later through typed functions a program declares** (2026-11-11), as every platform an agent controls does: Android's `AccessibilityNodeInfo` and AppFunctions, Apple's AX and App Intents, Windows' UI Automation and App Actions, the web's ARIA and WebMCP. `cargo waos` carries it, not an MCP server: `screenshot` and input at a point first, which need nothing of the program; `tree` and `act <ref>` once Waft has semantics, a port of its own after Android; a program's own run under `cargo waos run` always, an installed one later with the user's consent per agent. Not a debug-only inspector of Waft's element tree.
61. **A window moves from a press, is maximized and minimized, and on macOS hides its title bar, as Waft 0.3's `HostWindow` asks** (2026-11-11): `window.drag`, `is-maximized`, `set-maximized`, `set-minimized`, `title-bar-double-click-action` (none where the system has no such setting) and `window-config.set-macos-title-bar(standard | hidden(height))`, macOS's alone as in Waft, Electron and Tauri; the program's window process carries them out through `waft-embedder-winit`. WaOS's crates then move to Waft 0.5 together, released as 0.2.0.
62. **A screenshot is the program's picture and every composition layer's, each asked for what it shows now as a hardware buffer, drawn together in placement order on the GPU** (2026-11-11), the same on every platform: a layer the program presents to holds its last buffer; a system's video hands its current frame through a video output beside the layer (Apple's `AVPlayerItemVideoOutput`); a camera its last frame. Waft's `CompositionLayer` gains `current_picture() -> Option<LayerPicture>`, `Buffer` (the raw buffer presented last, filling the layer) or `Frame` (the lent frame of the video or camera shown, whole at its shape, held while kept, since a pool hands its surface out again once nothing holds it), decision 35's raw and lent as two cases (amended 2026-11-11), which WaOS's `ShellCompositionLayer` forwards; programs never see it. Not Core Animation rendering WaOS's layer tree offscreen, which is Apple's alone.
63. **While `cargo waos run` is attached, the run's windows count as in sight though other windows cover them** (2026-11-11), so a tool's screenshot and input meet a drawing window, as Chrome DevTools' "Emulate a focused page" keeps a page focused; a minimized window is still out of sight. One more input to the presence WaOS already decides from the shell's reports, with no frame forced and no flicker of presence.
64. **`cargo waos`'s input goes through the platform's own input where the shell can make it, and straight into the engine only where it cannot** (2026-11-11): on macOS the program's window process makes the events AppKit would deliver, so a key the program leaves goes on to the window's menu bar, the program's menus and WaOS's defaults, as a real key does, and nothing happens where nothing takes it; a shell without that part, the test shell among them, has the engine take the input as the shell would hand it over. Keys (amended 2026-11-11): a program's app in the background is offered a ⌘ chord by the menu bar before the program and loses its key-up (measured), so Waft's macOS host hands a key event to a window as its key window would receive it, the framework first and then the menu bar; where that meets a blocker, keys go to the engine as the shell would hand them over. For now (amended again 2026-11-11) every input is made by WaOS and handed to the engine as the shell would hand over the user's, on every platform, and `cargo waos` says the events are WaOS's: the program hears them as the user's, the system and the window's menu bar do not, so a key the program leaves reaches no menu item, a known gap left for later, being rare.
65. **A view gets one frame for each refresh of its screen** (2026-11-12): when the program asks for a frame and the view has not had the frame of the refresh its screen is in, it gets that frame at once; otherwise it gets the next refresh's. A refresh is an interval that is claimed once, not an instant, as an observer added to Chromium's `BeginFrameSource` between two ticks is sent the tick it missed (`ExternalBeginFrameSource::AddObserver`, with `CheckBeginFrameContinuity`: a frame's refresh must have begun after the last one the view had). So which of a refresh's touch and its frame reaches the program first no longer matters, a frame shows its own refresh's touch as a native app's does, and no program or framework has to know. Measured before it on a Pixel 2 XL: a drag in a Waft program ran at 30 fps whenever a refresh's touch reached the program just before that refresh's frame did, since the touch found the frame pending and asked for none, and the next refresh's touch, with nothing pending, asked just after its refresh had come and waited for the one after. The two-frames-in-flight rule stays, and so does the WIT but for its words on `request-frame`. The engine knows the refresh a screen is in as Chromium's `DelayBasedBeginFrameSource::AddObserver` knows the tick it missed: the last refresh the shell told of, moved on by the whole intervals passed since, the interval being the screen's refresh rate; where the system says no rate, or no refresh was told yet, a frame waits for the next refresh as before. This replaces, for a program's views, the line of `2026-10-20-inset-frames-per-view.md` that fixes a refresh's views when it starts.
66. **On macOS, WaOS asks not to be throttled while a program compiles or shows a window** (2026-11-17): a program's windows belong to processes of their own, so macOS takes WaOS for a background app and App Nap runs all its threads at background priority, which made a 5-second compile take over 9 minutes (measured). WaOS holds one `NSProcessInfo` activity, user-initiated with idle sleep allowed, while any program's code compiles or any program's window is in sight, and lets it go when none is, as Chrome holds one in a renderer while its tab is in front. Waft gives the call on its macOS platform type only (`MacOs::begin_user_activity`), reached by downcast, nothing on the shared `Platform`. Not App Nap turned off for WaOS's whole life, which would keep it awake with nothing to do.
67. **A release is one tag, and CI checks and publishes it; the interface's version changes with every change a program built before it can't run against** (2026-11-17): the app trials built from crates.io's `wasdk` 0.2.0 and the development WaOS refused them, its `wasdk:platform` changed since under the same `@0.1.0`, while `tests/new.rs` builds the template against the workspace only, so nothing saw it. On every change, CI fails when a WIT package differs from the last release's without a new version. On a pushed tag, CI runs the workspace's tests, publishes the SDK, the command and the engine at the tag's version together (decision 57), then writes a program with the published `cargo waos new`, and builds it from crates.io alone, so a release whose template does not build fails where its author sees it; the SDK matches its WaOS because both come from the one tag. WaOS's builds, signing and its site's downloads join the same flow when they are released.
68. **`cargo waos run --env NAME=VALUE` gives a program variables for that run alone** (2026-11-17), as Docker's `-e` gives a container's: a developer gives what one development run reads, `--env RUST_LOG=debug` being the usual, and nothing of the shell's own environment is forwarded, so no secret or fact of the machine reaches a program unless it was named. The variables travel in `ToWaOS::Run`, whose message rises to protocol 8, and become the run's `ProgramEnvironment::variables`, which the engine sets in the program's WASI environment; an installed program opened from the library gets none of them. `TMPDIR`, which WaOS sets to the program's own `/tmp`, wins over a variable of that name, and a variable is `NAME=VALUE`, its name before the first `=`, refused otherwise. Not the developer's whole environment passed through, which hands every run everything the shell holds; nor variables on a program's settings, which would outlive the run and reach the program when a person opens it.

Standing rules the lead set along the way: every interface, and every capability's details, is shown to the lead as WIT and Rust types with its mental model before it is written, since `2026-10-17-waos-v1-interface.md` holds only rough designs; and anything that turns out not to fit a platform naturally, or that needs forwarding layers where the thing should pass through, is raised with options, not built quietly.

Settled along the way, as the proposer's choices the lead accepted:
- `waos-store` is a crate of its own: installed programs, updates, package sources, each program's settings. An app bundling one program builds none of it.
- The engine reads and writes a program's saved permissions through a small trait in its environment, which `waos-store` implements; to be flagged if it turns out to be a forwarding layer. It lives in `waos-capabilities` (decision 8).
- Threads as the experiment proved them: a thread per program instance, workers included, each on a single-threaded tokio runtime; a thread per view that finishes its frames; one epoch-tick thread. Shell traits may be called from any thread.
- `inset-embedder-wasdk` lives in `sdk/` until its API settles; programs are built, run and packaged with `cargo waos` (decision 21).

The decisions of the experiment's last proposals carry over as the plan, copied here: `2026-10-16-waos-v1.md` (what the first version does) and `2026-10-17-waos-v1-interface.md` (what it gives a program), with this project's changes: no native backend, and remote machines last.

## The parts

```text
program (wasm) ── wasdk:platform ──▶ engine ──calls──▶ capabilities (services, per program)
                                       │
                                       └──asks, places──▶ shell (the embedding app's UI)
```

```text
wit/              wasdk:platform
crates/
  waos/                 the engine, by interface, each with its state; the runtime, the
                        GPU side, the shell's traits and the public API
  waos-capabilities/    the capability traits and their records, in WIT's shape, and
                        ProgramEnvironment, which holds one program's providers
  waos-package/         the .wapk format: manifest, read, write, sign, verify
  waos-cli/             cargo waos: build, run and package a program (decision 21)
  waos-dev-protocol/    what cargo waos and a WaOS say to each other (decisions 29, 30)
  waos-store/           installed programs, updates, package sources, settings
capabilities/
  local/                this computer's defaults, assembled
  local-fs/ local-network/ local-processes/ local-clipboard/ local-audio/
  local-cameras/ local-media/ local-fonts/ local-notifications/ local-opening/
sdk/
  wasdk/                the generated bindings, passed through, and builders
  wasdk-macros/         #[wasdk::main], #[wasdk::worker]
  wasdk-wgpu/           a wgpu backend over wasi:webgpu (decision 14)
  inset-embedder-wasdk/ Inset over wasdk
apps/
  waos/                 the app: the desktop and phone shells in Inset, an entry per OS
examples/               programs
```

Dependencies run one way: a capability crate depends on `waos-capabilities` only, and `capabilities/local` also on the `local-*` crates it assembles; `waos` on `waos-capabilities`; `waos-store` on `waos-package` and `waos-capabilities`; `waos-cli` on `waos-package` and `waos-dev-protocol`; the app on `waos`, the capabilities it picks, `waos-store`, `waos-dev-protocol`, and Inset. Programs depend on `sdk/` only.

## The engine's side and the shell's

As decided for step 4 (decisions 22 to 27). The shell passes on what the platform reports, as the platform reports it; the engine keeps every copy and every rule, and asks the shell only for what needs the platform.

```text
shell (the WaOS app)                    engine (waos)                               program
  reports: presence, metrics,  ───▶  keeps copies, queues events,   ◀── wasdk:platform ──▶  views, lifecycle,
  input, app suspending                derives lifecycle, decides frames                        WebGPU
  implements: ViewPlacement,   ◀───  asks only what needs the platform
  ShellView / TextInput / Window       one thread per program, one store, epoch tick
  awaits: ended(), request_exit()      pictures: IOSurface pool per view
```

### What an embedding app sees

```rust
pub struct WaOSConfig {
    /// Where compiled programs are kept, so the next start maps the code instead of compiling it.
    pub code_cache: PathBuf,
    pub freeze: FreezePolicy,
}

pub struct FreezePolicy {
    /// How long a program stays hidden before the engine freezes it on its own;
    /// none: only when the app is suspending. 5 s by default.
    pub after_hidden: Option<Duration>,
}

/// What the platform reports about the machine when the engine starts; the setters keep it current.
pub struct PlatformState {
    pub screens: Vec<Screen>,
    pub platform_brightness: Brightness,
    pub locales: Vec<Locale>,
}

impl WaOS {
    pub fn new(config: WaOSConfig, shell: Shell, platform: PlatformState) -> Result<WaOS>;
    /// The platform's screens now, the whole list, each time it changes; programs hear which
    /// were added, removed or changed.
    pub fn set_screens(&self, screens: Vec<Screen>);
    pub fn set_platform_brightness(&self, brightness: Brightness);
    pub fn set_locales(&self, locales: Vec<Locale>);
    /// Compiles once, then maps from the cache; blocks while compiling, so call it off the main thread.
    pub fn load(&self, component: &Path) -> Result<Program>;
    pub fn run(&self, program: &Program, environment: ProgramEnvironment, views: Vec<PlacedView>)
        -> Result<(RunningProgram, Vec<ProgramView>)>;
    /// The platform sent the whole app to the background (iOS's `didEnterBackground`, Android's
    /// `ProcessLifecycleOwner` `ON_STOP`): every program is `paused` and freezes now. Resolves once
    /// none runs and none has GPU work in flight.
    pub fn app_suspending(&self) -> impl Future<Output = ()> + Send;
    pub fn app_resumed(&self);
}

impl RunningProgram {
    pub fn add_view(&self, view: PlacedView) -> ProgramView;
    pub fn remove_view(&self, view: ViewId);
    pub fn view_request_failed(&self, request: RequestId, error: ViewError);
    pub fn request_exit(&self) -> impl Future<Output = AppExitResponse> + Send;
    pub fn stop(&self);
    /// Any number of times; each resolves with the same end. Dropping it stops nothing.
    pub fn ended(&self) -> impl Future<Output = ProgramEnd> + Send;
    pub fn stats(&self) -> &Stats;
}

#[derive(Clone)]
pub enum ProgramEnd {
    /// Its `main` returned: `Err` for a run that reported failure.
    Returned(Result<(), ()>),
    /// It trapped, or could not run.
    Failed(Arc<anyhow::Error>),
    /// `stop` ended it.
    Stopped,
}

impl ProgramView {
    // step 3's reports, and:
    pub fn window_frame_changed(&self, frame: Rect);
}

impl WaOS {
    /// `screen` refreshed at `at`: each program's views on it that asked for a frame, are in
    /// sight and whose picture can take one get it, the frame naming them all.
    pub fn screen_refreshed(&self, screen: ScreenId, at: Instant);
}
```

### What the shell implements

```rust
pub struct Shell {
    pub views: Arc<dyn ViewPlacement>,
    pub vsync: Arc<dyn VsyncWaiter>,
}

/// The screens' refreshes.
pub trait VsyncWaiter: Send + Sync {
    /// Calls `WaOS::screen_refreshed(screen, at)` at the screen's next refresh.
    fn next_vsync(&self, screen: ScreenId);
}

pub trait ViewPlacement: Send + Sync {
    /// Ok promises exactly one later answer: `add_view` with `request.id`, or `view_request_failed`.
    fn view_requested(&self, program: &RunningProgram, request: ViewRequest) -> Result<(), ViewError>;
}

pub struct ViewRequest {
    pub id: RequestId,
    /// What the program set on its `window-config`; the engine keeps the request's initial data.
    pub config: WindowConfig,
}

/// A view as it is when added; every change after comes through its `ProgramView`.
pub struct PlacedView {
    pub view: Arc<dyn ShellView>,
    pub text_input: Option<Arc<dyn ShellTextInput>>,
    pub window: Option<PlacedWindow>,
    pub metrics: Metrics,
    pub screen: Option<ScreenId>,
    pub presence: Presence,
    pub request: Option<RequestId>,
}

pub struct PlacedWindow {
    pub window: Arc<dyn ShellWindow>,
    pub frame: Rect,
}

/// `views.view`, minus what the engine keeps.
pub trait ShellView: Send + Sync {
    fn present(&self, picture: Picture, released: Box<dyn FnOnce() + Send + Sync>);
    fn set_cursor(&self, cursor: Cursor);
    fn request_focus_change(&self, state: ViewFocusState, direction: ViewFocusDirection);
}

/// `views.text-input`.
pub trait ShellTextInput: Send + Sync {
    fn start(&self, configuration: TextInputConfiguration);
    fn set_editing_state(&self, value: TextEditingValue);
    fn set_caret_rect(&self, rect: PixelRect);
    fn set_composing_rect(&self, rect: PixelRect);
    fn show(&self);
    fn hide(&self);
    fn stop(&self);
}

/// `windows.window`, minus its frame, which the engine keeps from the shell's reports.
pub trait ShellWindow: Send + Sync {
    fn set_frame(&self, frame: Rect, animate: Option<Duration>);
    fn set_title(&self, title: &str);
    fn show(&self);
    fn hide(&self);
    fn close(&self);
}

/// The platform's own picture type, as Inset's `Buffer` is.
#[cfg(target_vendor = "apple")]
pub type Picture = CFRetained<IOSurfaceRef>;
```

### What the shell must do

| when | exactly |
|---|---|
| it answers a view request Ok | one `add_view` with that id, or one `view_request_failed` |
| `next_vsync(screen)` | one `screen_refreshed(screen, at)` at that screen's next refresh |
| `present(picture, released)` | one `released()` once the compositor is done with the picture |
| a view changes after `add_view` | a report on its `ProgramView` |
| a view goes away | one `remove_view` |
| the user asks to close | `close_requested` only: the program decides |
| the platform backgrounds or foregrounds the app | `app_suspending()`, awaited inside the platform's grace period; `app_resumed()` |
| the screens, light or dark, or the languages change | the setter, with the whole new value |

### Inside the engine

- **Threads**: one per program, a single-threaded tokio runtime and one wasmtime store, whose data holds the program's state (step 3), its views' shell handles, its environment, its WASI context and its WebGPU objects. One epoch thread ticks every 100 ms, reading fuel and parking a frozen program.
- **Pictures**, on macOS: each view has a pool of IOSurfaces, each a wgpu texture. The view's context hands the program a free one, and makes another when none is (decision 41); `present` waits for the GPU, then calls `ShellView::present`; a released picture returns to the pool, and spares go back once the view has drawn nothing for a while.
- **WebGPU**: the experiment's vendored `wasi-gfx-runtime`, with its patches, in `vendor/`, serves `wasi:webgpu`; `sdk/wasdk-wgpu` is ported for programs. Fixed on the way: `stop` frees every GPU object the program held, which the experiment leaked.
- **Freezing**, in `lifecycle`: hidden for `after_hidden`, frozen; `app_suspending`, every program frozen at once; never frozen while it owes an answer to an exit request; thawed when one of its views shows or the app resumes; a frozen view keeps only the picture on screen.

## What a program sees of its views

```wit
interface views {
  resource view {
    id: func() -> view-id;
    request-id: func() -> option<request-id>;
    initial-data: func() -> option<string>;
    window: func() -> option<window>;
    events: func() -> option<stream<view-event>>;
  }

  variant view-change {
    added(view),
    removed(view-id),
    request-failed(tuple<request-id, view-error>),
  }

  views: func() -> list<view>;
  view-changes: func() -> stream<view-change>;
  request-view: func(request: view-request) -> result<request-id, view-error>;
}
```

## The work, in order

Each step builds and passes on its own; each interface is shown to the lead before it is written; the experiment's code is ported where it is proven, piece by piece with a review, never copied whole.

1. The skeleton: the workspace with every crate of the layout, each with its README; `AGENTS.md`; this proposal.
2. The core WIT: display and views (given and requested), the canvas, WebGPU, basic lifecycle; the generated bindings shown to the lead as a program would use them.
3. `waos`'s state, by interface, ported from the experiment's `wasdk-gui` with a review, with its tests, over the types wasmtime generates: the views (the event queue, frame pacing and the buffer gate, the views and their changes), the lifecycle (its state from the views' presence, the quit protocol), the screens and the system.
4. `waos`, in three parts:
   - 4a. Running a program: `WaOS`, `load` with the code cache, `run` on a thread of its own, `ended` and `stop`, the epoch tick and fuel; the shell's traits; the bindings for `views` (drawing refused until 4c), `screens`, `system`, `windows`, text input and `lifecycle`; a test shell, under which `examples/survey` lists its views, requests one and answers an exit request.
   - 4b. Freezing: `FreezePolicy`, `app_suspending` and `app_resumed`, and the rule; a program counting in a loop stops while frozen and goes on after the thaw.
   - 4c. Drawing: the vendored WebGPU host, the picture pool on macOS, `present` through `ShellView`, the frame gate, freeing a program's GPU objects at its end; `sdk/wasdk-wgpu`; `examples/triangle` presents frames to the test shell, read back and checked.
5. The WaOS app, in three parts:
   - 5a. The desktop shell on macOS and `cargo waos`: `crates/waos-dev-protocol` (the messages, their framing, the socket's path); `cargo waos build` and `run` with the app's listener; the shell over Inset (a native window per program view, the program's surface widget as `ShellView`, Inset's window as `ShellWindow`, input, presence and the machine's facts reported); quitting, which asks every program; the triangle and survey on screen. The app opens no window of its own until the library (step 6), and is built and signed with `cargo inset`.
   - 5b. Fonts (the experiment's interface brought to this project's conventions, shown to the lead first) and `inset-embedder-wasdk`; the gallery on screen.
   - 5c. A program's views on one screen served by one refresh, as Inset takes frames (`2026-10-20-inset-frames-per-view.md`, decisions 13 and 15): the shell's refresh, the engine and the WIT's frame, designed with the lead first.
6. `waos-package` and `waos-store`: the signed format, installing, updating and removing, settings, package sources; `cargo waos package`.
7. Capabilities, one at a time: files (with `/data`, `/cache` and `/granted`), the clipboard, networking (with `trust` and HTTP), processes, notifications, opening, dialogs, sound, cameras and media with surfaces, menus.
8. Workers, after their experiment.
9. Android, then iOS developer builds.
10. Apps of their own: the macOS window process, and Android's task per program.
11. Windows, then Linux.
12. Remote machines: the `ssh-*` capabilities.
