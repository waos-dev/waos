//! The window process: one program's windows, an Waft app of its own. It shows
//! `waos-program-window`'s windows for the program, linked to the program through WaOS
//! (`program_link`), a window that takes the focus bringing the app forward as it opens
//! (`os::come_forward`), the program's menu bar and its context menus (`program_menus`), its file
//! dialogs (`program_file_dialogs`), its notifications (`program_notifications`), and WaOS's
//! prompts for the program in dialogs of their own (`prompt_dialogs`); finds WaOS (`finding_waos`)
//! on a thread of its own; says hello with which program it is, why it was launched, a click on
//! one of its notifications among the reasons, and what the system opened it with so far; and once
//! WaOS welcomes it, carries out on the main thread what WaOS sends (`connection`), until the
//! program ends or WaOS goes, when it quits.
//!
//! The invariant: the main thread waits on WaOS only in a window's resize, for the program's
//! picture at the new size (`resize_waits`). Finding WaOS and receiving every message happen on
//! the connection's thread, which records what the wait hangs on, each picture WaOS says is shown,
//! each request the program awaits of the app, and the connection's end, where that wait is woken,
//! and hands everything but the pictures to the main thread as it arrives.

use std::cell::RefCell;
use std::ffi::c_int;
use std::io;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing_subscriber::EnvFilter;
use waft::ui::{OpenRequest as SystemOpenRequest, PickedFile, PlatformRef};
use waft::{
    App, DefaultEmbedder, Entity, IntoWidget, MultiViewApp, Shell, SizedBox, WidgetRef, run_widget,
};
use waos::OpenRequest;
use waos_program_window::{ProgramFace, ProgramLink, ProgramWindows, ViewWindows, WindowRequest};
use waos_window_messages::bundle::LAUNCHED_BY_WAOS;
use waos_window_messages::os::macos::{Arrival, ReceiveRight, SendRight};
use waos_window_messages::{FromWindow, Hello, PROTOCOL, ToWindow, bootstrap_name, decode, encode};

use crate::connection::{Carried, Connection};
use crate::finding_waos::{Launcher, find_or_start_waos};
use crate::os::{BundleInfo, check_layer_hosts, come_forward};
use crate::program_file_dialogs::{OpenFileDialogs, open_file_dialog};
use crate::program_link::WaosProgram;
use crate::program_menus::{open_context_menu, send_menu_choices, show_menu_bar};
use crate::program_notifications::{NotificationClicks, ProgramNotifications};
use crate::program_quit::QuitsAsked;
use crate::prompt_dialogs::PromptDialogs;
use crate::resize_waits::ResizeWaits;

/// What the connection's thread hands the main thread.
enum Event {
    /// WaOS answers at this port: the hello can be sent.
    Found(SendRight),
    /// WaOS sent this, with these ports.
    Received(ToWindow, Vec<SendRight>),
    /// The connection is over, or never was, and why.
    Gone(String),
}

/// Runs the window process of the bundle it was loaded in until it ends; answers its exit status.
/// A system that cannot host WaOS's layers in a window has no window process: it ends at once.
pub(crate) fn run() -> c_int {
    write_traces();
    let bundle = match BundleInfo::read() {
        Ok(bundle) => bundle,
        Err(reason) => {
            eprintln!("waos-window: {reason}");
            return 1;
        }
    };
    if let Err(lacking) = check_layer_hosts() {
        tracing::error!(lacking, "a program's window cannot show what it draws");
        return 1;
    }
    let launcher = if std::env::args().any(|argument| argument == LAUNCHED_BY_WAOS) {
        Launcher::Waos
    } else {
        Launcher::User
    };
    tracing::info!(program = %bundle.program, ?launcher, "the program's app starts");
    DefaultEmbedder::default()
        .run(move |platform| Shell::new(platform, move |app| start(app, bundle, launcher)));
    0
}

/// Shows the program's windows, none until WaOS asks, and connects to WaOS; a window process that
/// cannot make a port of its own quits.
fn start(app: &mut App, bundle: BundleInfo, launcher: Launcher) {
    let (requests, requested) = mpsc::unbounded();
    let view_windows = ViewWindows::<WaosProgram>::listen(app, requested);
    let dialogs = PromptDialogs::new(app, view_windows.clone());
    let windows = ProgramWindows {
        view_windows: view_windows.clone(),
        over_each: Rc::new(|_app, _program| SizedBox::shrink().into_widget()),
    };
    run_widget(app, every_window(windows, dialogs.clone()));
    let opened = Rc::new(RefCell::new(Opened::Collecting(Vec::new())));
    take_open_requests(app, Rc::clone(&opened));
    let clicks = NotificationClicks::listen(app);
    let waits = Arc::new(ResizeWaits::default());
    let (own_port, events) = match connect(&bundle, launcher, Arc::clone(&waits)) {
        Ok(connecting) => connecting,
        Err(error) => {
            tracing::error!(%error, "the program's app has no port of its own");
            quit(app);
            return;
        }
    };
    let process = WindowProcess {
        bundle,
        launcher,
        own_port: Some(own_port),
        waits,
        requests,
        view_windows,
        dialogs,
        file_dialogs: OpenFileDialogs::default(),
        opened,
        clicks,
        connection: None,
        notifications: None,
        quits: QuitsAsked::default(),
        platform: app.platform(),
    };
    process.carry_out(app, events);
}

