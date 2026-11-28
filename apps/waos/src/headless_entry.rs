//! Headless WaOS daemon: runs the engine, local capabilities, store, dev server, the system's
//! video decoders, which need no window, and a screen with
//! no display (`waos_headless`) with no `winit`, Wayland, X11, or `waft-single-instance`. Used on headless Linux
//! servers/CI and whenever `waos --headless` is invoked.
//!
//! The invariant: every question Permissions asks stays open through `PromptsOnRequest` until
//! answered via `waos settings set` (or the program ends), listed among what Permissions asks,
//! identically to desktop WaOS.
//! When `idle_timeout` is non-zero, the daemon exits cleanly once no programs are running and no
//! dev-server connections are active for that duration, dropping `DevServer` so the socket file is
//! removed.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::sync::{oneshot, watch};
use waos::bindings::wasdk::platform::system::{Brightness, Locale};
use waos::{Compiler, EngineTime, FreezePolicy, HttpProxy, PlatformState, WaOS};
use waos_headless::{Display, HeadlessShell, Refreshes};
use waos_local::LocalCapabilities;
use waos_services::permissions::Prompts;
use waos_services::sources::SourceProxy;

use crate::logic::asking_now::AskingNow;
use crate::logic::dev_server::{DevServer, ServedDomains};
use crate::logic::developer_access::DeveloperAccess;
use crate::logic::folders::{self, AppFolders};
use crate::logic::program_places::ProgramPlace;
use crate::logic::prompts::{PromptRequests, PromptsOnRequest};
use crate::logic::running_apps::RunningApps;
use crate::logic::startup::{self, BeforeTheEngine, EngineCompiler, Outside, Setup};
use crate::logic::video_decoder_thread;
use crate::os;
use crate::write_traces;

/// Configuration for a headless WaOS daemon run.
pub(crate) struct HeadlessConfig {
    pub(crate) folders: AppFolders,
    pub(crate) socket: PathBuf,
    /// Exits after this long with no active runs and no open dev-server connections; `None` or
    /// zero waits forever (until Ctrl-C / SIGTERM).
    pub(crate) idle_timeout: Option<Duration>,
    pub(crate) display: Display,
}

/// The screen a headless WaOS shows its programs' windows on: 1280 by 800 pixels at a scale of 1,
/// each view in a window of its own.
const DISPLAY: Display = Display {
    width: 1280,
    height: 800,
    scale: 1.0,
    windows: true,
};

/// How long after the engine asks the headless screen refreshes, as a 60 Hz screen does.
const REFRESH_INTERVAL: Duration = Duration::from_millis(16);

