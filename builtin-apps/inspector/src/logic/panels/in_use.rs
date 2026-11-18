//! What a program uses now, as a system's activity monitor lists a process's open files and
//! sockets: a row per use active, what it is, what kind, what it moved for a connection or a
//! socket, ticking in place as it moves, and since when; a use leaves as it ends.
//!
//! The invariant: the rows are exactly the uses active as last told, so a use that ended never
//! shows; and what a connection or a socket moved is as WaOS last told it, its start, a resync or
//! its traffic, an update of its row, at most once a second.

use wasdk::inspect::active_uses::{
    ActiveUse, ActiveUseChange, ActiveUseKind, SocketAddress, TcpConnectionOrigin, Traffic,
};

use super::{Cell, Column, PanelRow, RowKind, Rows, Said, Tile, Tone, Wording, found_in};
use crate::logic::detail::{Detail, Field, Glyph, Section};
use crate::words;

/// What the panel hears: what is active, then each change.
#[derive(Debug, Default)]
pub(crate) struct InUse;

impl InUse {
    /// What was active as the panel began to listen, or as WaOS told it again.
    pub(crate) fn began(&mut self, rows: &mut impl Rows<InUseRow>, mut uses: Vec<ActiveUse>) {
        rows.clear();
        uses.sort_by_key(|used| used.since.seconds);
        for used in uses {
            rows.insert(rows.len(), InUseRow::of(used));
        }
    }

    pub(crate) fn change(&mut self, rows: &mut impl Rows<InUseRow>, change: ActiveUseChange) {
        match change {
            ActiveUseChange::Started(used) => rows.insert(rows.len(), InUseRow::of(used)),
            ActiveUseChange::Ended(id) => {
                if let Some(index) = rows.newest(|row| row.used.id == id) {
                    rows.remove(index);
                }
            }
            ActiveUseChange::Resync(uses) => self.began(rows, uses),
            ActiveUseChange::Updated(traffic) => {
                if let Some(index) = rows.newest(|row| row.used.id == traffic.id) {
                    rows.update(index, |row| row.moved_to(&traffic));
                }
            }
        }
    }
}

/// The kinds of a use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InUseKind {
    Connection,
    OpenFile,
    Capture,
    Watch,
    Command,
}

