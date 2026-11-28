//! WaOS's end of the Mach connections to its window processes, as Chrome's `AppShimListener` and
//! `MachBootstrapAcceptor` are the browser's: the bootstrap name of WaOS's data folder, checked in
//! while WaOS serves window processes, which every window process's hello arrives on, with its own
//! port; each hello answered `Welcome` with a port of the connection's once the window process is
//! found to be its app's and, where it was opened by the user, the instance it serves is known, or
//! `Refused` with why; and each connection's messages carried out on a thread of its own until the
//! window process is gone. A hello the user's open sent, from the Dock or a notification's click,
//! asks app management to open the app, and waits for the instance it answers, which is handed
//! the files or the click the hello carried; the hellos after it wait behind it: one is taken at a
//! time. The Dock or the click brought the window process forward already, as macOS does for an
//! app the user opens.
//!
//! The invariants: a hello's sender is known by the audit token the kernel wrote on its message,
//! never by what the hello says, and nothing is welcomed, nor opened for the user, before its code
//! is checked; and the listener's thread holds the process table, never `WindowProcesses`, whose
//! going stops it.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{AppInstance, OpenRequest};
use waos_package::ProgramId;
use waos_window_messages::os::macos::{Arrival, ReceiveRight, Received, SendRight};
use waos_window_messages::{
    FromWindow, Hello, LaunchReason, PROTOCOL, ToWindow, Welcome, decode, encode,
};

use super::window_process::{SendToWindow, WindowProcess};
use super::window_processes::{Bundles, ProcessTable};
use crate::logic::app_instances::{OpenConfiguration, OpenPlace, OpenedFor};
use crate::logic::app_management::AppManagement;
use crate::logic::opening::hand_over;
use crate::logic::program_places::ProgramPlace;

/// The thread that takes the window processes' hellos, until it is stopped.
pub(super) struct Listener {
    /// Set as the listener stops, before it is woken.
    stopping: Arc<AtomicBool>,
    /// A right to the listener's own port, which wakes it to stop.
    wake: SendRight,
    thread: Option<JoinHandle<()>>,
}

impl Listener {
    /// Checks `name` in, and takes every hello that arrives on it on a thread of its own, until
    /// stopped: each checked by `bundles`, the app a hello the user's open sent opened by
    /// `app_management`, and each window process welcomed served into `table`.
    pub(super) fn listen(
        name: &str,
        table: Arc<ProcessTable>,
        bundles: Arc<dyn Bundles>,
        app_management: Arc<dyn AppManagement>,
    ) -> io::Result<Listener> {
        let port = ReceiveRight::check_in(name)?;
        let wake = port.make_send()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let hellos = Hellos {
            table,
            bundles,
            app_management,
        };
        let stopped = Arc::clone(&stopping);
        let thread = thread::Builder::new()
            .name("waos-window-processes".to_owned())
            .spawn(move || hellos.take_each(&port, &stopped))?;
        tracing::info!(name, "WaOS listens for its apps' window processes");
        Ok(Listener {
            stopping,
            wake,
            thread: Some(thread),
        })
    }

    /// Stops taking hellos once the one being taken is: the port is let go, so a window process
    /// that looks the name up finds none, and the thread ends.
    pub(super) fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Err(error) = self.wake.send(PROTOCOL, &[], Vec::new()) {
            tracing::warn!(%error, "the listener for the window processes could not be woken to stop");
            return;
        }
        let Some(thread) = self.thread.take() else {
            return;
        };
        // The listener's own thread lets go of the last holder when a run it served was the last
        // thing holding the engine's shell: it ends as it returns.
        if thread.thread().id() != thread::current().id() {
            let _ = thread.join();
        }
    }
}

/// The hellos the listener takes, and what it takes them with.
struct Hellos {
    table: Arc<ProcessTable>,
    bundles: Arc<dyn Bundles>,
    app_management: Arc<dyn AppManagement>,
}