/// Entry point for `waos --headless [--idle-timeout <secs>]`.
pub(crate) fn run(idle_timeout: Option<Duration>) -> Result<()> {
    write_traces(std::io::stderr, false);
    let config = HeadlessConfig {
        folders: folders::app_folders(),
        socket: waos_dev_protocol::listener_path(),
        idle_timeout: idle_timeout.filter(|duration| !duration.is_zero()),
        display: DISPLAY,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the headless runtime")?;
    runtime.block_on(serve_until_shutdown(config))
}

/// Runs the headless daemon until Ctrl-C, SIGTERM, or `config.idle_timeout` elapses with nothing
/// running and no connections open.
async fn serve_until_shutdown(config: HeadlessConfig) -> Result<()> {
    let idle_timeout = config.idle_timeout;
    let daemon = HeadlessDaemon::start(config, tokio::runtime::Handle::current())?;
    let active_connections = daemon.server.active_connections();
    let runs = Arc::clone(&daemon.runs);

    tokio::select! {
        _ = shutdown_signal() => {
            tracing::info!("headless WaOS received shutdown signal");
        }
        _ = wait_until_idle(&runs, active_connections, idle_timeout) => {
            tracing::info!(
                seconds = idle_timeout.map(|d| d.as_secs()).unwrap_or(0),
                "headless WaOS idle timeout reached; shutting down"
            );
        }
    }

    daemon.stop();
    Ok(())
}

/// A running headless WaOS daemon: engine, store, prompt waiter, and dev server. Dropping it stops
/// the dev server (stopping every run and removing the socket file) and closes prompt tasks.
struct HeadlessDaemon {
    runs: Arc<RunningApps>,
    server: DevServer,
    stop_prompts: Option<oneshot::Sender<()>>,
    _engine: Arc<WaOS>,
}

impl HeadlessDaemon {
    fn start(config: HeadlessConfig, runtime: tokio::runtime::Handle) -> Result<HeadlessDaemon> {
        let compiler =
            Compiler::new(native_unwind_info_asked()).context("starting the compiler")?;
        let setup = Setup {
            store: config.folders.store.clone(),
            temporary_folders: config.folders.temporary_folders.clone(),
            install_compiler: Arc::new(EngineCompiler(compiler.clone())),
            compiler,
            source_proxy: SourceProxy::System,
            http_proxy: HttpProxy::System,
            freeze: FreezePolicy::default(),
            time: EngineTime::Real,
            usual_place: ProgramPlace::Windows,
        };
        let display = config.display;
        let (started, (prompted, asking)) = startup::start(
            setup,
            |running| {
                let (prompts, prompted) = PromptsOnRequest::new(Arc::clone(running));
                let prompts = Arc::new(prompts);
                (
                    Arc::clone(&prompts) as Arc<dyn Prompts>,
                    (prompts, prompted),
                )
            },
            |before, (prompts, prompted)| Ok(headless_outside(display, before, &prompts, prompted)),
        )?;
        let startup::Started {
            running,
            packages,
            permissions,
            engine,
            environments,
            starter,
            ..
        } = started;
        let served = ServedDomains {
            running: Arc::clone(&running),
            packages,
            starter,
            permissions,
            environments,
            engine: Arc::clone(&engine),
        };

        let (stop_prompts, stop_prompts_rx) = oneshot::channel();
        runtime.spawn(hold_prompts(prompted, stop_prompts_rx));

        // Developer access stays off: a headless WaOS is reached from this machine alone.
        let access = DeveloperAccess::at(config.folders.developer_access.clone());
        let server =
            DevServer::start(served, asking, &config.socket, access).with_context(|| {
                format!("listening for dev commands at {}", config.socket.display())
            })?;
        tracing::info!(socket = %config.socket.display(), "headless WaOS listening");

        Ok(HeadlessDaemon {
            runs: running,
            server,
            stop_prompts: Some(stop_prompts),
            _engine: engine,
        })
    }

    fn stop(mut self) {
        if let Some(stop) = self.stop_prompts.take() {
            let _ = stop.send(());
        }
        drop(self.server);
    }
}

/// What a headless WaOS's engine runs over: a screen of `display` with no window system, the
/// pickers put to its user through `prompts`, the system's video decoders and this computer's
/// capabilities; with the questions `prompted` and what its users are asked now kept.
fn headless_outside(
    display: Display,
    before: BeforeTheEngine<'_>,
    prompts: &PromptsOnRequest,
    prompted: PromptRequests,
) -> Outside<(PromptRequests, Arc<AskingNow>)> {
    let asking = AskingNow::new(before.permissions.asking());
    let headless = HeadlessShell::new(display, Refreshes::Every(REFRESH_INTERVAL));
    let mut shell = headless.shell();
    shell.audio_output_picker = Some(Arc::new(prompts.output_picker(Arc::clone(&asking))));
    shell.machine_picker = Some(Arc::new(prompts.machine_picker(Arc::clone(&asking), None)));
    shell.gpus = video_decoder_thread::start();
    let platform = PlatformState {
        screens: vec![headless.screen()],
        platform_brightness: Brightness::Light,
        caption_style: None,
        locales: system_locales(),
        time_zone: os::time_zone(),
    };
    Outside::new(
        shell,
        platform,
        LocalCapabilities::new(),
        (prompted, asking),
    )
}

/// Holds each pending prompt alive until the engine cancels it (for example when `waos settings
/// set` answers the permission or the program stops), so `ask` permissions never spuriously deny
/// in headless mode.
async fn hold_prompts(mut prompted: PromptRequests, mut stop: oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut stop => break,
            asked = prompted.questions.next() => match asked {
                Some(asked) => {
                    tokio::spawn(async move {
                        let _asking = asked.asking;
                        let _ = asked.cancelled.await;
                    });
                }
                None => break,
            },
            opened = prompted.settings.next() => match opened {
                Some(opened) => {
                    // No GUI settings window exists in headless mode; finish immediately.
                    let _ = opened.left.send(());
                }
                None => break,
            },
        }
    }
}

/// Resolves once `idle_timeout` elapses with no running programs and no active dev-server
/// connections; pends forever when `idle_timeout` is `None`.
async fn wait_until_idle(
    runs: &RunningApps,
    mut connections: watch::Receiver<usize>,
    idle_timeout: Option<Duration>,
) {
    let Some(idle_timeout) = idle_timeout else {
        std::future::pending::<()>().await;
        return;
    };
    let mut running_ids = runs.running_ids();
    loop {
        let busy =
            !running_ids.borrow_and_update().is_empty() || *connections.borrow_and_update() > 0;
        if busy {
            tokio::select! {
                changed = running_ids.changed() => if changed.is_err() { return; },
                changed = connections.changed() => if changed.is_err() { return; },
            }
            continue;
        }
        let idle = tokio::time::sleep(idle_timeout);
        tokio::select! {
            () = idle => return,
            changed = running_ids.changed() => if changed.is_err() { return; },
            changed = connections.changed() => if changed.is_err() { return; },
        }
    }
}

