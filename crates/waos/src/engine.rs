//! The engine an app embeds: one wasmtime engine every program runs on, set up for components
//! with their async ABI, fuel and an epoch tick; one GPU instance every program draws on; the
//! shell it asks; the machine's facts, set whole and worked out into what changed; the screens'
//! refreshes every program's frames wait on; the programs running, for what the app's suspending
//! and resuming and a screen's refresh do to all of them; and the public API that compiles, loads
//! and runs programs.
//!
//! The invariant: code compiled ahead of time loads only into an engine of the same wasmtime and
//! the same settings. wasmtime checks most of them at every load, but not native unwind
//! information, so the code cache names each file after the engine's settings too: a setting
//! changed here names other files, and each program is compiled again at its next load.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result, ensure};
use futures::FutureExt;
use futures::future::join_all;
use waos_capabilities::{AuditDay, AuditLog, ProgramEnvironment};
use wasmtime::{Config, Engine, WasmBacktraceDetails};

use crate::access::Days;
use crate::bindings::wasdk::media::caption_settings::CaptionStyle;
use crate::bindings::wasdk::platform::screens::ScreenId;
use crate::bindings::wasdk::platform::system::{Brightness, Locale};
use crate::caption_settings::CaptionSettings;
use crate::clipboard::PlatformClipboard;
use crate::code_cache;
use crate::engine_time::{EngineClock, EngineTime};
use crate::gpu::{self, GpuInstance};
use crate::inspection::InspectedPrograms;
use crate::lifecycle::FreezePolicy;
use crate::menus::MenuPlaces;
use crate::network::HttpProxy;
use crate::program::Program;
use crate::running::{HeldProgram, Hold, ProgramStart, Programs, RunningProgram};
use crate::screens::{Screen, Screens};
use crate::screenshots::Screenshots;
use crate::shell::{PlacedView, Shell, ShellClipboardItem};
use crate::source_lines::ComponentFile;
use crate::system::SystemSettings;
use crate::temporary_folders::TemporaryFolders;
use crate::views::{Refresh, ReservedView, ScreenRefreshes};

/// How hardware buffers are backed on Linux. Ignored on other platforms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HardwareBufferMode {
    /// Use zero-copy dmabufs when a DRM render node and a dmabuf-capable Vulkan adapter are
    /// available; fall back to offscreen textures otherwise.
    #[default]
    Auto,
    /// Use offscreen GPU textures with staging-buffer readback on Linux without opening a DRM
    /// render node or requiring dmabuf Vulkan extensions.
    Offscreen,
}

/// How the engine is set up.
#[derive(Clone)]
pub struct WaOSConfig {
    /// Where compiled programs are kept, so the next start maps the code instead of compiling
    /// it. A directory of the app's own: wasmtime trusts the code it maps.
    pub code_cache: PathBuf,
    /// When a program out of sight is frozen.
    pub freeze: FreezePolicy,
    /// Whether programs are compiled with native unwind information (wasmtime's
    /// `Config::native_unwind_info`): what a native profiler or debugger needs to walk through a
    /// program's frames, and nothing else does, wasmtime's own backtraces of a program among
    /// them. Off unless asked for: macOS's unwinder lets go of each compiled function's
    /// information one at a time, at a cost that grows with the square of their number, so a
    /// large program takes seconds to drop with it (the gallery's debug build, 7 s against 2 ms).
    /// Always on Windows, whose exception handling needs it, and where wasmtime refuses it off.
    pub native_unwind_info: bool,
    /// Which proxy programs' HTTP goes through: the system's, as an app sends, or none, as the
    /// tests send, so what they reach does not depend on the machine's settings.
    pub http_proxy: HttpProxy,
    /// What moves the engine's time, which freezes, a view's hide and show and its spare
    /// hardware buffers are timed by: its own thread every 100 ms, as an app has it, or a test.
    pub time: EngineTime,
    /// Where each running program's `/tmp` is made, a folder of its own, removed once it ends: a
    /// folder of the app's own among its caches, on the volume of the programs' `/data`, so a file
    /// renamed from one into the other is one rename. One the engine starts on is cleared of what
    /// a crash left there.
    pub temporary_folders: PathBuf,
    /// Where each program's audit summary is kept, day by day: the store, for the WaOS app;
    /// nowhere for an engine given none, as the tests' are.
    pub audit: Option<Arc<dyn AuditLog>>,
    /// The programs a program holding the inspector role sees: the store's and the runs', for
    /// the WaOS app; none for an engine given none, whose inspectors see no device.
    pub inspected_programs: Option<Arc<dyn InspectedPrograms>>,
    /// How hardware buffers are backed on Linux: automatic (dmabuf with offscreen fallback) or
    /// offscreen. Decided once at engine startup.
    pub hardware_buffers: HardwareBufferMode,
}