impl Hellos {
    /// Takes every hello that arrives on `port`, one at a time, until `stopping` is set.
    fn take_each(&self, port: &ReceiveRight, stopping: &AtomicBool) {
        loop {
            let arrival = port.receive(PROTOCOL);
            if stopping.load(Ordering::Acquire) {
                return;
            }
            match arrival {
                Ok(Arrival::Message(hello)) => self.take(hello),
                Ok(Arrival::NoSenders) => {}
                Ok(Arrival::OtherProtocol { protocol, received }) => {
                    refuse_other_protocol(protocol, received);
                }
                Err(error) => tracing::warn!(%error, "a message no window process could have sent"),
            }
        }
    }

    /// Welcomes the window process that sent `received` if it is its app's, and serves it;
    /// refuses it if not.
    fn take(&self, received: Received) {
        let Received {
            bytes,
            ports,
            sender,
        } = received;
        let Some(app) = ports.into_iter().next() else {
            tracing::warn!(pid = sender.pid(), "a hello with no port to answer on");
            return;
        };
        let hello = match decode::<FromWindow>(&bytes) {
            Ok(FromWindow::Hello(hello)) => hello,
            Ok(message) => return tracing::warn!(?message, "a message before any hello"),
            Err(error) => return refuse(&app, &format!("a hello this WaOS cannot read: {error}")),
        };
        tracing::info!(
            ?hello,
            pid = sender.pid(),
            "an app's window process said hello"
        );
        let Hello {
            program: id,
            launched_for,
            opened,
        } = hello;
        let found = Arc::clone(&self.bundles)
            .check(&id, &sender)
            .and_then(|()| self.instance_for(&id, launched_for, opened));
        match found {
            Ok(instance) => {
                if let Err(error) = self.serve(id, instance, app) {
                    tracing::warn!(%error, "the app's window process could not be served");
                }
            }
            Err(reason) => refuse(&app, &reason),
        }
    }

    /// The instance the checked hello of `app`'s window process serves from its hello on: none
    /// for one WaOS launched, `launched_for` an instance running, which attaches to it if it asked
    /// for it; for a window process the user opened, with `files`, or a click on one of its app's
    /// notifications launched, the instance opening it answers.
    fn instance_for(
        &self,
        app: &ProgramId,
        launched_for: LaunchReason,
        files: OpenRequest,
    ) -> Result<Option<AppInstance>, String> {
        let notification_click = match launched_for {
            LaunchReason::ProgramRunning if self.table.launching(app) => return Ok(None),
            LaunchReason::ProgramRunning => {
                return Err("WaOS launched no window process for it".to_owned());
            }
            LaunchReason::UserOpened => None,
            LaunchReason::NotificationActivated(click) => Some(click),
        };
        self.open_for_the_user(app, files, notification_click)
            .map(Some)
    }

    /// The instance app management answers for the user's open of `app`, in a window, as the
    /// library opens one, the one running left as it is, once it does, handed `files` or
    /// `notification_click`; the hellos after this one wait. An open that hands something over
    /// runs a tool too, as files handed to it do.
    fn open_for_the_user(
        &self,
        app: &ProgramId,
        files: OpenRequest,
        notification_click: Option<Activation>,
    ) -> Result<AppInstance, String> {
        let hands_over =
            !(files.files.is_empty() && files.links.is_empty()) || notification_click.is_some();
        let opened_for = if hands_over {
            OpenedFor::HandOver
        } else {
            OpenedFor::User
        };
        let configuration = OpenConfiguration {
            place: OpenPlace::Named(ProgramPlace::Windows),
            ..OpenConfiguration::new(opened_for)
        };
        let opening = Arc::clone(&self.app_management).open(app.clone(), configuration);
        let instance = futures::executor::block_on(opening)
            .map(|opened| opened.instance().clone())
            .map_err(|error| format!("{error:#}"))?;
        hand_over(&instance, files, notification_click);
        Ok(instance)
    }