/// Every window of the process, built from what its view is: a window of the program's, or a
/// dialog of a prompt for it. A user's request to close one goes to what it shows: the program's
/// to the program, which decides, and a dialog's dismissing its prompt.
fn every_window(windows: ProgramWindows<WaosProgram>, dialogs: Entity<PromptDialogs>) -> WidgetRef {
    let (built, closing) = (windows.clone(), windows);
    let (in_dialogs, closing_dialogs) = (dialogs.clone(), dialogs);
    MultiViewApp::new(move |app, view| {
        built
            .content_of(app, view)
            .or_else(|| PromptDialogs::content_of(&in_dialogs, app, view))
    })
    .on_close_requested(Rc::new(move |app, view| {
        if !closing.close_requested(app, view.id()) {
            closing_dialogs.update(app, |dialogs, cx| dialogs.close_requested(view.id(), cx));
        }
    }))
    .into_widget()
}

/// A port of the window process's own, which the connection's thread receives on, and a send
/// right to it for the hello; the thread finds WaOS, then hands on what arrives, recording in
/// `waits` what a window's wait in its resize hangs on.
fn connect(
    bundle: &BundleInfo,
    launcher: Launcher,
    waits: Arc<ResizeWaits>,
) -> io::Result<(SendRight, UnboundedReceiver<Event>)> {
    let port = ReceiveRight::new()?;
    let own_port = port.make_send()?;
    port.notify_when_no_senders()?;
    let (events, arriving) = mpsc::unbounded();
    let name = bootstrap_name(&bundle.data_folder);
    let waos = bundle.waos.clone();
    thread::Builder::new()
        .name("waos-connection".to_owned())
        .spawn(move || match find_or_start_waos(&name, &waos, launcher) {
            Ok(found) => {
                let _ = events.unbounded_send(Event::Found(found));
                receive(&port, &events, &waits);
            }
            Err(error) => {
                waits.end();
                let _ = events.unbounded_send(Event::Gone(format!("no WaOS: {error}")));
            }
        })?;
    Ok((own_port, arriving))
}

/// Hands on every message WaOS sends on `port`, until WaOS is gone, once `waits` has heard it, but
/// each picture WaOS says a view shows, which goes to `waits` alone.
fn receive(port: &ReceiveRight, events: &UnboundedSender<Event>, waits: &ResizeWaits) {
    loop {
        let event = event_of(port.receive(PROTOCOL));
        if recorded_in(waits, &event) {
            continue;
        }
        let gone = matches!(event, Event::Gone(_));
        if events.unbounded_send(event).is_err() || gone {
            return;
        }
    }
}

/// What the main thread is handed of what arrived.
fn event_of(arrival: io::Result<Arrival>) -> Event {
    match arrival {
        Ok(Arrival::Message(received)) => match decode::<ToWindow>(&received.bytes) {
            Ok(message) => Event::Received(message, received.ports),
            Err(error) => Event::Gone(format!("WaOS sent what this build cannot read: {error}")),
        },
        Ok(Arrival::NoSenders) => Event::Gone("WaOS is gone".to_owned()),
        Ok(Arrival::OtherProtocol { protocol, .. }) => Event::Gone(format!(
            "WaOS speaks the window protocol {protocol} and this app {PROTOCOL}: its app is written again at its next launch through WaOS"
        )),
        Err(error) => Event::Gone(format!("receiving from WaOS: {error}")),
    }
}

/// Records in `waits` what `event` says of what a window waiting in its resize on the main thread
/// hangs on, which wakes it: a picture WaOS says a view shows, which nothing else needs, and
/// answers so; a request the program awaits of the app, or one settled; or the end of the program
/// or of the connection. The main thread is handed all but the pictures.
fn recorded_in(waits: &ResizeWaits, event: &Event) -> bool {
    match event {
        Event::Received(ToWindow::PictureShown { window, size }, _) => {
            waits.shown(*window, *size);
            true
        }
        Event::Received(ToWindow::ProgramEnded, _) | Event::Gone(_) => {
            waits.end();
            false
        }
        Event::Received(message, _) => {
            waits.heard(message);
            false
        }
        Event::Found(_) => false,
    }
}

