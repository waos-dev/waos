//! WaOS's end of the Mach connections to its programs' apps, as Chrome's `AppShimListener` and
//! `MachBootstrapAcceptor` are the browser's: the bootstrap name of WaOS's data folder, checked in
//! for as long as WaOS runs, which every app's hello arrives on, with the app's own port; each hello
//! answered `Welcome` with a port of the connection's once the app is found to be its program's,
//! or `Refused` with why, and what it carried handed to the program; and each connection's
//! messages carried out on a thread of its own until the app is gone.
//!
//! The invariant: a hello's sender is known by the audit token the kernel wrote on its message,
//! never by what the hello says, and nothing is welcomed before its code is checked.

use std::io;
use std::sync::Arc;
use std::thread;

use waos::{OpenRequest, RunningProgram};
use waos_window_messages::os::macos::{Arrival, ReceiveRight, Received, SendRight};
use waos_window_messages::{
    FromWindow, Hello, LaunchReason, PROTOCOL, ToWindow, Welcome, decode, encode,
};

use super::ProgramApps;
use super::window_process::{SendToWindow, WindowProcess};
use crate::logic::program_name::program_name;

/// Checks `name` in, and takes every hello that arrives on it on a thread of its own, for as long
/// as WaOS runs.
pub(super) fn listen(apps: &Arc<ProgramApps>, name: &str) -> io::Result<()> {
    let port = ReceiveRight::check_in(name)?;
    let apps = Arc::downgrade(apps);
    thread::Builder::new()
        .name("waos-program-apps".to_owned())
        .spawn(move || {
            loop {
                let hello = match port.receive(PROTOCOL) {
                    Ok(Arrival::Message(hello)) => hello,
                    Ok(Arrival::NoSenders) => continue,
                    Ok(Arrival::OtherProtocol { protocol, received }) => {
                        refuse_other_protocol(protocol, received);
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "a message no app could have sent");
                        continue;
                    }
                };
                let Some(apps) = apps.upgrade() else {
                    return;
                };
                take_hello(&apps, hello);
            }
        })?;
    tracing::info!(name, "WaOS listens for its programs' apps");
    Ok(())
}

/// Welcomes the app that sent `received` if it is its program's, and serves it; refuses it if not.
fn take_hello(apps: &Arc<ProgramApps>, received: Received) {
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
    tracing::info!(?hello, pid = sender.pid(), "a program's app said hello");
    let found = apps.runtime.block_on(apps.program_for(&hello, &sender));
    match found {
        Ok(program) => {
            if let Err(error) = serve(apps, hello, program, app) {
                tracing::warn!(%error, "the program's app could not be served");
            }
        }
        Err(reason) => refuse(&app, &reason),
    }
}

/// Refuses the app whose hello, `received`, is of `protocol`, not this WaOS's: one plain line names
/// both, and the app is answered in this WaOS's protocol, which it cannot read, so it ends rather
/// than waits. Its bundle is written again at its next launch through WaOS, or as WaOS starts, being
/// made from another WaOS's files (`bundles`).
fn refuse_other_protocol(protocol: u32, received: Received) {
    let pid = received.sender.pid();
    tracing::warn!(
        pid,
        "a program's app speaks the window protocol {protocol} and this WaOS {PROTOCOL}: it is refused"
    );
    if let Some(app) = received.ports.into_iter().next() {
        let reason = format!("this WaOS speaks the window protocol {PROTOCOL}, not {protocol}");
        // An app gone meanwhile has nobody to tell.
        let _ = app.send(PROTOCOL, &encode(&ToWindow::Refused(reason)), Vec::new());
    }
}

/// Answers the app `app` that it is refused, and why.
fn refuse(app: &SendRight, reason: &str) {
    tracing::warn!(reason, "a program's app was refused");
    let _ = app.send(
        PROTOCOL,
        &encode(&ToWindow::Refused(reason.to_owned())),
        Vec::new(),
    );
}

/// Welcomes the app `app` with a port of the connection's, serves it as `program`'s, hands the
/// program what the hello carried, and carries out what the app sends on a thread of its own.
fn serve(
    apps: &Arc<ProgramApps>,
    hello: Hello,
    program: RunningProgram,
    app: SendRight,
) -> io::Result<()> {
    let Hello {
        program: id,
        launched_for,
        opened,
    } = hello;
    let connection = ReceiveRight::new()?;
    let welcome = Welcome {
        name: program_name(&apps.packages, &program),
    };
    let give = connection.make_send()?;
    connection.notify_when_no_senders()?;
    app.send(PROTOCOL, &encode(&ToWindow::Welcome(welcome)), vec![give])?;
    let send = SendToWindow::new(move |message| {
        if let Err(error) = app.send(PROTOCOL, &encode(&message), Vec::new()) {
            tracing::warn!(%error, ?message, "the program's app did not take a message");
        }
    });
    let process = apps.connected(&id, program.clone(), send);
    deliver_hello(&program, launched_for, opened);
    let apps = Arc::downgrade(apps);
    thread::Builder::new()
        .name(format!("waos-app {id}"))
        .spawn(move || {
            carry_out(&connection, &process);
            if let Some(apps) = apps.upgrade() {
                apps.disconnected(&id, &process);
            }
        })?;
    Ok(())
}

/// Hands `program` what its app's hello carried: the files and links the system opened the app
/// with, and the click on one of its notifications the app was launched for, each heard once the
/// program listens.
pub(super) fn deliver_hello(
    program: &RunningProgram,
    launched_for: LaunchReason,
    opened: OpenRequest,
) {
    if !(opened.files.is_empty() && opened.links.is_empty()) {
        program.open_requested(opened);
    }
    if let LaunchReason::NotificationActivated(click) = launched_for {
        program.notification_activated(click);
    }
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
