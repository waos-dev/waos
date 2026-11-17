# Rust UI frameworks on WaOS: research

**No decisions taken yet.** Research for the project lead, 2026-11-05, on WaOS's `main` of that day and on shallow clones of each framework's main branch: Zed `c7bea34` (GPUI, sparse), egui `fc9d66d` (0.37), iced `8ff48c4` (0.15-dev), Slint `b6b1b86` (1.19), Blitz `3f85506` with anyrender's main, Dioxus `7dfd07e`, Makepad `506ec05` (sparse), Xilem `dc49687` (0.4), and winit `7ed7c20` (0.31 beta). Nothing was built or run from them. It builds on the Zed research of 2026-10-16 (wasdk's `2026-10-16-zed-on-waos.md`; the reports behind it are in the maintainers' experiments folder). A claim is read in source unless marked *(inferred)* or *(unverified)*.

## In short

- **Closest: egui and iced.** Both render into a texture they are handed and treat the clipboard as events or tasks; each needs a new shell crate, not a fork.
- **Slint next:** a public platform API and a wgpu 30 render-to-texture, but a synchronous clipboard read and an internal-only IME and menu API.
- **GPUI is nearer than on 10-16** (async clipboard read, no blocking on wasm; WaOS gained screens, menus, dialogs, pipes). Its one blocker: `gpui_wgpu` owns a `wgpu::Surface`.
- **Blitz/Dioxus native:** a shell trait already shaped for this. **Xilem:** blocked by wgpu 28 and tokio threads. **Makepad:** furthest, no WebGPU renderer.
- **Common blockers, none in WaOS's WIT:** `wasm32` read as "the browser" (winit will not compile for `wasm32-wasip2`), wgpu surfaces (refused by [the waos project](2026-10-17-the-waos-project.md)'s decision 14), one wgpu per program.
- **Most commonly missing in WaOS:** accessibility (six of seven build a tree), threads, drag and drop, window states and moves, appearance beyond light and dark, and font outlines for frameworks that rasterize their own glyphs.

## What apps need, and what WaOS has