/// What the platform reports about the machine when the engine starts; the setters keep it
/// current.
#[derive(Clone, Debug)]
pub struct PlatformState {
    pub screens: Vec<Screen>,
    pub platform_brightness: Brightness,
    /// The user's languages, most preferred first.
    pub locales: Vec<Locale>,
    /// The IANA name of the user's time zone; none where the system has none.
    pub time_zone: Option<String>,
    /// How the user set captions to look; none where the system keeps no caption settings, and
    /// then a program's `get-caption-settings` answers none.
    pub caption_style: Option<CaptionStyle>,
}

/// The engine: runs programs and serves `wasdk:platform` and `wasdk:media` to them.
pub struct WaOS {
    engine: Arc<Engine>,
    gpu: GpuInstance,
    /// The resolved hardware buffer backing mode for this engine.
    hardware_buffers: HardwareBufferMode,
    /// What draws the screenshots of every program's views, on the same instance.
    screenshots: Arc<Screenshots>,
    shell: Shell,
    screens: Screens,
    /// The screens' refreshes asked of the shell and not come yet, and the last each had.
    refreshes: ScreenRefreshes,
    settings: SystemSettings,
    caption_settings: Option<CaptionSettings>,
    clipboard: PlatformClipboard,
    code_cache: PathBuf,
    /// Where each program's `/tmp` is made.
    temporary_folders: Arc<TemporaryFolders>,
    /// The days of what each program did, counted in memory and kept by their writer, where the
    /// engine keeps them.
    days: Option<Days>,
    /// What an inspector sees of the programs.
    inspected_programs: Option<Arc<dyn InspectedPrograms>>,
    freeze: FreezePolicy,
    http_proxy: HttpProxy,
    /// The engine's time, which every program's freeze and hardware buffers read.
    clock: EngineClock,
    programs: Programs,
}

impl WaOS {
    /// An engine over `shell`, knowing the machine as `platform` says. Its time ticks for as
    /// long as it lives, as `config.time` moves it.
    pub fn new(config: WaOSConfig, shell: Shell, platform: PlatformState) -> Result<WaOS> {
        std::fs::create_dir_all(&config.code_cache)
            .with_context(|| format!("making {}", config.code_cache.display()))?;
        let temporary_folders = Arc::new(TemporaryFolders::new(config.temporary_folders)?);
        let engine = Arc::new(Engine::new(&engine_config(config.native_unwind_info))?);
        let programs = Programs::default();
        let clock = config.time.start(&engine, &programs)?;
        let gpu = gpu::instance();
        let hardware_buffers = crate::os::resolve_buffer_mode(&gpu, config.hardware_buffers);
        let screens = Screens::new(platform.screens);
        let days = config
            .audit
            .map(Days::new)
            .transpose()
            .context("the thread that keeps the programs' days")?;
        Ok(WaOS {
            engine,
            screenshots: Arc::new(Screenshots::new(Arc::clone(&gpu), hardware_buffers)),
            gpu,
            hardware_buffers,
            refreshes: ScreenRefreshes::new(Arc::clone(&shell.vsync), screens.clone()),
            clipboard: PlatformClipboard::new(shell.clipboard.clone()),
            shell,
            screens,
            settings: SystemSettings::new(
                platform.platform_brightness,
                platform.locales,
                platform.time_zone,
            ),
            caption_settings: platform.caption_style.map(CaptionSettings::new),
            code_cache: config.code_cache,
            temporary_folders,
            days,
            inspected_programs: config.inspected_programs,
            freeze: config.freeze,
            http_proxy: config.http_proxy,
            clock,
            programs,
        })
    }

    /// The platform's screens now, the whole list, each time it changes; programs hear which
    /// were added, removed or changed.
    pub fn set_screens(&self, screens: Vec<Screen>) {
        self.screens.set(screens);
    }

