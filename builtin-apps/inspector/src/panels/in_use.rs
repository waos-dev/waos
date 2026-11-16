//! What a program uses now, as a system's activity monitor lists a process's open files and
//! sockets: a row per use active, what it is, what it moved for a connection or a socket, and since
//! when; a use leaves as it ends.
//!
//! The invariant: the rows are exactly the uses active as last told, so a use that ended never
//! shows; and what a connection moved is as of when it was told, its start or a resync.

use wasdk::inspect::active_uses::{
    ActiveUse, ActiveUseChange, ActiveUseKind, SocketAddress, TcpConnectionOrigin,
};

use super::{Column, Panel, PanelRow};
use crate::{clock, words};

const COLUMNS: [Column; 5] = [
    ("Name", 0.0),
    ("Kind", 104.0),
    ("Sent", 76.0),
    ("Received", 76.0),
    ("Since", 76.0),
];

/// The uses active, the oldest first.
#[derive(Debug, Default)]
pub(crate) struct InUse {
    uses: Vec<ActiveUse>,
}

impl InUse {
    /// What was active as the panel began to listen.
    pub(crate) fn began(&mut self, uses: Vec<ActiveUse>) {
        self.uses = uses;
        self.uses.sort_by_key(|used| used.since.seconds);
    }

    pub(crate) fn change(&mut self, change: ActiveUseChange) {
        match change {
            ActiveUseChange::Started(used) => self.uses.push(used),
            ActiveUseChange::Ended(id) => self.uses.retain(|used| used.id != id),
            ActiveUseChange::Resync(uses) => self.began(uses),
        }
    }
}

impl Panel for InUse {
    fn columns(&self) -> &'static [Column] {
        &COLUMNS
    }

    fn rows(&self) -> Vec<PanelRow> {
        self.uses.iter().map(drawn).collect()
    }

    fn nothing_yet(&self) -> &'static str {
        words::NOTHING_IN_USE
    }
}

fn drawn(used: &ActiveUse) -> PanelRow {
    let (kind, name, moved) = said(&used.kind);
    let since = clock::time_of_day(used.since.seconds);
    let (sent, received) = moved
        .map(|(sent, received)| (words::bytes(sent), words::bytes(received)))
        .unwrap_or_default();
    let mut details = vec![format!("{} {since}", words::SINCE)];
    if moved.is_some() {
        details.push(format!("{sent} sent, {received} received"));
    }
    PanelRow {
        cells: vec![name.clone(), kind.to_owned(), sent, received, since],
        title: format!("{kind}: {name}"),
        details,
        beneath: Vec::new(),
    }
}

/// A use's kind, what it is, and for a connection or a socket the bytes it sent and received.
fn said(kind: &ActiveUseKind) -> (&'static str, String, Option<(u64, u64)>) {
    match kind {
        ActiveUseKind::Camera(capture) => (
            "Camera",
            words::capturing(words::CAMERA, capture.simulated),
            None,
        ),
        ActiveUseKind::Microphone(capture) => (
            "Microphone",
            words::capturing(words::MICROPHONE, capture.simulated),
            None,
        ),
        ActiveUseKind::Command(command) => {
            let mut said = format!("{}, on {}", command.command, command.machine);
            if command.in_terminal {
                said = format!("{said}, {}", words::IN_A_TERMINAL);
            }
            ("Command", said, None)
        }
        ActiveUseKind::HttpRequest(request) => (
            "Request",
            format!("{} {}", request.method, request.url),
            None,
        ),
        ActiveUseKind::FolderWatch(watch) if watch.recursive => {
            ("Watch", format!("{}, and under it", watch.folder), None)
        }
        ActiveUseKind::FolderWatch(watch) => ("Watch", watch.folder.clone(), None),
        ActiveUseKind::OpenFile(file) => {
            let kind = match (file.read, file.write) {
                (true, true) => "Open to read and write",
                (false, true) => "Open to write",
                _ => "Open to read",
            };
            let path = file
                .path
                .clone()
                .unwrap_or_else(|| words::WHERE_UNKNOWN.to_owned());
            (kind, path, None)
        }
        ActiveUseKind::TcpConnection(connection) => {
            let said = match connection.names.first() {
                Some(name) => format!("{name} ({})", address(&connection.remote)),
                None => address(&connection.remote),
            };
            let kind = match connection.origin {
                TcpConnectionOrigin::Connected => "Connection",
                TcpConnectionOrigin::Accepted => "Accepted",
                TcpConnectionOrigin::HttpRequest => "HTTP connection",
            };
            let moved = (connection.bytes_sent, connection.bytes_received);
            (kind, said, Some(moved))
        }
        ActiveUseKind::TcpListener(at) => ("Listening", address(at), None),
        ActiveUseKind::UdpSocket(socket) => {
            let said = match (&socket.local, &socket.remote) {
                (_, Some(remote)) => format!("to {}", address(remote)),
                (Some(local), None) => format!("at {}", address(local)),
                (None, None) => words::NOT_BOUND.to_owned(),
            };
            let moved = (socket.bytes_sent, socket.bytes_received);
            ("UDP socket", said, Some(moved))
        }
    }
}

fn address(at: &SocketAddress) -> String {
    format!("{}:{}", at.address, at.port)
}
