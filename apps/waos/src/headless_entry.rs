//! Headless WaOS daemon: runs the engine, local capabilities, store, dev server, and a screen with
//! no display (`waos_headless`) with no `winit`, Wayland, X11, or `waft-single-instance`. Used on headless Linux
//! servers/CI and whenever `waos --headless` is invoked.
//!
//! The invariant: every `ask` permission prompt stays open through `PromptsOnRequest` + `AskingNow`
//! until answered via `waos settings set` (or the program ends), identically to desktop WaOS.
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
use waos::inspection::InspectedPrograms;
use waos::{
    EngineTime, FreezePolicy, HardwareBufferMode, HttpProxy, PlatformState, WaOS, WaOSConfig,
};
use waos_headless::{Display, HeadlessShell, Refreshes};
use waos_local::LocalCapabilities;
use waos_store::{SourceProxy, Store};

use crate::logic::asking_now::AskingNow;
use crate::logic::dev_server::DevServer;
use crate::logic::environments::ProgramEnvironments;
use crate::logic::folders::{self, AppFolders};
use crate::logic::held_starts::HeldStarts;
use crate::logic::inspected_programs::ProgramsOnThisComputer;
use crate::logic::installed_runs::InstalledRuns;
use crate::logic::program_places::{ProgramPlace, ProgramPlaces};
use crate::logic::prompts::{PromptRequests, PromptsOnRequest};
use crate::logic::runs::Runs;
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
    runs: Arc<Runs>,
    server: DevServer,
    stop_prompts: Option<oneshot::Sender<()>>,
    _engine: Arc<WaOS>,
}

impl HeadlessDaemon {
    fn start(config: HeadlessConfig, runtime: tokio::runtime::Handle) -> Result<HeadlessDaemon> {
        let store = Store::open(&config.folders.store, SourceProxy::System)
            .with_context(|| format!("opening the store at {}", config.folders.store.display()))?;
        let asking = AskingNow::new();
        let (prompts, prompted) = PromptsOnRequest::new(Arc::clone(&asking));
        let output_picker = prompts.output_picker();
        let machine_picker = prompts.machine_picker();
        let headless = HeadlessShell::new(config.display, Refreshes::Every(REFRESH_INTERVAL));
        let mut shell = headless.shell(Arc::new(prompts));
        shell.audio_output_picker = Some(Arc::new(output_picker));
        shell.machine_picker = Some(Arc::new(machine_picker));

        let runs = Arc::new(Runs::default());
        let inspected = Arc::new(ProgramsOnThisComputer::new(
            store.clone(),
            Arc::clone(&runs),
        ));
        let waos_config = WaOSConfig {
            code_cache: config.folders.code_cache,
            freeze: FreezePolicy::default(),
            native_unwind_info: native_unwind_info_asked(),
            http_proxy: HttpProxy::System,
            time: EngineTime::Real,
            temporary_folders: config.folders.temporary_folders,
            audit: Some(Arc::new(store.clone())),
            inspected_programs: Some(Arc::clone(&inspected) as Arc<dyn InspectedPrograms>),
            hardware_buffers: HardwareBufferMode::Offscreen,
        };
        let platform = PlatformState {
            screens: vec![headless.screen()],
            platform_brightness: Brightness::Light,
            caption_style: None,
            locales: system_locales(),
            time_zone: os::time_zone(),
        };
        let engine =
            Arc::new(WaOS::new(waos_config, shell, platform).context("starting the engine")?);

        let capabilities = LocalCapabilities::new();
        let environments = ProgramEnvironments::new(capabilities, store.clone());
        let places = ProgramPlaces::new(ProgramPlace::Windows);
        let installed =
            InstalledRuns::new(Arc::clone(&engine), environments, Arc::clone(&runs), places);
        let starts = HeldStarts {
            installed: installed.clone(),
            store: store.clone(),
        };
        inspected.serve_starts(starts, runtime.clone());

        let (stop_prompts, stop_prompts_rx) = oneshot::channel();
        runtime.spawn(hold_prompts(prompted, stop_prompts_rx));

        let server =
            DevServer::start(installed, store, asking, &config.socket).with_context(|| {
                format!("listening for dev commands at {}", config.socket.display())
            })?;
        tracing::info!(socket = %config.socket.display(), "headless WaOS listening");

        Ok(HeadlessDaemon {
            runs,
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
    runs: &Runs,
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
        tokio::select! {
            _ = tokio::time::sleep(idle_timeout) => return,
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
        Answer, ClientStream, Connection, Hello, Input, InputAnswer, InputDone, Install,
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
            code_cache: PathBuf::from(waos_test_programs::CODE_CACHE),
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
            #[cfg(target_os = "macos")]
            installed_apps: root.join("installed-apps"),
        }
    }

    async fn connect(socket: &Path) -> Connection<ClientStream> {
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
    ) -> (Connection<ClientStream>, ProgramId) {
        let component = std::fs::read(component_path).expect("read component");
        let mut conn = connect(socket).await;
        conn.send(&Request::Install(Install {
            id: id.clone(),
            kind: waos_package::ProgramKind::App,
            name: id.to_string(),
            version: "0.1.0".parse().unwrap(),
            icon: None,
            length: component.len() as u64,
            digest: component_digest(&component),
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
}