    /// `screen` refreshed at `at`, as the shell's `VsyncWaiter` was asked, the picture drawn for it
    /// expected on screen at `display_time`: each program's views on it that asked for a frame and
    /// are in sight get one, the frame naming them all.
    pub fn screen_refreshed(&self, screen: ScreenId, at: Instant, display_time: Instant) {
        let refresh = Refresh { at, display_time };
        self.refreshes.came(screen, refresh);
        self.programs.screen_refreshed(screen, refresh);
    }

    /// The user's light or dark preference now; programs hear of a change.
    pub fn set_platform_brightness(&self, brightness: Brightness) {
        self.settings.brightness_changed(brightness);
    }

    /// The user's languages now, most preferred first; programs hear of a change.
    pub fn set_locales(&self, locales: Vec<Locale>) {
        self.settings.locales_changed(locales);
    }

    /// The IANA name of the user's time zone now, none where the system has none; programs hear
    /// of a change, and their offsets from UTC follow it.
    pub fn set_time_zone(&self, name: Option<String>) {
        self.settings.time_zone_changed(name);
    }

    /// How the user set captions to look now; programs hear it. Where the system kept no caption
    /// settings as the engine started, programs have none, and this changes nothing.
    pub fn set_caption_style(&self, style: CaptionStyle) {
        if let Some(settings) = &self.caption_settings {
            settings.style_changed(style);
        }
    }

    /// What is on the platform's clipboard now, as the shell lists it, each time a writer other
    /// than the engine's programs changes it: a change, which replaces the copy before it.
    /// Programs with a focused view hear of it. A program's own write is not reported here: the
    /// shell answers it with its listing (`ShellClipboard::write`).
    pub fn set_clipboard(&self, items: Vec<Arc<dyn ShellClipboardItem>>) {
        self.clipboard.set(items);
    }

    /// The program at `component`, compiled once, at its first load unless `compile` compiled it
    /// before, and mapped from the code cache after; blocks while compiling, so call it off the
    /// main thread.
    pub fn load(&self, component: &Path) -> Result<Program> {
        let file = ComponentFile::at(component);
        let compiled = code_cache::load(&self.engine, component, &self.code_cache)?;
        Ok(Program::new(compiled, name_of(component), file))
    }

    /// Compiles the program at `component` into the code cache ahead of its load, as an app
    /// compiles a program it installs, so that `load` maps it without compiling; nothing to do
    /// when the cache holds its code already. Blocks while compiling, so call it off the main
    /// thread.
    pub fn compile(&self, component: &Path) -> Result<()> {
        code_cache::compile(&self.engine, component, &self.code_cache)
    }

    /// Whether the code cache holds the code of the program at `component` as it is now, compiled
    /// by this engine, so that `load` maps it without compiling.
    pub fn is_compiled(&self, component: &Path) -> Result<bool> {
        code_cache::is_compiled(&self.engine, component, &self.code_cache)
    }

    /// Removes the code this engine compiled from the program at `component` from the code
    /// cache, as an app does with a program it removes; call it while the component is still
    /// there, since its code is found by its path. A program running keeps the code it mapped.
    /// The code engines of other settings or another wasmtime compiled from it stays.
    pub fn remove_compiled(&self, component: &Path) -> Result<()> {
        code_cache::remove(&self.engine, component, &self.code_cache)
    }

    /// The days kept of what the program `name` did, newest first, today's as counted now in
    /// place of the one kept; none where the engine keeps no days. Reads the kept days from the
    /// engine's audit log.
    pub fn days_of(&self, name: &str) -> Vec<AuditDay> {
        self.days
            .as_ref()
            .map(|days| days.days_of(name))
            .unwrap_or_default()
    }

    /// Keeps every program's day as counted now, after each day the writer was asked to keep
    /// before, and returns once they are kept: an app calls it as it quits, after its programs
    /// end, since a process ended without unwinding never drops the engine. Writes files, so
    /// call it off the main thread.
    pub fn keep_every_day(&self) {
        if let Some(days) = &self.days {
            days.keep_all();
        }
    }

    /// Starts `program` on a thread of its own, with `environment` as its capabilities and a
    /// `/tmp` of its own. It has no view until it asks for one, and every request goes to the
    /// shell's placement.
    pub fn run(
        &self,
        program: &Program,
        environment: ProgramEnvironment,
    ) -> Result<RunningProgram> {
        let (running, _) = self.start(program, environment, None, None)?;
        Ok(running)
    }

