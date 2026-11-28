//! A program that looks around, and what a reader learns the API from: it prints what the user
//! set, its time zone with the time on the user's clock among it, and the screens, and each change
//! of what the user sets; asks for a view, with no title bar of macOS's own; keeps each view's window between a smallest and a largest size where its user
//! resizes it; treats a band at the top of each view as its title bar, as a program that draws its
//! own does, a press there moving the window, a double-click doing what the system's settings
//! say, a press of the secondary button taking the window full screen or back and one of the
//! middle button asking to bring the window to the front; follows each
//! view's events, closing its window when the user asks; says each change of its window, maximized
//! or full screen or no longer; logs each change of its own state; answers each request to
//! end with `exit`, having nothing to save; and logs every change among its views until the last is
//! gone.
//!
//! ```text
//! cargo build -p survey --target wasm32-wasip3
//! ```

use wasdk::lifecycle::{self, AppExitResponse, ExitRequest, LifecycleState};
use wasdk::screens::{self, Screen};
use wasdk::system::{self, Locale, SystemChange};
use wasdk::views::{
    self, PointerButtons, PointerChange, PointerEvent, RequestFailure, View, ViewChange, ViewEvent,
    ViewId,
};
use wasdk::windows::{
    MacosTitleBar, Rect, Size, TitleBarAction, Window, WindowChange, WindowConfig,
};
use wasdk::{StreamReader, system_clock};

/// The smallest size survey's windows may be resized to.
const MIN_SIZE: Size = Size {
    width: 320.0,
    height: 240.0,
};

/// The largest size survey's windows may be resized to.
const MAX_SIZE: Size = Size {
    width: 1920.0,
    height: 1200.0,
};

/// How tall the band at the top of each view is that survey treats as its title bar, in logical
/// points; on macOS the window's buttons sit in it.
const TITLE_BAR_HEIGHT: f64 = 28.0;

/// The longest time from one press on the title bar to the next that makes a double-click, in
/// nanoseconds: Flutter's `kDoubleTapTimeout`.
const DOUBLE_CLICK_TIME: u64 = 300_000_000;

#[wasdk::main]
async fn main() -> Result<(), String> {
    // Each stream is opened here, before what it changes is read, rather than in its task, which
    // first runs at the first await: a change between the two is not missed.
    let system_changes = system::changes();
    let state_changes = lifecycle::state_changes();
    let exit_requests = lifecycle::exit_requests();
    describe_system();
    for screen in screens::screens() {
        describe_screen(&screen);
    }
    println!("program: {:?}", lifecycle::state());
    wasdk::spawn(log_system_changes(system_changes));
    wasdk::spawn(log_state_changes(state_changes));
    wasdk::spawn(answer_exit_requests(exit_requests));

    // The stream is opened before the request, so the view that arrives for it is not missed.
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("Survey");
    config.set_macos_title_bar(MacosTitleBar::Hidden(TITLE_BAR_HEIGHT));
    let request = views::request_view(&config, Some("opened at start"))
        .map_err(|error| format!("no view for this program: {error}"))?;
    println!("asked for a view: request {request}");

    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(view) => follow(view),
            ViewChange::Removed(id) => {
                println!("view {id} removed");
                if views::views().is_empty() {
                    break;
                }
            }
            ViewChange::RequestFailed(RequestFailure { request, error }) => {
                return Err(format!("request {request} failed: {error}"));
            }
        }
    }
    Ok(())
}

async fn log_system_changes(mut changes: StreamReader<SystemChange>) {
    while let Some(change) = changes.next().await {
        match change {
            SystemChange::TimeZone(name) => println!("system: time zone now {name:?}"),
            other => println!("system: {other:?}"),
        }
    }
}

async fn log_state_changes(mut states: StreamReader<LifecycleState>) {
    while let Some(state) = states.next().await {
        println!("program: {state:?}");
    }
}

/// Answers each request to end with `exit`, after saying so: a program with nothing to save.
async fn answer_exit_requests(mut requests: StreamReader<ExitRequest>) {
    while let Some(request) = requests.next().await {
        println!("program: asked whether it may end: yes");
        ExitRequest::respond(request, AppExitResponse::Exit);
    }
}

/// Opens `view`'s events, describes it, limits its window, then logs what happens to it on a task
/// of its own until it is gone.
fn follow(view: View) {
    // Opened before anything is logged or spawned, as `main` opens its own, so a close request or
    // freeze right after the view arrives already sees a reader on it.
    let events = view.events();
    describe_view(&view);
    limit_window_size(&view);
    if let Some(window) = view.window() {
        wasdk::spawn(log_window_changes(view.id(), window.changes()));
    }
    if let Some(events) = events {
        wasdk::spawn(log_events(view, events));
    }
}

/// Keeps the window of `view` between `MIN_SIZE` and `MAX_SIZE`, where its user resizes it.
fn limit_window_size(view: &View) {
    let Some(limits) = view.window().and_then(|window| window.size_limits()) else {
        return;
    };
    limits.set_min_size(Some(MIN_SIZE));
    limits.set_max_size(Some(MAX_SIZE));
    println!(
        "view {}: its window keeps between {}x{} and {}x{}",
        view.id(),
        MIN_SIZE.width,
        MIN_SIZE.height,
        MAX_SIZE.width,
        MAX_SIZE.height
    );
}