/// What the system opened the program's app with: collected until WaOS welcomes the window
/// process, the hello carrying those before it, and sent to WaOS as they come from then on.
enum Opened {
    Collecting(Vec<SystemOpenRequest>),
    Connected(Rc<Connection>),
}

/// From now on, what the system opens the app with goes to `opened`.
fn take_open_requests(app: &mut App, opened: Rc<RefCell<Opened>>) {
    app.set_open_requested_handler(Some(Rc::new(move |_app, request| {
        match &mut *opened.borrow_mut() {
            Opened::Collecting(requests) => requests.push(request),
            Opened::Connected(connection) => {
                connection.send(FromWindow::Opened(open_request_of(vec![request])));
            }
        }
    })));
}

/// The window process on the main thread, between WaOS and its windows.
struct WindowProcess {
    bundle: BundleInfo,
    launcher: Launcher,
    /// The send right to the window process's own port, until the hello carries it.
    own_port: Option<SendRight>,
    /// What a window's wait in its resize hangs on, as the connection's thread records it.
    waits: Arc<ResizeWaits>,
    requests: UnboundedSender<WindowRequest<WaosProgram>>,
    /// The program's windows, at whose views its context menus open and on which its file
    /// dialogs are sheets.
    view_windows: Entity<ViewWindows<WaosProgram>>,
    /// The dialogs of WaOS's prompts for the program.
    dialogs: Entity<PromptDialogs>,
    /// The file dialogs open in the app.
    file_dialogs: OpenFileDialogs,
    opened: Rc<RefCell<Opened>>,
    /// The clicks on the program's notifications, the one that launched the app among them.
    clicks: NotificationClicks,
    connection: Option<Rc<Connection>>,
    /// The program's notifications, once WaOS welcomed the app.
    notifications: Option<ProgramNotifications>,
    /// The quits the user asked of the app, waiting for the program's answer.
    quits: QuitsAsked,
    /// Waft's platform, whose macOS host shows WaOS's layers in the windows.
    platform: PlatformRef,
}

impl WindowProcess {
    /// Carries out every event of the connection's thread, in order, on the main thread.
    fn carry_out(mut self, app: &mut App, mut events: UnboundedReceiver<Event>) {
        app.spawn(async move |cx| {
            while let Some(event) = events.next().await {
                if !cx.update(|app| self.event(app, event)) {
                    break;
                }
            }
        });
    }

    /// Carries out `event`; answers whether the window process goes on.
    fn event(&mut self, app: &mut App, event: Event) -> bool {
        match event {
            Event::Found(waos) => self.say_hello(&waos),
            Event::Received(ToWindow::Welcome(welcome), mut ports) if self.connection.is_none() => {
                let Some(to_waos) = ports.pop() else {
                    return self.end(app, "WaOS's welcome brought no port");
                };
                let waits = Arc::clone(&self.waits);
                self.welcomed(app, Rc::new(Connection::welcomed(welcome, to_waos, waits)));
            }
            Event::Received(ToWindow::Refused(reason), _) => {
                return self.end(app, &format!("WaOS refused it: {reason}"));
            }
            Event::Received(message, _) => return self.received(app, message),
            Event::Gone(reason) => return self.end(app, &reason),
        }
        true
    }

    /// Says which program this is, why it was launched, the user's click on one of its
    /// notifications among the reasons, and what the system opened it with so far, with the port
    /// WaOS answers on.
    fn say_hello(&mut self, waos: &SendRight) {
        let Some(own_port) = self.own_port.take() else {
            return;
        };
        let hello = Hello {
            program: self.bundle.program.clone(),
            launched_for: self.clicks.launch_reason(self.launcher),
            opened: self.take_opened(),
        };
        tracing::info!(?hello, "saying hello to WaOS");
        let sent = waos.send(PROTOCOL, &encode(&FromWindow::Hello(hello)), vec![own_port]);
        if let Err(error) = sent {
            tracing::warn!(%error, "WaOS did not take the hello");
        }
    }

    /// WaOS took the window process: what the system opened it with since the hello goes to
    /// WaOS, and so does all that follows, the choices in the program's menu bar, the clicks on
    /// its notifications and the user's quitting the app among it.
    fn welcomed(&mut self, app: &mut App, connection: Rc<Connection>) {
        tracing::info!(name = connection.name(), "WaOS welcomed the program's app");
        send_menu_choices(app, Rc::clone(&connection));
        self.quits.ask_through(app, Rc::clone(&connection));
        self.clicks.connected(Rc::clone(&connection));
        self.notifications = Some(ProgramNotifications::serve(app, Rc::clone(&connection)));
        let since_hello = self.take_opened();
        if !(since_hello.files.is_empty() && since_hello.links.is_empty()) {
            connection.send(FromWindow::Opened(since_hello));
        }
        *self.opened.borrow_mut() = Opened::Connected(Rc::clone(&connection));
        self.connection = Some(connection);
    }