    /// Starts `program` as [`WaOS::run`] does, with `reserved`, a screen the shell shows already,
    /// such as a phone's tab the user opened it into, kept for its first `request-view`; answers
    /// the handle on it and what resolves with the view the screen becomes, or with none once the
    /// program ends without asking. Every later request goes to the shell's placement.
    pub fn run_reserving(
        &self,
        program: &Program,
        environment: ProgramEnvironment,
        reserved: PlacedView,
    ) -> Result<(RunningProgram, ReservedView)> {
        let (running, reserved) = self.start(program, environment, Some(reserved), None)?;
        Ok((running, reserved.expect("a screen reserved is waited on")))
    }

    /// Starts `program` as [`WaOS::run`] does, but held before its first instruction until the
    /// [`HeldProgram`] answered is dropped, so whoever listens to it once [`HeldProgram::linked`]
    /// resolves hears it from its start.
    pub fn run_held(
        &self,
        program: &Program,
        environment: ProgramEnvironment,
    ) -> Result<(RunningProgram, HeldProgram)> {
        let (held, hold) = crate::running::hold();
        let (running, _) = self.start(program, environment, None, Some(hold))?;
        Ok((running, held))
    }

    fn start(
        &self,
        program: &Program,
        environment: ProgramEnvironment,
        reserved: Option<PlacedView>,
        hold: Option<Hold>,
    ) -> Result<(RunningProgram, Option<ReservedView>)> {
        ensure!(
            Engine::same(&self.engine, program.component().engine()),
            "{} was loaded by another engine",
            program.name()
        );
        let temporary = self.temporary_folders.make(program.name());
        let start = ProgramStart {
            engine: Engine::clone(&self.engine),
            gpu: Arc::clone(&self.gpu),
            hardware_buffers: self.hardware_buffers,
            screenshots: Arc::clone(&self.screenshots),
            component: program.component().clone(),
            name: program.name().to_owned(),
            file: program.file().clone(),
            placement: Arc::clone(&self.shell.views),
            compositor: self.shell.compositor.clone(),
            prompts: Arc::clone(&self.shell.prompts),
            screens: self.screens.clone(),
            refreshes: self.refreshes.clone(),
            settings: self.settings.clone(),
            caption_settings: self.caption_settings.clone(),
            clipboard: self.clipboard.clone(),
            menus: MenuPlaces::of(&self.shell),
            audio_output_picker: self.shell.audio_output_picker.clone(),
            recording_indicator: self.shell.recording_indicator.clone(),
            file_dialogs: self.shell.file_dialogs.clone(),
            opener: self.shell.opener.clone(),
            notifications: self.shell.notifications.clone(),
            media_players: self.shell.media_players.clone(),
            media_assets: self.shell.media_assets.clone(),
            image_codecs: self.shell.image_codecs.clone(),
            video_frame_readers: self.shell.video_frame_readers.clone(),
            cameras: self.shell.cameras.clone(),
            simulated_cameras: self.shell.simulated_cameras.clone(),
            machine_picker: self.shell.machine_picker.clone(),
            environment,
            temporary_folder: temporary.as_ref().map(|folder| folder.path().to_owned()),
            days: self.days.clone(),
            inspected_programs: self.inspected_programs.clone(),
            freeze: self.freeze,
            http_proxy: self.http_proxy,
            clock: self.clock.clone(),
            programs: self.programs.clone(),
            view_layers: Arc::default(),
            hold,
        };
        RunningProgram::start(start, reserved, temporary)
    }

    /// The platform sent the whole app to the background (iOS's `didEnterBackground`, Android's
    /// `ProcessLifecycleOwner` `ON_STOP`): every program is `paused` and freezes now, and one
    /// started before the app resumes starts so. Resolves once none runs: each stands frozen,
    /// waits in a host call, or has ended. Awaited inside the platform's grace period. Step 4c,
    /// drawing, adds to what each program's stop means: and none of its GPU work is in flight
    /// (`RunningProgram::idle`).
    pub fn app_suspending(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let stopped = self.programs.suspend();
        // A freeze takes at an epoch tick: this one, rather than the next tick of the engine's
        // time.
        self.engine.increment_epoch();
        join_all(stopped).map(|_: Vec<()>| ())
    }

    /// The platform brought the app back to the foreground: every program is back in the state
    /// its views give, and thawed if that shows it.
    pub fn app_resumed(&self) {
        self.programs.resume();
    }
}