/// Waits for Ctrl-C (or SIGTERM on Unix).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                if let Some(ref mut stream) = term {
                    stream.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Whether `WAOS_NATIVE_UNWIND_INFO` asks for native unwind information in compiled programs.
fn native_unwind_info_asked() -> bool {
    std::env::var("WAOS_NATIVE_UNWIND_INFO").is_ok_and(|value| {
        !value.is_empty()
            && !value.eq_ignore_ascii_case("0")
            && !value.eq_ignore_ascii_case("false")
    })
}

/// Best-effort system locales parsed from `LC_ALL` / `LANG`, falling back to `en-US`.
fn system_locales() -> Vec<Locale> {
    for name in ["LC_ALL", "LANG"] {
        if let Ok(raw) = std::env::var(name)
            && let Some(locale) = parse_posix_locale(&raw)
        {
            return vec![locale];
        }
    }
    vec![Locale {
        language: "en".to_owned(),
        script: None,
        region: Some("US".to_owned()),
    }]
}

/// Parses a POSIX locale like `en_US.UTF-8` or `fr-CA` into a `Locale`.
fn parse_posix_locale(raw: &str) -> Option<Locale> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "C" || trimmed == "POSIX" || trimmed.starts_with("C.") {
        return None;
    }
    let without_encoding = trimmed.split(['.', '@']).next()?;
    let mut parts = without_encoding
        .split(['_', '-'])
        .filter(|part| !part.is_empty());
    let language = parts.next()?.to_ascii_lowercase();
    let region = parts.next().map(|part| part.to_ascii_uppercase());
    Some(Locale {
        language,
        script: None,
        region,
    })
}