    /// Carries out what WaOS sent, a window the program asks for, its menus, its file dialogs, its
    /// notifications, a prompt's dialog and the program's end among it; answers whether the window
    /// process goes on.
    fn received(&mut self, app: &mut App, message: ToWindow) -> bool {
        let Some(connection) = &self.connection else {
            tracing::warn!(?message, "WaOS spoke before it welcomed the program's app");
            return true;
        };
        match connection.received(message) {
            Carried::Done => true,
            Carried::WindowAsked(request) => {
                if request.config.visible && request.config.activating {
                    come_forward();
                }
                let program = WaosProgram::new(Rc::clone(connection), Rc::clone(&self.platform));
                // macOS shows the app's icon for every window, and no window's own; the app is the
                // program's identity.
                let face = ProgramFace {
                    name: program.name().to_owned(),
                    icon: None,
                    identity: None,
                };
                let _ = self.requests.unbounded_send(WindowRequest {
                    program,
                    request,
                    face,
                });
                true
            }
            Carried::PromptAsked {
                prompt,
                size,
                layers,
            } => {
                let connection = Rc::clone(connection);
                self.dialogs.update(app, |dialogs, cx| {
                    dialogs.show(prompt, size, layers, connection, cx);
                });
                true
            }
            Carried::PromptCalled { prompt, call } => {
                self.dialogs
                    .update(app, |dialogs, _| dialogs.call(prompt, call));
                true
            }
            Carried::PromptHidden(prompt) => {
                self.dialogs
                    .update(app, |dialogs, cx| dialogs.hide(prompt, cx));
                true
            }
            Carried::MenuBarSet(entries) => {
                show_menu_bar(app, &entries);
                true
            }
            Carried::ContextMenuAsked(asked) => {
                open_context_menu(app, &self.view_windows, Rc::clone(connection), asked);
                true
            }
            Carried::QuitAnswered(response) => {
                self.quits.answered(response);
                true
            }
            Carried::FileDialogAsked(asked) => {
                open_file_dialog(
                    app,
                    &self.view_windows,
                    &self.file_dialogs,
                    Rc::clone(connection),
                    asked,
                );
                true
            }
            Carried::CloseFileDialog(dialog) => {
                self.file_dialogs.close(dialog);
                true
            }
            Carried::NotificationAsked(asked) => {
                if let Some(notifications) = &self.notifications {
                    notifications.post(asked);
                }
                true
            }
            Carried::NotificationRemovalAsked(id) => {
                if let Some(notifications) = &self.notifications {
                    notifications.remove(id);
                }
                true
            }
            Carried::NotificationPermissionAsked(request) => {
                if let Some(notifications) = &self.notifications {
                    notifications.request_permission(request);
                }
                true
            }
            Carried::NotificationPermissionReadingAsked => {
                if let Some(notifications) = &self.notifications {
                    notifications.tell_permission();
                }
                true
            }
            Carried::ProgramEnded => self.end(app, "the program ended"),
        }
    }

    /// The program's app ends, for `reason`: its windows close with the connection, and it quits.
    fn end(&mut self, app: &mut App, reason: &str) -> bool {
        tracing::info!(reason, "the program's app ends");
        if let Some(connection) = &self.connection {
            connection.end();
        }
        self.quits.app_ends();
        quit(app);
        false
    }

    /// The requests the system opened the app with that are still collected, as WaOS takes them.
    fn take_opened(&self) -> OpenRequest {
        match &mut *self.opened.borrow_mut() {
            Opened::Collecting(requests) => open_request_of(std::mem::take(requests)),
            Opened::Connected(_) => OpenRequest::default(),
        }
    }
}

/// `requests`, as one request the engine takes: their files by where they are on the machine, as
/// the system's always are, and their links.
fn open_request_of(requests: Vec<SystemOpenRequest>) -> OpenRequest {
    let mut request = OpenRequest::default();
    for opened in requests {
        request
            .files
            .extend(opened.files.into_iter().filter_map(|file| match file {
                PickedFile::Path(path) => Some(path),
                PickedFile::Handle(_) | PickedFile::Stream(_) => None,
            }));
        request.links.extend(opened.links);
    }
    request
}

/// Quits the application, as the system quits one.
fn quit(app: &mut App) {
    if let Some(exit) = app.platform().app_exit() {
        exit.quit(0);
    }
}

/// Writes what `RUST_LOG` picks of the window process's events to standard error; without it, its
/// own at `info` and warnings elsewhere.
fn write_traces() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_unset| EnvFilter::new("warn,waos_window=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

#[cfg(test)]
mod tests;