impl Drop for WaOS {
    /// The engine goes once every day is kept, so nothing counted is lost with it.
    fn drop(&mut self) {
        self.keep_every_day();
    }
}

/// Components with their async ABI, fuel and an epoch tick for what a program costs and for
/// freezing it, backtraces without wasmtime's details, and native unwind information where
/// `native_unwind_info` asks for it. Cranelift
/// compiles programs for this system's engine ([`compile_for_this_system`]). A program's trap is
/// caught on macOS by a signal handler on its own thread, as on every other Unix (the project
/// proposal's decision 58).
///
/// The invariant: every engine in the process is made from this config, since wasmtime installs
/// its trap handler once, at the first engine, and refuses an engine that asks for another.
pub(crate) fn engine_config(native_unwind_info: bool) -> Config {
    let mut config = Config::new();
    compile_for_this_system(&mut config);
    // wasmtime's Mach exception thread lets a trap end the whole process when a caught signal,
    // such as a child's `SIGCHLD`, interrupts its read of the trapping thread's registers.
    config.macos_use_mach_ports(false);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.consume_fuel(true);
    // How a program's fuel gets read: a tick interrupts it wherever it is, which is the one
    // moment wasmtime lets the engine look into a running store.
    config.epoch_interruption(true);
    // A backtrace names each frame by its function alone: wasmtime's details would read the
    // program's DWARF at every unwind, a stop's among them. A program that fails has its frames'
    // files and lines read then (`source_lines`). Part of the code cache's key, as every setting.
    config.wasm_backtrace_details(WasmBacktraceDetails::Disable);
    // Windows walks every frame with it to handle an exception, so wasmtime refuses it off there.
    config.native_unwind_info(native_unwind_info || cfg!(windows));
    config
}

/// iOS gives an app no executable memory, so programs are compiled to Pulley's bytecode, which
/// wasmtime's interpreter runs, and which the code cache keeps as it keeps native code. The
/// simulator is held to the same rule, though it would map Cranelift's code, so that it fails
/// where an iPhone would.
///
/// A program's memory reserves only what it holds and room to grow, not wasmtime's 64-bit 4 GiB:
/// iOS gives an app a few gigabytes of address space in all, so the second program's 4 GiB failed
/// to map. Pulley checks every access itself, without guard pages or signals (wasmtime's
/// `Tunables::default_for_target`), so the large reservation buys it nothing.
#[cfg(target_os = "ios")]
fn compile_for_this_system(config: &mut Config) {
    config
        .target("pulley64")
        .expect("`pulley64` is a target wasmtime names");
    config.memory_reservation(0);
    // Room for a memory to grow before it is moved and copied; a move per 256 MiB grown keeps
    // copies rare, and a dozen programs still fit in iOS's address space.
    config.memory_reservation_for_growth(256 << 20);
}

/// Elsewhere programs are compiled to this machine's own code, Cranelift's default.
#[cfg(not(target_os = "ios"))]
fn compile_for_this_system(_config: &mut Config) {}