impl RowKind for InUseKind {
    const ALL: &'static [InUseKind] = &[
        InUseKind::Connection,
        InUseKind::OpenFile,
        InUseKind::Capture,
        InUseKind::Watch,
        InUseKind::Command,
    ];

    fn name(self) -> &'static str {
        match self {
            InUseKind::Connection => "Connections",
            InUseKind::OpenFile => "Open files",
            InUseKind::Capture => "Captures",
            InUseKind::Watch => "Watched folders",
            InUseKind::Command => "Commands",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct InUseRow {
    used: ActiveUse,
    kind: InUseKind,
    glyph: Glyph,
    /// What it is, in a word: `Connection`, `Open to read`…
    what: &'static str,
    /// What it reaches: a name, a path, a command.
    name: String,
}

impl InUseRow {
    fn of(used: ActiveUse) -> InUseRow {
        let (kind, glyph, what, name) = said(&used.kind);
        InUseRow {
            used,
            kind,
            glyph,
            what,
            name,
        }
    }

    /// What a connection or a socket moved so far, as WaOS told it.
    fn moved_to(&mut self, traffic: &Traffic) {
        match &mut self.used.kind {
            ActiveUseKind::TcpConnection(connection) => {
                connection.bytes_sent = traffic.bytes_sent;
                connection.bytes_received = traffic.bytes_received;
            }
            ActiveUseKind::UdpSocket(socket) => {
                socket.bytes_sent = traffic.bytes_sent;
                socket.bytes_received = traffic.bytes_received;
            }
            _ => {}
        }
    }

    /// The bytes it sent and received, for a connection or a socket.
    pub(crate) fn moved(&self) -> Option<(u64, u64)> {
        match &self.used.kind {
            ActiveUseKind::TcpConnection(connection) => {
                Some((connection.bytes_sent, connection.bytes_received))
            }
            ActiveUseKind::UdpSocket(socket) => Some((socket.bytes_sent, socket.bytes_received)),
            _ => None,
        }
    }
}

impl PanelRow for InUseRow {
    type Kind = InUseKind;

    const COLUMNS: &'static [Column] = &[
        Column::rest("What"),
        Column::points("Kind", 150.0).stepping_out(),
        Column::numeric("Sent", 88.0),
        Column::numeric("Received", 96.0),
        Column::numeric("Since", 80.0).stepping_out(),
    ];

    const NOTHING_YET: &'static str = words::NOTHING_IN_USE;

    const DECIDED: bool = false;

    const COUNTED_AS: &'static str = words::IN_USE_NOW;

    fn kind(&self) -> InUseKind {
        self.kind
    }

    fn glyph(&self) -> Glyph {
        self.glyph
    }

    fn cell(&self, index: usize) -> Cell {
        let moved = self.moved();
        match index {
            0 => Cell::code(self.name.clone()),
            1 => Cell::plain(self.what).toned(Tone::Quiet),
            2 => Cell::code(
                moved
                    .map(|(sent, _)| words::bytes(sent))
                    .unwrap_or_default(),
            ),
            3 => Cell::code(
                moved
                    .map(|(_, received)| words::bytes(received))
                    .unwrap_or_default(),
            ),
            _ => Cell::code(Said::moment(self.used.since, Wording::TimeOfDay)).toned(Tone::Quiet),
        }
    }

    fn refused(&self) -> bool {
        false
    }

    fn matches(&self, text: &str) -> bool {
        found_in(text, &[&self.name, self.what])
    }

    fn tile(&self) -> Tile {
        let since = Said::moment_after(words::SINCE, self.used.since, Wording::TimeOfDay);
        let mut details = vec![since];
        if let Some((sent, received)) = self.moved() {
            details.push(Said::text(format!(
                "{} sent, {} received",
                words::bytes(sent),
                words::bytes(received)
            )));
        }
        Tile {
            code: true,
            title: format!("{}: {}", self.what, self.name),
            details,
            beneath: Vec::new(),
        }
    }

    fn summed(rows: &[&InUseRow]) -> Option<String> {
        let (sent, received) = (rows.iter())
            .filter_map(|row| row.moved())
            .fold((0, 0), |(sent, received), (s, r)| (sent + s, received + r));
        Some(format!(
            "{} {} · {} {}",
            words::bytes(sent),
            words::SENT_LOWER,
            words::bytes(received),
            words::RECEIVED_LOWER
        ))
    }

    fn detail(&self) -> Detail {
        let mut fields = vec![
            Field::plain(words::KIND, self.what),
            Field::code(
                words::SINCE,
                Said::moment(self.used.since, Wording::Precise),
            ),
        ];
        if let Some((sent, received)) = self.moved() {
            fields.push(Field::code(words::SENT, words::bytes(sent)));
            fields.push(Field::code(words::RECEIVED, words::bytes(received)));
        }
        Detail {
            glyph: self.glyph,
            title: self.what.to_owned(),
            subtitle: self.name.clone(),
            refusal: None,
            sections: vec![Section {
                title: words::IN_USE,
                fields,
            }],
        }
    }
}