    /// Welcomes the window process of the app `id`, `app`, with a port of the connection's, serves
    /// it as its app's, `instance` attached where the hello came of opening it, and carries out
    /// what the window process sends on a thread of its own.
    fn serve(
        &self,
        id: ProgramId,
        instance: Option<AppInstance>,
        app: SendRight,
    ) -> io::Result<()> {
        let connection = ReceiveRight::new()?;
        let welcome = Welcome {
            name: self.bundles.name(&id),
        };
        let give = connection.make_send()?;
        connection.notify_when_no_senders()?;
        app.send(PROTOCOL, &encode(&ToWindow::Welcome(welcome)), vec![give])?;
        let send = SendToWindow::new(move |message| {
            if let Err(error) = app.send(PROTOCOL, &encode(&message), Vec::new()) {
                tracing::warn!(%error, ?message, "the app's window process did not take a message");
            }
        });
        let process = self.table.connect(&id, instance, send);
        let table = Arc::clone(&self.table);
        thread::Builder::new()
            .name(format!("waos-app {id}"))
            .spawn(move || {
                carry_out(&connection, &process);
                table.disconnected(&id, &process);
            })?;
        Ok(())
    }
}

/// Refuses the window process whose hello, `received`, is of `protocol`, not this WaOS's: one
/// plain line names both, and it is answered in this WaOS's protocol, which it cannot read, so it
/// ends rather than waits. Its bundle is written again at its next launch through WaOS, or as WaOS
/// starts, being made from another WaOS's files (`bundles`).
fn refuse_other_protocol(protocol: u32, received: Received) {
    let pid = received.sender.pid();
    tracing::warn!(
        pid,
        "a program's app speaks the window protocol {protocol} and this WaOS {PROTOCOL}: it is refused"
    );
    if let Some(app) = received.ports.into_iter().next() {
        let reason = format!("this WaOS speaks the window protocol {PROTOCOL}, not {protocol}");
        // A window process gone meanwhile has nobody to tell.
        let _ = app.send(PROTOCOL, &encode(&ToWindow::Refused(reason)), Vec::new());
    }
}

/// Answers the window process `app` that it is refused, and why.
fn refuse(app: &SendRight, reason: &str) {
    tracing::warn!(reason, "a program's app was refused");
    let _ = app.send(
        PROTOCOL,
        &encode(&ToWindow::Refused(reason.to_owned())),
        Vec::new(),
    );
}

/// Carries out every message the app sends on `connection`, until it is gone.
fn carry_out(connection: &ReceiveRight, process: &WindowProcess) {
    loop {
        match connection.receive(PROTOCOL) {
            Ok(Arrival::Message(received)) => match decode::<FromWindow>(&received.bytes) {
                Ok(message) => process.received(message, received.ports),
                Err(error) => tracing::warn!(%error, "a message this WaOS cannot read"),
            },
            Ok(Arrival::NoSenders) => return,
            Ok(Arrival::OtherProtocol { protocol, .. }) => {
                tracing::warn!(
                    "the program's app spoke the window protocol {protocol}, not {PROTOCOL}: its connection ends"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "receiving from the program's app");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_of_another_protocol_is_answered_in_this_one_and_so_ends() {
        let waos = ReceiveRight::new().unwrap();
        let to_waos = waos.make_send().unwrap();
        let app = ReceiveRight::new().unwrap();
        let older = PROTOCOL - 1;
        to_waos
            .send(older, b"an older hello", vec![app.make_send().unwrap()])
            .unwrap();
        let Arrival::OtherProtocol { protocol, received } = waos.receive(PROTOCOL).unwrap() else {
            panic!("a hello of another protocol");
        };
        assert_eq!(protocol, older);
        refuse_other_protocol(protocol, received);
        let Arrival::OtherProtocol {
            protocol: answered_in,
            received,
        } = app.receive(older).unwrap()
        else {
            panic!("an answer the older app cannot read, which ends it");
        };
        assert_eq!(answered_in, PROTOCOL);
        let Ok(ToWindow::Refused(reason)) = decode::<ToWindow>(&received.bytes) else {
            panic!("refused");
        };
        assert_eq!(
            reason,
            format!("this WaOS speaks the window protocol {PROTOCOL}, not {older}")
        );
    }
}