/// What names the component at `path`: its file's name without the extension.
fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A program stopped gives every GPU object it held back, measured on macOS as the experiment
/// measured it: wgpu-core's registries, read from inside the crate, and Metal's count of what is
/// allocated on the device. The program is `examples/triangle`, built by the test programs'
/// crate, on a shell of this module's whose screen refreshes as soon as it is asked and which
/// releases each hardware buffer as it is presented.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{OnceLock, Weak};
    use std::thread;
    use std::time::{Duration, Instant};

    use futures::executor::block_on;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_metal::MTLDevice;

    use super::*;
    use crate::bindings::wasdk::platform::views::{
        Cursor, Insets, Metrics, OverlayStyle, Placement, ViewError, ViewFocusDirection,
        ViewFocusState,
    };
    use crate::lifecycle::Presence;
    use crate::running::ProgramEnd;
    use crate::shell::{
        HardwareBuffer, ShellCompositionLayer, ShellView, ViewPlacement, ViewRequest, VsyncWaiter,
    };

    /// How long a program may take to draw its first frames.
    const DRAWING_TIMEOUT: Duration = Duration::from_secs(60);

    /// How long what a program held may take to go once it has ended: a frame still on the GPU
    /// is waited for first.
    const RELEASE_TIMEOUT: Duration = Duration::from_secs(5);

    /// The one screen the shell's views are on.
    const SCREEN: ScreenId = 1;

    /// How long after it is asked the shell's screen refreshes.
    const REFRESH_INTERVAL: Duration = Duration::from_millis(1);

    /// A shell whose screen refreshes a moment after it is asked, on a thread of its own, which
    /// counts the frames presented and releases each hardware buffer at once, and gives no view on
    /// request.
    #[derive(Default)]
    struct Refreshing {
        presents: AtomicUsize,
        /// The engine the refreshes are reported to, once it is made.
        engine: OnceLock<Weak<WaOS>>,
    }

    impl Refreshing {
        fn wait_for_presents(&self, count: usize) {
            let deadline = Instant::now() + DRAWING_TIMEOUT;
            while self.presents.load(Ordering::SeqCst) < count {
                assert!(
                    Instant::now() < deadline,
                    "{count} frames were never presented"
                );
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl ShellView for Refreshing {
        fn present(
            &self,
            _hardware_buffer: HardwareBuffer,
            _layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
            released: Box<dyn FnOnce() + Send + Sync>,
        ) {
            self.presents.fetch_add(1, Ordering::SeqCst);
            released();
        }

        fn set_cursor(&self, _cursor: Cursor) {}

        fn request_focus_change(&self, _state: ViewFocusState, _direction: ViewFocusDirection) {}

        fn set_handles_back(&self, _handles: bool) {}

        fn set_overlay_style(&self, _style: OverlayStyle) {}
    }

    impl VsyncWaiter for Refreshing {
        fn next_vsync(&self, screen: ScreenId) {
            let engine = self.engine.get().cloned();
            thread::spawn(move || {
                thread::sleep(REFRESH_INTERVAL);
                if let Some(engine) = engine.as_ref().and_then(Weak::upgrade) {
                    let now = Instant::now();
                    engine.screen_refreshed(screen, now, now + REFRESH_INTERVAL);
                }
            });
        }
    }

    impl ViewPlacement for Refreshing {
        fn view_requested(
            &self,
            _program: &RunningProgram,
            _request: ViewRequest,
        ) -> Result<(), ViewError> {
            Err(ViewError::Unsupported)
        }
    }

    /// An engine over `shell`, with the code cache the tests share, which `shell` reports its
    /// refreshes to.
    fn engine(shell: &Arc<Refreshing>) -> Arc<WaOS> {
        let config = WaOSConfig {
            code_cache: PathBuf::from(waos_test_programs::CODE_CACHE),
            freeze: FreezePolicy::default(),
            native_unwind_info: false,
            http_proxy: HttpProxy::None,
            time: EngineTime::Real,
            temporary_folders: std::env::temp_dir().join("waos-tests"),
            audit: None,
            inspected_programs: None,
            hardware_buffers: HardwareBufferMode::Auto,
        };
        let platform = PlatformState {
            screens: Vec::new(),
            platform_brightness: Brightness::Light,
            caption_style: None,
            locales: Vec::new(),
            time_zone: None,
        };
        let shell_parts = Shell {
            views: Arc::clone(shell) as Arc<dyn ViewPlacement>,
            vsync: Arc::clone(shell) as Arc<dyn VsyncWaiter>,
            compositor: None,
            prompts: Arc::new(crate::shell::NoPrompts),
            clipboard: None,
            menu_bar: None,
            context_menus: None,
            tray_icons: None,
            audio_output_picker: None,
            recording_indicator: None,
            file_dialogs: None,
            opener: None,
            notifications: None,
            media_players: None,
            media_assets: None,
            image_codecs: None,
            video_frame_readers: None,
            cameras: None,
            simulated_cameras: None,
            machine_picker: None,
        };
        let engine = Arc::new(WaOS::new(config, shell_parts, platform).expect("an engine"));
        let _ = shell.engine.set(Arc::downgrade(&engine));
        engine
    }

    /// A view of 1600 by 1000 pixels on `shell`, on its screen, in sight and focused.
    fn placed_view(shell: &Arc<Refreshing>) -> PlacedView {
        let none = Insets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        PlacedView {
            view: Arc::clone(shell) as Arc<dyn ShellView>,
            text_input: None,
            window: None,
            metrics: Metrics {
                width: 1600,
                height: 1000,
                device_pixel_ratio: 2.0,
                padding: none,
                view_padding: none,
                view_insets: none,
            },
            screen: Some(SCREEN),
            presence: Presence {
                on_screen: true,
                occluded: false,
                focused: true,
            },
        }
    }

    /// What is held on the GPU: wgpu-core's objects of each kind, and Metal's bytes.
    #[derive(Clone, Debug, PartialEq)]
    struct GpuHoldings {
        objects: Vec<(&'static str, usize)>,
        metal_allocated: usize,
    }

    impl GpuHoldings {
        fn read(engine: &WaOS, metal: &Retained<ProtocolObject<dyn MTLDevice>>) -> GpuHoldings {
            GpuHoldings {
                objects: gpu::objects_by_kind(&engine.gpu),
                metal_allocated: metal.currentAllocatedSize(),
            }
        }

        /// What is held once it is back to `before` in what `checked` names, or, if it is not
        /// within [`RELEASE_TIMEOUT`], what is held then.
        fn once_back_to(
            before: &GpuHoldings,
            checked: Checked,
            engine: &WaOS,
            metal: &Retained<ProtocolObject<dyn MTLDevice>>,
        ) -> GpuHoldings {
            let deadline = Instant::now() + RELEASE_TIMEOUT;
            loop {
                let now = GpuHoldings::read(engine, metal);
                if now.is_back_to(before, checked) || Instant::now() > deadline {
                    return now;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }

        fn is_back_to(&self, before: &GpuHoldings, checked: Checked) -> bool {
            let metal_back = self.metal_allocated <= before.metal_allocated;
            self.objects == before.objects && (checked == Checked::Objects || metal_back)
        }
    }

    /// What a run's stop is checked for: wgpu-core's objects on every run, and Metal's bytes once
    /// a run before it has warmed Metal's caches, which keep what that first program left them.
    #[derive(Clone, Copy, PartialEq)]
    enum Checked {
        Objects,
        ObjectsAndMetal,
    }

    /// What was held before a run, while it drew, and once it was stopped.
    struct RunHoldings {
        before: GpuHoldings,
        drawing: GpuHoldings,
        stopped: GpuHoldings,
    }

    fn mib(bytes: usize) -> f64 {
        bytes as f64 / (1024.0 * 1024.0)
    }

    /// Runs triangle until it has drawn ten frames and stops it where it stands, twice. Metal
    /// keeps caches of its own once the process has had a device, which do not grow after a
    /// first program ended, so the first run warms them and Metal's count is checked on the
    /// second; wgpu-core's objects are checked on both.
    #[test]
    fn stopping_triangle_gives_its_gpu_objects_back() {
        let shell = Arc::new(Refreshing::default());
        let engine = engine(&shell);
        let triangle = engine
            .load(Path::new(waos_test_programs::TRIANGLE))
            .expect("triangle loads");
        let metal = objc2_metal::MTLCreateSystemDefaultDevice().expect("a Metal device");
        let runs = [
            ("first", Checked::Objects),
            ("second", Checked::ObjectsAndMetal),
        ];
        let [first, second] = runs.map(|(run, checked)| {
            let before = GpuHoldings::read(&engine, &metal);
            let presented = shell.presents.load(Ordering::SeqCst);
            let (running, _reserved) = engine
                .run_reserving(&triangle, ProgramEnvironment::default(), placed_view(&shell))
                .expect("triangle runs");
            shell.wait_for_presents(presented + 10);
            let drawing = GpuHoldings::read(&engine, &metal);
            running.stop();
            assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
            let stopped = GpuHoldings::once_back_to(&before, checked, &engine, &metal);
            eprintln!(
                "triangle, {run} run: Metal allocated {:.1} MiB before it started, {:.1} MiB while it drew, {:.1} MiB once it was stopped",
                mib(before.metal_allocated),
                mib(drawing.metal_allocated),
                mib(stopped.metal_allocated)
            );
            RunHoldings {
                before,
                drawing,
                stopped,
            }
        });
        for (run, holdings) in [("first", &first), ("second", &second)] {
            assert_ne!(
                holdings.drawing.objects, holdings.before.objects,
                "triangle's {run} run held GPU objects while it drew"
            );
            assert_eq!(
                holdings.stopped.objects, holdings.before.objects,
                "wgpu-core's objects, by kind, once triangle's {run} run was stopped"
            );
        }
        assert!(
            second.stopped.metal_allocated <= second.before.metal_allocated,
            "Metal's allocations once triangle's second run was stopped: {} bytes, against {} before it",
            second.stopped.metal_allocated,
            second.before.metal_allocated
        );
    }
}