/// A use's kind, glyph, what it is in a word, and what it reaches.
fn said(kind: &ActiveUseKind) -> (InUseKind, Glyph, &'static str, String) {
    match kind {
        ActiveUseKind::Camera(capture) => (
            InUseKind::Capture,
            Glyph::Camera,
            "Camera",
            words::capturing(words::CAMERA, capture.simulated),
        ),
        ActiveUseKind::Microphone(capture) => (
            InUseKind::Capture,
            Glyph::Microphone,
            "Microphone",
            words::capturing(words::MICROPHONE, capture.simulated),
        ),
        ActiveUseKind::Command(command) => {
            let mut said = format!("{}, on {}", command.command, command.machine);
            if command.in_terminal {
                said = format!("{said}, {}", words::IN_A_TERMINAL);
            }
            (InUseKind::Command, Glyph::Machine, "Command", said)
        }
        ActiveUseKind::HttpRequest(request) => (
            InUseKind::Connection,
            Glyph::Request,
            "Request",
            format!("{} {}", request.method, request.url),
        ),
        ActiveUseKind::FolderWatch(watch) if watch.recursive => (
            InUseKind::Watch,
            Glyph::Watch,
            "Watch, and under it",
            watch.folder.clone(),
        ),
        ActiveUseKind::FolderWatch(watch) => (
            InUseKind::Watch,
            Glyph::Watch,
            "Watch",
            watch.folder.clone(),
        ),
        ActiveUseKind::OpenFile(file) => {
            let what = match (file.read, file.write) {
                (true, true) => "Open to read and write",
                (false, true) => "Open to write",
                _ => "Open to read",
            };
            let path = file
                .path
                .clone()
                .unwrap_or_else(|| words::WHERE_UNKNOWN.to_owned());
            (InUseKind::OpenFile, Glyph::File, what, path)
        }
        ActiveUseKind::TcpConnection(connection) => {
            let said = match connection.names.first() {
                Some(name) => format!("{name} ({})", address(&connection.remote)),
                None => address(&connection.remote),
            };
            let what = match connection.origin {
                TcpConnectionOrigin::Connected => "Connection",
                TcpConnectionOrigin::Accepted => "Accepted",
                TcpConnectionOrigin::HttpRequest => "HTTP connection",
            };
            (InUseKind::Connection, Glyph::Connection, what, said)
        }
        ActiveUseKind::TcpListener(at) => (
            InUseKind::Connection,
            Glyph::Listener,
            "Listening",
            address(at),
        ),
        ActiveUseKind::UdpSocket(socket) => {
            let said = match (&socket.local, &socket.remote) {
                (_, Some(remote)) => format!("to {}", address(remote)),
                (Some(local), None) => format!("at {}", address(local)),
                (None, None) => words::NOT_BOUND.to_owned(),
            };
            (InUseKind::Connection, Glyph::Datagram, "UDP socket", said)
        }
    }
}

fn address(at: &SocketAddress) -> String {
    format!("{}:{}", at.address, at.port)
}

#[cfg(test)]
mod tests {
    use wasdk::inspect::active_uses::{FolderWatch, TcpConnection};
    use wasdk::system_clock::Instant;

    use super::*;

    fn watch(id: u64, since: i64) -> ActiveUse {
        ActiveUse {
            id,
            kind: ActiveUseKind::FolderWatch(FolderWatch {
                folder: format!("/data/{id}"),
                recursive: false,
            }),
            since: Instant {
                seconds: since,
                nanoseconds: 0,
            },
        }
    }

    fn ids(rows: &[InUseRow]) -> Vec<u64> {
        rows.iter().map(|row| row.used.id).collect()
    }

    #[test]
    fn the_rows_are_the_uses_active_as_last_told() {
        let (mut log, mut rows) = (InUse, Vec::new());
        log.began(&mut rows, vec![watch(2, 20), watch(1, 10)]);
        assert_eq!(ids(&rows), [1, 2], "the oldest first");
        log.change(&mut rows, ActiveUseChange::Started(watch(3, 30)));
        log.change(&mut rows, ActiveUseChange::Ended(1));
        assert_eq!(ids(&rows), [2, 3]);
        log.change(&mut rows, ActiveUseChange::Resync(vec![watch(4, 40)]));
        assert_eq!(ids(&rows), [4], "a resync replaces them all");
    }

    #[test]
    fn a_connections_traffic_updates_its_row() {
        let (mut log, mut rows) = (InUse, Vec::new());
        let connection = ActiveUse {
            id: 7,
            kind: ActiveUseKind::TcpConnection(TcpConnection {
                origin: TcpConnectionOrigin::Connected,
                local: None,
                remote: SocketAddress {
                    address: "192.0.2.1".to_owned(),
                    port: 443,
                },
                names: Vec::new(),
                bytes_sent: 0,
                bytes_received: 0,
            }),
            since: Instant {
                seconds: 1,
                nanoseconds: 0,
            },
        };
        log.began(&mut rows, vec![connection, watch(8, 2)]);
        let traffic = Traffic {
            id: 7,
            bytes_sent: 3_700,
            bytes_received: 176_400,
        };
        log.change(&mut rows, ActiveUseChange::Updated(traffic));
        assert_eq!(rows[0].moved(), Some((3_700, 176_400)));
        assert_eq!(rows[1].moved(), None, "a watch moves nothing");
    }
}