"How common" counts the seven frameworks that model the need, and says how many apps use it *(inferred from the frameworks' APIs)*.

| Need | How common | What WaOS has | What is missing |
|---|---|---|---|
| A window and an event loop | every app; all seven | `views.view-changes`, `views.request-view`, one `view.events` stream per view; `windows.window` | nothing in the WIT; the frameworks' loops block or are winit's (below) |
| GPU drawing | every app; six on wgpu, Makepad its own | `wasi:webgpu` with `views.view.context` (a canvas context), `wasdk_wgpu::wrap_texture` | `wgpu::Surface` (`wasdk-wgpu` `instance.rs` `create_surface` answers `wasi_gap`); WebGL; anything past WebGPU |
| Several windows | most desktop apps; all seven (GPUI, egui viewports, iced `multi_window`, Slint `create_child_window_adapter`, Blitz `windows`, Masonry `create_window`, Makepad `CxWindowPool`) | `request-view` with a `window-config`, on a desktop | parent windows and modality for dialogs; a phone refuses requests |
| Window states | most desktop apps; all seven (GPUI `minimize`/`zoom`/`toggle_fullscreen`, egui `ViewportCommand::Minimized`/`Maximized`/`Fullscreen`/`MinInnerSize`, iced `window::Action::Maximize`/`SetMode`/`SetMinSize`, Slint `Window::set_maximized`/`set_fullscreen`, Blitz `set_window_maximized`, Masonry `RenderRootSignal::Minimize`, Makepad `CxOsOp::MinimizeWindow`) | `window.frame`, `set-frame`, `show`, `hide`, `close`; `window-config` options when made | minimize, maximize, fullscreen, minimum and maximum size, the window's own state and a move event |
| A window without the system's frame | many apps with their own title bar; six (GPUI `start_window_move`, egui `StartDrag`/`BeginResize`, iced `Drag`/`DragResize`, Slint `start_window_move`, Blitz `drag_window`, Masonry `DragWindow`) | `window-config.set-decorations(false)` | starting a move or a resize from the program, so such a window cannot be moved |
| High-DPI and screen changes | every app; all seven | `metrics.device-pixel-ratio`, `metrics` events, `view.screen`, `screens.screens` and `screen-changes` with scale and refresh rate | nothing found |
| Pointer, touch, trackpad | every app; all seven | `pointer-event` in Flutter's `PointerData` shape, pan and zoom, stylus | modifiers on pointer events and click counts, which every bridge rebuilds from key events and timing; the system's double-click interval (Slint `Platform::click_interval`) |
| Keys and the keyboard layout | every app; all seven | `key-event` with `physical`, `logical`, `character`, `synthesized`; `key-reply` | the layout's name and changes (GPUI `keyboard_layout`), Caps Lock's state |
| Text input and IME | every app with text; all seven | `views.text-input` in Flutter's model: the program pushes the whole value, the IME's edit arrives whole | nothing for Flutter-shaped frameworks; winit-shaped ones (egui, iced, Blitz, Masonry) hold no surrounding text, so the IME sees none *(inferred)* |
| Clipboard | most apps; all seven | `clipboard` (items, MIME types, promises, files) and the `copy`, `cut`, `paste` and `edit` view events | a synchronous read, which Slint (`Platform::clipboard_text`) and GPUI's older call sites need |
| Drag and drop | many apps; five (GPUI `FileDropEvent` and `start_external_drag`, egui `hovered_files`/`dropped_files`, iced `FileHovered`/`FileDropped`, Slint `start_drag`, Makepad `drag_promise`) | none in `views` (drops planned in `2026-10-17-waos-v1-interface.md`, not built) | files dropped in with positions; dragging data out |
| Menu bar and context menus | most desktop apps; four use the system's (GPUI `set_menus`, Slint `setup_menubar`, Makepad `macos_menu`, Dioxus desktop through muda) | `menus.menu-bar`, `context-menus`, provided Edit items | a Dock menu |
| System tray | some apps; Slint (`items::SystemTrayIcon`), Makepad (`tray.rs`), Dioxus desktop (`tray-icon`) | `menus.tray-icons` | nothing found |
| Global shortcuts | few apps; none of the seven | none | all of it |
| File dialogs | most apps; GPUI `prompt_for_paths`, Blitz `open_file_dialog`, Makepad `file_dialogs.rs`; egui and iced apps use `rfd` | `file-dialogs`: open, several, folder, save, async, mounted at their paths | nothing for async callers; Blitz's call is synchronous |
| Alerts and other system dialogs | some apps; GPUI `PlatformWindow::prompt` | none | message boxes, colour and font pickers (GPUI draws its own prompt when the platform answers `None`) |
| Notifications | some apps; GPUI `show_system_notification` | `notifications` with ids, actions, replies, scheduling | nothing found |
| Opening links and files; being opened | most apps; GPUI `open_url`, `on_open_urls`, `reveal_path`; Slint `open_url` | `opener`, `lifecycle.open-requests` | registering a URL scheme (GPUI `register_url_scheme`) |
| The system's appearance | every app; all seven read light or dark | `system.platform-brightness`, `locales`, `changes`; `caption-settings` | accent colour, high contrast, reduced motion, text scale, scroll bar behaviour (GPUI `should_auto_hide_scrollbars`), caret blink (Slint `cursor_flash_cycle`) |
| The system's fonts | every app with text; all seven | `fonts`: families, matching, fallback by character and language, glyphs drawn by the engine | outline tables, withheld by design (`fonts.typeface.tables`), so egui, iced, Slint, Blitz, Masonry and Makepad, which rasterize their own glyphs, bring their own fonts |
| Accessibility | every app, by law in many places; six (AccessKit in GPUI, egui `PlatformOutput::accesskit_update`, Slint, Blitz, Masonry; `makepad-a11y`) | none; a known gap that will change `views` (`2026-10-16-waos-v1.md`) | all of it |
| Threads and parallel work | many apps; GPUI's background executor, Makepad `thread.rs`, Xilem tokio's multi-threaded runtime, egui's image loaders, iced's `thread-pool` feature | one thread; `wasdk::spawn` tasks | threads; workers designed (`2026-10-17-waos-v1-interface.md` decision 10) but not built |
| Timers | every app; all seven | `wasi:clocks/monotonic-clock@0.3.0` `wait-until`, `wait-for` | nothing; executors must be given them (iced `Executor`, GPUI's dispatcher) |
| Network and HTTP | most apps; image loading in egui, iced, Blitz | `wasi:sockets` 0.2, `wasi:http` 0.2, `trust` | crates that pick `fetch` on `wasm32` (egui's `ehttp`, `blitz-net` disabled on wasm32 in `dioxus-native/Cargo.toml`) |
| Processes | some apps (editors, tools) | `machines.machine.exec` with pipes, a working directory and variables; `exec-in-terminal` | nothing for the 10-16 route of Zed's `remote_server` |
| Secrets | some apps; GPUI `write_credentials` | none | a keychain |
| Printing | some document apps; none of the seven | none | all of it |
| Spell check | some text apps; none of the seven | hints only: `text-input-configuration.autocorrect`, `enable-suggestions` | a checker a framework can ask |
| Cursor | every desktop app; all seven | `view.set-cursor` with CSS names | custom images, grabbing or hiding the pointer for games (egui `CursorGrab`) |
| Power and sleep | few apps; GPUI `power_mode`, `prevent_idle_sleep`, `thermal_state` | `lifecycle` states | power mode, idle inhibition |

## What stops each framework, and what only degrades it

| Framework | Stops it today | Only degrades it | The port |
|---|---|---|---|
| GPUI | `gpui_wgpu` needs a surface and its wasm code is the browser's; Zed's wgpu is a git revision of 30, so the program needs one `[patch]` with `wasdk-wgpu` | one thread for background work; no accessibility, window states, drops or keyboard layout | a `gpui_wasdk` platform crate, 3,500 to 3,800 lines (10-16 estimate), and a render-target seam in `gpui_wgpu` |
| egui | `eframe` picks its web backend on `wasm32` and pulls winit natively, so eframe apps cannot run | no accessibility, no IME context, bundled fonts only (CJK by a glyph rasterizer hook) | a WaOS integration over `egui` and `egui-wgpu`, the size of `egui-winit` (3,609 lines) or less *(inferred)*; eframe needs a WASI backend upstream |
| iced | the `iced` crate depends on `iced_winit` unconditionally (`Cargo.toml`), and winit fails to compile (iced's winit fork *inferred* to share upstream's `web_platform` cfg); the default executor on wasm32 is `wasm_bindgen_futures` | no accessibility (iced has none), bundled fonts | a WaOS shell taking iced's `Program` (`iced_program`), like `iced_winit` (4,236 lines) *(inferred)*, with a WaOS executor |
| Slint | `i-slint-core` calls the browser on wasm32 (`detect_operating_system` reads `web_sys::window()`, `debug_log` calls `console.log`) *(inferred to trap at the first call, as gpui_wgpu did on 10-16)* | the clipboard read is synchronous, so paste needs a Slint change; native menus and IME are internal API | a backend over `i-slint-core`, pinned exactly as Slint's own are, about the size of its Android backend (2,667 lines) *(inferred)* |
| Blitz and Dioxus native | `blitz-shell` and `dioxus-native` are winit's; `anyrender_vello`'s window renderer needs a surface | system fonts unusable (vello draws outlines); synchronous file dialog; `blitz-net` needs tokio's multi-threaded runtime | a shell over `blitz-dom` and Dioxus like `blitz-shell` (1,994 lines) plus `dioxus-native` (1,004), and an anyrender renderer to a texture *(inferred)* |
| Xilem and Masonry | wgpu 28 and vello 0.8 (`xilem/Cargo.toml`, `Cargo.lock`); `xilem` and `xilem_masonry` ask tokio for `rt-multi-thread`, a `compile_error!` on wasm (tokio 1.54 `src/lib.rs`) | system fonts unusable; no file drops in `masonry_winit` | a shell over `masonry_core` like `masonry_winit` (1,921 lines), once Xilem is on wgpu 30 |
| Makepad | no WebGPU renderer: Metal, D3D11, OpenGL and Vulkan natively, WebGL on the web; its OS layer is chosen by `cfg` and its wasm one is the browser's | threads used throughout | a WaOS backend under its new HAL and a WebGPU renderer over its WGSL output; the largest *(inferred)* |

### A question for the lead: surfaces

Every framework's default renderer asks wgpu for a `wgpu::Surface` from a window handle: `gpui_wgpu::WgpuRenderer::new`, `iced_wgpu` `window/compositor.rs`, Slint's `FemtoVGWGPURenderer` surface path, `anyrender_vello` `window_renderer.rs`, `masonry_winit`, `egui-wgpu` `winit.rs`. `wasdk-wgpu` refuses one by [the waos project](2026-10-17-the-waos-project.md)'s decision 14, so each needs a path that renders into a texture it is handed; egui, iced and Slint have one, and so does vello (`render_to_texture`, as `anyrender_vello/src/image_renderer.rs` calls it), which serves Blitz and Masonry; GPUI does not. A wgpu surface over a view's canvas context is what wgpu's own browser backend is (`SurfaceTarget::Canvas` over a `GPUCanvasContext`); the obstacle is only naming the view, since wgpu 30's custom backend receives `SurfaceTargetUnsafe`, whose variants are raw handles, DRM and a Core Animation layer (`wgpu-30.0.1/src/api/surface.rs`). wgpu's browser backend already names a canvas by a raw handle: a `RawWindowHandle::Web` carries a number, which it looks up as the canvas's `data-raw-handle` (`wgpu-30.0.1/src/backend/webgpu.rs`, `create_surface`). [The Bevy research](2026-11-05-bevy-on-waos-research.md) asks the same question and recommends option 2. The options:

1. **Keep decision 14**, and port each framework through its render-to-texture path; GPUI gains a seam upstream.
2. **A raw window handle that names a view**, a `Web` handle whose number is the view's as the browser backend's is a canvas's, so frameworks' surface code runs unchanged; the 10-16 research found raw handles the only way into wgpu 30's custom backend.
3. **A custom surface target in wgpu upstream**, which `wasdk-wgpu` then implements over the view's context.

Recommendation: 1 now, since every wgpu framework but GPUI already renders to a texture somewhere, and 3 proposed upstream if ports multiply.

## GPUI

**How it plugs in.** No fork of `gpui`: `Application::with_platform(Rc<dyn Platform>)` and `Application::run_embedded` (`crates/gpui/src/app.rs`, the latter documented for "GPUI compiled into a Wasm guest") let a `gpui_wasdk` crate supply `Platform`, `PlatformWindow`, `PlatformDispatcher` and `PlatformTextSystem` (`crates/gpui/src/platform.rs`). `gpui_platform` chooses `gpui_web` for every `target_family = "wasm"` (`crates/gpui_platform/Cargo.toml`), so a program names `gpui_wasdk` itself. The 10-16 report's mapping stands; what follows is what changed.

**Fixed in GPUI since 10-16:**

- **Blocking.** `ForegroundExecutor::block_on`, `block_with_timeout`, `Scope` and `PlatformScheduler::block` are now compiled out on wasm (`executor.rs`, `platform_scheduler.rs`), and `App::shutdown` spawns its quit futures there instead of blocking. GPUI no longer panics on quit; Zed's own blocking call sites now fail to compile on wasm instead of panicking *(inferred; Zed's other crates were not checked out)*.
- **The clipboard.** `Platform::read_from_clipboard_async` and `App::read_from_clipboard_async` exist, documented for "asynchronous and permission-gated" clipboards. It maps onto `clipboard.items` and `clipboard-item.read`; a user's paste also arrives as `views.view-event.paste`. Whether Zed's ~90 synchronous call sites moved to it is unverified.

**Fixed in WaOS since 10-16:** screens (`screens`, for GPUI's `displays` and `on_displays_changed`); menus pushed whole (`menus.menu-bar.set`), the shape the 10-16 report asked for, with GPUI's `on_validate_app_menu_command` run by the platform crate before each `set` *(inferred)*; `file-dialogs` for `prompt_for_paths` and `prompt_for_new_path`; `notifications` with ids and actions for `show_system_notification`; `opener` for `open_url`, `open_with_system` and `reveal_path`; `lifecycle.open-requests` for `on_open_urls`; `machines.exec` with pipes, the route for Zed's `remote_server`; the 256 MiB cap dropped (wasdk `9e3aa3b`); path resolution rewritten to the system's one call (`crates/waos/src/files/mod.rs`; not measured here).

**Still blocking:**

- `gpui_wgpu` still holds a `wgpu::Surface` (`WgpuRenderer`, `surface_config`, `get_current_texture` in `draw`), and its wasm code is the browser's: `new_from_canvas(&web_sys::HtmlCanvasElement)`, `web-sys` and wgpu's `webgl` on `target_family = "wasm"` (`crates/gpui_wgpu/Cargo.toml`). It needs a render target it is handed each frame and a split of browser from WASI.
- Zed's wgpu is a git revision reporting 30.0.0 (`Cargo.lock`); `wasdk-wgpu` uses crates.io's 30.0.1, so the two must be patched to one *(inferred)*.

**Still missing in WaOS:** window states and moves; `start_window_move` for Zed's own title bar; `keyboard_layout`; `write_credentials`; drops (`FileDropEvent`) and the new `start_external_drag`; accessibility, which GPUI still compiles out on wasm (`window.rs`, `#[cfg(not(target_family = "wasm"))]` around its AccessKit tree); parallel work.

**New precedents in `gpui_web`** (now 7,887 lines, from 2,600): `ime_mirror.rs` keeps a window of the document's text mirrored for the IME, the design the 10-16 report proposed for WaOS's `text-input`; `text_system.rs` wraps `CosmicTextSystem` and draws glyphs no bundled font has through the browser's canvas (`canvas_text.rs`), which is WaOS's `fonts.glyph-alpha` model; its dispatcher runs work on web workers only with shared memory (`multithreaded` feature).

## egui and eframe

**How it plugs in.** egui is built to be embedded: `Context::run(RawInput) -> FullOutput`, where `PlatformOutput` carries the cursor, `OutputCommand::CopyText`, `OpenUrl`, IME rectangles and `accesskit_update` (`crates/egui/src/data/output.rs`). A WaOS integration replaces `egui-winit` and uses `egui_wgpu::Renderer` unchanged: `render(&mut wgpu::RenderPass, ..)` draws into any pass (`crates/egui-wgpu/src/renderer.rs`), so the canvas texture from `wrap_texture` serves; wgpu 30 matches. `eframe`'s `App` cannot be reused without eframe's help: `Frame`'s fields are `pub(crate)` (`crates/eframe/src/epi.rs`) and eframe picks `web` on `target_arch = "wasm32"` (`crates/eframe/src/lib.rs`), with 197 uses of `wasm_bindgen`, `web_sys` or `js_sys`.

**What fits.** Paste is an event, `Event::Paste(String)`, so WaOS's `paste` view event feeds it without a switch; `copy` and `cut` become `Event::Copy`/`Cut`, and the frame's `CopyText` is written through the event's `copy-request` *(inferred)*. `ViewportCommand::RequestPaste` maps to an async read. Several windows are egui's viewports, which an integration may embed in one window instead. Repaint delays map to `request-frame` and a timer.

**What degrades.** IME is winit's shape (`ImeEvent::Enabled`, `Preedit`, `Commit`, `Disabled`) and `IMEOutput` gives rectangles, not the text being edited, so WaOS's `text-input` gets no surrounding text. Text is rasterized by epaint (skrifa and `vello_cpu`), from whole font files (`FontData::font`), so egui uses its bundled fonts; its new hooks help: `FontProvider` (`epaint/src/text/font_provider.rs`) and `GlyphRasterizer`, a per-cluster raster hook that eframe's web backend fills from the browser's canvas (`eframe/src/web/canvas_glyphs.rs`), which a WaOS integration could fill from `fonts.match-character` and `glyph-alpha` for CJK and emoji *(inferred)*. AccessKit updates have nowhere to go. `egui_extras`' HTTP image loading picks `fetch` on wasm32 *(unverified)*.

## iced

**How it plugs in.** `iced::application(..)` is a `Program` (`impl<P: Program> Program for Application<P>`, `src/application.rs`), and `iced_program::Program` is public, so a WaOS shell can run any iced app as `iced_winit::run(program)` does. The `iced` crate itself depends on `iced_winit` unconditionally (`Cargo.toml`, `src/lib.rs` `use iced_winit as shell`) on a winit fork, so a program uses the lower crates or iced makes the shell optional upstream. Rendering fits: `iced_wgpu::Renderer::present(clear, format, &TextureView, viewport)` (`wgpu/src/lib.rs`) is what the `integration` example uses; wgpu 30.0.1 matches; `iced_tiny_skia` is a CPU fallback.

**What fits.** The clipboard is asynchronous throughout: widgets ask through `Shell::read_clipboard` and get `clipboard::Event::Read` later (`core/src/shell.rs`), and `runtime::clipboard::Action::Read` carries a oneshot. Window actions and events are a closed list a shell answers (`runtime/src/window.rs`), multi-window included. The executor is the program's choice (`Program::Executor`, `Application::executor`), so `wasdk::spawn` and WASI timers serve; the default picks `wasm_bindgen_futures` on wasm32 (`futures/src/backend/default.rs`).

**What degrades.** IME is winit-shaped (`core/src/input_method.rs`: preedit and commit, no surrounding text). Text is cosmic-text over bundled Fira Sans and icons (`graphics/src/text.rs` `FontSystem::new_with_fonts`), rasterized from outlines. iced has no accessibility at all (no AccessKit in any manifest). `FileHovered`/`FileDropped` and most window actions have no WaOS counterpart.

## Slint

**How it plugs in.** Slint has the most explicit custom-platform API, written for microcontrollers: `slint::platform::Platform` (`create_window_adapter`, `run_event_loop`, `duration_since_start`, `clipboard_text`, `open_url`, `click_interval`) and `WindowAdapter` (`size`, `renderer`, `request_redraw`, `set_visible`) in `internal/core/platform.rs` and `window.rs`, driven by `update_timers_and_animations`. `FemtoVGWGPURenderer` is "for a custom Slint platform where you want the scene rendered into a WGPU texture", with `render_to_texture(&wgpu::Texture)` under `unstable-wgpu-30` (`internal/renderers/femtovg/wgpu.rs`); `SoftwareRenderer` is the fallback.

**What needs Slint's internals.** The IME, cursor, native menus, popups, drag and focus are on `WindowAdapterInternal`, reached only with an `InternalToken` (`internal/core/lib.rs`); Slint's own backends pin `i-slint-core` exactly (`=1.19.0`), and a WaOS backend would too. Its IME request fits WaOS's model one to one: `InputMethodProperties` carries the text, cursor, anchor and preedit (`window.rs`), as Flutter's `TextEditingValue` does, in UTF-8 offsets to convert. `setup_menubar` and `show_native_popup_menu` map to `menus`.

**What degrades or blocks.** `Platform::clipboard_text` is synchronous and Slint's text input calls it on its own paste keys, so paste needs a Slint change: an async read or a paste event. Its 85 `wasm32` or `wasm` cfgs in `i-slint-core` are the browser's (`web-time`, `web_sys::window()` in `detect_operating_system`, `console.log` in `debug_log`); `OPERATING_SYSTEM_OVERRIDE` may avoid the first *(unverified)*. Text is parley over fontique, rasterized by FemtoVG from outlines, so fonts are bundled or imported in `.slint` files. Accessibility exists only in the winit backend (`accesskit.rs`).

## Dioxus native and Blitz

Dioxus's other renderers do not apply: `dioxus-desktop` is a webview (wry) and `dioxus-web` the browser's DOM. Its native renderer is Blitz: `blitz-dom` (Stylo, Taffy, parley), painted through anyrender, with `dioxus-native` (in both repositories) over it.

**How it plugs in.** `blitz-traits`' `ShellProvider` (`shell.rs`) is the shell as a trait: redraw, cursor, title, IME on and its rectangle, `request_clipboard_text` delivered later as an event, `set_clipboard_text`, `open_file_dialog`, and window chrome. A WaOS shell implements it and replaces `blitz-shell` (winit, 1,994 lines). anyrender's `WindowRenderer` (`crates/anyrender/src/lib.rs`) is the renderer seam; `anyrender_vello` already renders to a texture inside its window renderer (`render_to_texture` in `window_renderer.rs` and `image_renderer.rs`), so a WaOS one is small *(inferred)*. Blitz runs in the browser today on WebGL through `vello_hybrid` (`HOWTO_WASM.md`); vello's compute shaders on WebGPU are unverified under WaOS.

**What degrades.** `open_file_dialog` returns paths synchronously, against `file-dialogs`' async calls. vello draws glyphs from outlines, so Blitz bundles fonts, as its wasm examples do (DejaVu Sans). `blitz-net` needs tokio's threaded runtime and is a no-op on wasm32 (`dioxus-native/Cargo.toml`), so images and CSS over the network need a provider over `wasi:http`. No file drops in `blitz-shell` were found.

## Xilem and Masonry

**How it plugs in.** `masonry_core` names no platform (parley, `ui-events`, AccessKit): a shell feeds it events and acts on its `RenderRootSignal`s (`StartIme`, `ImeMoved`, `ClipboardStore`, `SetCursor`, `SetTitle`, `DragWindow`, `Minimize`, `RequestAnimFrame` and more); paste arrives as `TextEvent::ClipboardPaste(String)` (`masonry_core/src/core/events.rs`), which WaOS's `paste` event fits. `masonry_winit` (1,921 lines) is the shell to replace; Xilem runs only through it (`Xilem::run_in(EventLoopBuilder)`, `xilem/src/app.rs`).

**What blocks.** wgpu 28 and vello 0.8, two majors behind WaOS's wgpu. `xilem` and `xilem_masonry` start a tokio runtime with `rt-multi-thread`, which tokio refuses at compile time on wasm. Both are Xilem's to change. Glyphs are drawn by vello from outlines, so fonts are bundled.

## Makepad

**How it plugs in.** Not without a fork: `makepad-platform` chooses its OS layer by `cfg` (`platform/src/os/mod.rs`; `web` on `target_arch = "wasm32"`, through a JavaScript bridge), and renders with its own backends (`platform/hal/src/backend`: Apple Metal, Windows, GL, Vulkan, Android; WebGL on the web). Its shader compiler emits WGSL (`platform/shader/src/shader_ir_wgsl.rs`), but no backend found consumes it. Makepad is moving to a HAL below `Cx`, commands in and events out over four synchronous entries (`hal_open`, `hal_submit`, `hal_pump`, `hal_wake`, `platform/hal/src/lib.rs`), with "the platform backends come with their phases": a WaOS backend would be one of those, and a WebGPU renderer of its draw commands. Its HAL rasterizer already asks a host's font service for glyphs (`draw/src/text/rasterizer_hal.rs`), WaOS's model. The clipboard read is asynchronous (`platform/src/clipboard_read.rs`); accessibility is its own `makepad-a11y`. Threads are used throughout (`platform/src/thread.rs`). The largest port, against a moving target.

## Where the work would sit

- **In WaOS's WIT, by how many frameworks gain:** accessibility, whose shape across frameworks is AccessKit's tree of values with actions coming back (five frameworks); window states and moves (seven) and starting a move or resize (six); drops in and drags out (five); workers (heavy apps in any framework); appearance beyond light and dark; a keychain; parent windows for dialogs.
- **In the frameworks, upstream:** a `wasm32` split of browser from WASI (winit's `web_platform` already reads `target_os = "unknown"`, `winit/build.rs`; eframe, iced, Slint, gpui_wgpu and Makepad read `target_arch`); render-to-texture paths where missing (GPUI); async clipboard reads (Slint) and dialogs (Blitz); Xilem on wgpu 30 and off tokio's threaded runtime on wasm.
- **In new crates over the SDK, as `inset-embedder-wasdk` (7,339 lines) is for Inset:** one shell per framework. winit 0.31 splits its traits into `winit-core` (`EventLoopProvider`, `ActiveEventLoop`, `Window`, `EventLoopExtRegister::register_app`), but its facade picks the platform itself (`winit/src/event_loop.rs`, `platform_impl::EventLoop`), and the frameworks name the facade, so a `winit-waos` would serve them only with upstream changes in each *(inferred)*.