async fn log_events(view: View, mut events: StreamReader<ViewEvent>) {
    let id = view.id();
    let mut title_bar = TitleBar::default();
    while let Some(event) = events.next().await {
        match event {
            ViewEvent::Metrics(metrics) => {
                println!("view {id}: now {}x{}", metrics.width, metrics.height);
            }
            ViewEvent::Pointer(event) => title_bar.pointer(&view, &event),
            ViewEvent::Focus(focus) => println!("view {id}: {:?}", focus.state),
            ViewEvent::CloseRequested => {
                println!("view {id}: asked to close");
                if let Some(window) = view.window() {
                    window.close();
                }
            }
            _ => {}
        }
    }
    println!("view {id}: events ended");
}

/// Says each change of the window of view `id`, whoever made it: the program, or the user by the
/// system's own means.
async fn log_window_changes(id: ViewId, mut changes: StreamReader<WindowChange>) {
    while let Some(change) = changes.next().await {
        let now = match change {
            WindowChange::Maximized(true) => "maximized",
            WindowChange::Maximized(false) => "no longer maximized",
            WindowChange::Fullscreen(true) => "full screen",
            WindowChange::Fullscreen(false) => "no longer full screen",
        };
        println!("view {id}: its window is {now}");
    }
}

/// The band at the top of a view that survey treats as its title bar: a press there moves the
/// window, a second press soon after does what a double-click on a title bar does, and a press
/// of the secondary button takes the window full screen, or back, and one of the middle button
/// asks to bring the window to the front.
#[derive(Default)]
struct TitleBar {
    /// When the last press on it went down, on the program's clock; none once it made a
    /// double-click.
    last_press: Option<u64>,
}

impl TitleBar {
    fn pointer(&mut self, view: &View, event: &PointerEvent) {
        let height = TITLE_BAR_HEIGHT * view.metrics().device_pixel_ratio;
        if !matches!(event.change, PointerChange::Down) || event.y >= height {
            return;
        }
        let Some(window) = view.window() else {
            return;
        };
        if event.buttons.contains(PointerButtons::SECONDARY) {
            let fullscreen = !window.is_fullscreen();
            println!(
                "view {}: secondary press on its title bar: full screen {fullscreen}",
                view.id()
            );
            window.set_fullscreen(fullscreen, None);
            return;
        }
        if event.buttons.contains(PointerButtons::TERTIARY) {
            println!(
                "view {}: middle press on its title bar: asking to come to the front",
                view.id()
            );
            window.activate();
            return;
        }
        let soon_after = self
            .last_press
            .is_some_and(|last| event.time.saturating_sub(last) <= DOUBLE_CLICK_TIME);
        if soon_after {
            self.last_press = None;
            double_click(view.id(), &window);
        } else {
            self.last_press = Some(event.time);
            println!(
                "view {}: pressed on its title bar: moving its window",
                view.id()
            );
            window.drag();
        }
    }
}

/// Does to `window` what the system's settings say a double-click on a title bar does.
fn double_click(id: ViewId, window: &Window) {
    let action = window.title_bar_double_click_action();
    println!("view {id}: double-clicked its title bar: {action:?}");
    match action {
        Some(TitleBarAction::Maximize) => window.set_maximized(!window.is_maximized()),
        Some(TitleBarAction::Minimize) => window.set_minimized(true),
        Some(TitleBarAction::Nothing) | None => {}
    }
}

fn describe_system() {
    let languages: Vec<String> = system::locales().iter().map(language_tag).collect();
    println!(
        "system: {:?}, {:?}, languages {}",
        system::os(),
        system::platform_brightness(),
        languages.join(", ")
    );
    println!(
        "system: time zone {:?}, {}",
        system::time_zone(),
        local_time()
    );
}

/// The time on the user's clock now, `hh:mm UTC+h`: the system clock's instant moved by the time
/// zone's offset from UTC at it.
fn local_time() -> String {
    let now = system_clock::now();
    let Some(offset) = system::utc_offset(now) else {
        return "no offset from UTC".to_owned();
    };
    let local_seconds = now.seconds + offset / 1_000_000_000;
    let minutes_of_day = local_seconds.rem_euclid(86_400) / 60;
    format!(
        "{:02}:{:02} UTC{:+}h",
        minutes_of_day / 60,
        minutes_of_day % 60,
        offset as f64 / 3.6e12
    )
}

/// The locale's BCP 47 tag: `zh-Hant-TW`.
fn language_tag(locale: &Locale) -> String {
    [
        Some(&locale.language),
        locale.script.as_ref(),
        locale.region.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(String::as_str)
    .collect::<Vec<_>>()
    .join("-")
}

fn describe_screen(screen: &Screen) {
    let size = screen.size();
    println!(
        "screen {}: {:?}, {}x{} at {}x, {} mHz, frame {}, visible frame {}",
        screen.id(),
        screen.name(),
        size.width,
        size.height,
        screen.scale(),
        screen.refresh_rate(),
        describe_rect(screen.frame()),
        describe_rect(screen.visible_frame())
    );
}

/// `rect` as its origin and size: `(0, 25) 1512x957`.
fn describe_rect(rect: Rect) -> String {
    format!(
        "({}, {}) {}x{}",
        rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
    )
}

fn describe_view(view: &View) {
    let metrics = view.metrics();
    println!(
        "view {}: {}x{} at {}x, on screen {:?}, in a window: {}, for request {} with {:?}",
        view.id(),
        metrics.width,
        metrics.height,
        metrics.device_pixel_ratio,
        view.screen().map(|screen| screen.id()),
        view.window().is_some(),
        view.request_id(),
        view.initial_data()
    );
}