#[cfg(all(test, any(target_vendor = "apple", target_os = "linux")))]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use tempfile::TempDir;
    use waos_dev_protocol::{
        Answer, ClientConnection, Connection, Hello, Input, InputAnswer, InputDone, Install,
        InstallAnswer, Launch, LaunchAnswer, Request, ResizeAnswer, Role, Row, RunEvent,
        ScreenshotAnswer, SettingsAnswer, StopAnswer, Upload, WindowId, component_digest,
    };
    use waos_package::ProgramId;

    use super::*;

    const WITHIN: Duration = Duration::from_secs(30);
    const INPUT_ID: &str = "dev.waos.examples.input";
    const INSPECTOR_ID: &str = "dev.waos.examples.headlessinspector";

    fn test_folders(root: &Path) -> AppFolders {
        AppFolders {
            temporary_folders: root.join("tmp"),
            package_copies: root.join("packages"),
            store: root.join("store"),
            preferences: root.join("preferences.toml"),
            program_icons: root.join("icons"),
            #[cfg(target_os = "linux")]
            applications: root.join("applications"),
            #[cfg(windows)]
            start_menu: root.join("start-menu"),
            data: root.join("data"),
            developer_access: root.join("developer"),
            #[cfg(target_os = "macos")]
            installed_apps: root.join("installed-apps"),
        }
    }

    async fn connect(socket: &Path) -> ClientConnection {
        let mut conn = Connection::connect(socket).await.expect("connect");
        conn.hello(&Hello::command("headless-test"))
            .await
            .expect("hello");
        conn
    }

    /// The program at `component_path` installed as the program `id`, launched and attached to,
    /// as `waos run` runs it: the attach's connection, and the development id it runs as.
    async fn start_run(
        socket: &Path,
        id: ProgramId,
        component_path: &Path,
    ) -> (ClientConnection, ProgramId) {
        let component = std::fs::read(component_path).expect("read component");
        let mut conn = connect(socket).await;
        conn.send(&Request::Install(Install {
            id: id.clone(),
            kind: waos_package::ProgramKind::App,
            name: id.to_string(),
            version: "0.1.0".parse().unwrap(),
            icon: None,
            category: None,
            file_associations: Vec::new(),
            length: component.len() as u64,
            digest: component_digest(&component),
            compiling_answer: false,
        }))
        .await
        .expect("send install");
        let upload: Upload = conn.receive().await.expect("upload").expect("upload reply");
        if upload == Upload::Send {
            conn.send_component(&component)
                .await
                .expect("send component");
        }
        let InstallAnswer::Installed { id: development } =
            conn.receive().await.expect("install").expect("installed")
        else {
            panic!("installed");
        };
        assert_eq!(development, id.development());
        let mut conn = connect(socket).await;
        conn.send(&Request::Launch(Launch {
            id: development.clone(),
            arguments: Vec::new(),
            variables: Vec::new(),
        }))
        .await
        .expect("send launch");
        loop {
            match conn.receive().await.expect("launch").expect("an answer") {
                LaunchAnswer::Compiling => {}
                LaunchAnswer::Launched => break,
                other => panic!("launched: {other:?}"),
            }
        }
        let mut conn = connect(socket).await;
        conn.send(&Request::Attach {
            id: development.clone(),
        })
        .await
        .expect("send attach");
        (conn, development)
    }

    async fn ask<A: serde::de::DeserializeOwned>(socket: &Path, request: &Request) -> A {
        let mut conn = connect(socket).await;
        conn.send(request).await.expect("send request");
        conn.receive::<A>()
            .await
            .expect("receive answer")
            .expect("answer")
    }

    #[tokio::test]
    async fn headless_daemon_serves_an_install_a_launch_its_attach_screenshot_resize_input_settings_stop_and_idle_exit()
     {
        let temp = TempDir::new().unwrap();
        let socket = temp.path().join("dev.sock");
        let config = HeadlessConfig {
            folders: test_folders(temp.path()),
            socket: socket.clone(),
            idle_timeout: Some(Duration::from_millis(150)),
            display: Display {
                width: 400,
                height: 300,
                scale: 2.0,
                windows: true,
            },
        };
        let daemon =
            HeadlessDaemon::start(config, tokio::runtime::Handle::current()).expect("start daemon");

        // 1. Install, launch and attach to `examples/input` over waos-dev-protocol on the
        //    headless screen.
        let (run_conn, id) = start_run(
            &socket,
            INPUT_ID.parse().unwrap(),
            Path::new(waos_test_programs::INPUT),
        )
        .await;

        // Wait until first screenshot is presented.
        let deadline = Instant::now() + WITHIN;
        let first_png = loop {
            let answer: ScreenshotAnswer = ask(
                &socket,
                &Request::Screenshot {
                    id: id.clone(),
                    window: None,
                },
            )
            .await;
            if let ScreenshotAnswer::Pictures(pictures) = answer
                && let Some(pic) = pictures.first()
                && let Some(png) = &pic.png
            {
                assert_eq!((pic.width, pic.height), (400, 300));
                break png.clone();
            }
            assert!(Instant::now() < deadline, "timed out waiting for picture");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(!first_png.is_empty());

        // 2. Resize the virtual window and verify updated dimensions.
        let resize: ResizeAnswer = ask(
            &socket,
            &Request::Resize {
                id: id.clone(),
                window: Some(WindowId(1)),
                width: 300.0,
                height: 200.0,
            },
        )
        .await;
        assert!(matches!(resize, ResizeAnswer::Done(_)), "{resize:?}");

        let after_resize: ScreenshotAnswer = ask(
            &socket,
            &Request::Screenshot {
                id: id.clone(),
                window: Some(WindowId(1)),
            },
        )
        .await;
        let ScreenshotAnswer::Pictures(pictures) = after_resize else {
            panic!("expected pictures, got {after_resize:?}");
        };
        assert_eq!((pictures[0].width, pictures[0].height), (600, 400));

        // 3. Deliver tap input and confirm it was given to a window in sight.
        let input_answer: InputAnswer = ask(
            &socket,
            &Request::Input {
                id: id.clone(),
                window: Some(WindowId(1)),
                input: Input::Tap { x: 10.0, y: 10.0 },
            },
        )
        .await;
        assert_eq!(
            input_answer,
            InputAnswer::Done(InputDone { in_sight: true })
        );

        // 4. Settings read & set on the running program.
        let settings: SettingsAnswer = ask(&socket, &Request::Settings { id: id.clone() }).await;
        assert!(matches!(settings, SettingsAnswer::Rows(_)), "{settings:?}");

        // 5. Detach, the program running on, then stop it.
        drop(run_conn);
        let stopped: StopAnswer = ask(&socket, &Request::Stop { id: id.clone() }).await;
        assert_eq!(stopped, StopAnswer::Stopped);

        // 6. Verify a pending `ask` prompt waits (is not immediately denied) and resolves via
        //    `Request::Set` (`waos settings set`).
        let (mut ask_conn, inspector_id) = start_run(
            &socket,
            INSPECTOR_ID.parse().unwrap(),
            Path::new(waos_test_programs::IMPORTS_INSPECT),
        )
        .await;

        let deadline = Instant::now() + WITHIN;
        loop {
            let event =
                tokio::time::timeout(Duration::from_secs(5), ask_conn.receive::<RunEvent>())
                    .await
                    .expect("waiting for prompt event")
                    .expect("stream ok")
                    .expect("event");
            if matches!(event, RunEvent::Asking(_)) {
                break;
            }
            assert!(
                !matches!(event, RunEvent::Ended(_)),
                "program ended before prompt was answered: {event:?}"
            );
            assert!(Instant::now() < deadline, "never received Asked event");
        }

        // Answer the pending inspector role prompt via `Request::Set`.
        let set_answer: SettingsAnswer = ask(
            &socket,
            &Request::Set {
                id: inspector_id.clone(),
                row: Row::Role(Role::Inspector, Some(Answer::Allow)),
            },
        )
        .await;
        assert!(
            matches!(set_answer, SettingsAnswer::Rows(_)),
            "{set_answer:?}"
        );

        // The program should now finish starting and end cleanly (without hanging).
        let deadline = Instant::now() + WITHIN;
        loop {
            let event =
                tokio::time::timeout(Duration::from_secs(5), ask_conn.receive::<RunEvent>())
                    .await
                    .expect("waiting for end")
                    .expect("stream ok");
            match event {
                Some(RunEvent::Ended(_)) | None => break,
                Some(_) => assert!(Instant::now() < deadline, "never ended"),
            }
        }
        drop(ask_conn);

        // 7. Idle timeout triggers once all runs and connections are gone.
        let idle = tokio::time::timeout(
            Duration::from_secs(5),
            wait_until_idle(
                &daemon.runs,
                daemon.server.active_connections(),
                Some(Duration::from_millis(150)),
            ),
        )
        .await;
        assert!(idle.is_ok(), "idle timeout should fire once quiet");

        daemon.stop();
        assert!(!socket.exists(), "socket file should be cleaned up on stop");
    }

    /// A headless WaOS has the system's video decoders, which need no window: a program reads
    /// an H.264 stream, hands it to the decoder picture by picture and gets each frame back in
    /// the order it is shown, its planes read on the GPU and in memory, as in the WaOS app.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_program_decodes_a_stream_with_the_systems_decoder() {
        const VIDEO_DECODING: &str = "dev.waos.examples.videodecoding";
        /// Twelve pictures of 320 by 240, and when each is shown, in the order they are decoded.
        const CLIP: &[u8] = include_bytes!("../../../examples/video-decoding/clip.h264");
        const SHOWN: &str = "0\n3\n1\n2\n5\n4\n8\n6\n7\n11\n9\n10\n";

        let temp = TempDir::new().unwrap();
        let socket = temp.path().join("dev.sock");
        let folders = test_folders(temp.path());
        let id: ProgramId = VIDEO_DECODING.parse().unwrap();
        let data = waos_services::app_storage::AppStorage::new(&folders.store)
            .folders(&id.development())
            .expect("folders")
            .data;
        std::fs::write(data.join("clip.h264"), CLIP).unwrap();
        std::fs::write(data.join("shown"), SHOWN).unwrap();
        let config = HeadlessConfig {
            folders,
            socket: socket.clone(),
            idle_timeout: None,
            display: DISPLAY,
        };
        let daemon =
            HeadlessDaemon::start(config, tokio::runtime::Handle::current()).expect("start daemon");

        let (mut run, _id) =
            start_run(&socket, id, Path::new(waos_test_programs::VIDEO_DECODING)).await;
        let mut written = Vec::new();
        loop {
            let event = tokio::time::timeout(WITHIN, run.receive::<RunEvent>())
                .await
                .expect("the program ends in time")
                .expect("stream ok");
            match event {
                Some(RunEvent::Output { bytes, .. }) => written.extend(bytes),
                Some(RunEvent::Ended(_)) | None => break,
                Some(_) => {}
            }
        }
        let written = String::from_utf8(written).expect("text");
        let mut said = vec![
            "decoding 12 pictures".to_owned(),
            "picture 0: 320x240 in 2 planes".to_owned(),
            // The clip's first pixel is black, which over video's range is these.
            "its first samples: PlaneContent::Luma [16], PlaneContent::Chroma [128, 128]"
                .to_owned(),
            "and in memory: PlaneContent::Luma [16], PlaneContent::Chroma [128, 128]".to_owned(),
        ];
        said.extend((1..12).map(|shown| format!("picture {shown}: 320x240 in 2 planes")));
        assert_eq!(written.lines().collect::<Vec<_>>(), said);

        daemon.stop();
    }
}
