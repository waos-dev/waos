//! A program's network as it happens, as Chrome DevTools' Network panel lists it: a row per event,
//! what it reached, what it was, when it started and how WaOS decided it; an HTTP request's row
//! takes its response's status and time, as DevTools' row of a request does.
//!
//! The invariant: a response joins the row of the request it answers wherever that row is still
//! kept; only a response to a request made before the panel showed has a row of its own.

use wasdk::inspect::trace::{HttpResponse, NetworkEvent, NetworkUse, TcpConnection};

use super::{Column, Decided, Kept, Panel, PanelRow};
use crate::{clock, words};

/// The columns, in DevTools' order where it has them.
const COLUMNS: [Column; 6] = [
    ("Name", 0.0),
    ("Status", 56.0),
    ("Kind", 76.0),
    ("Time", 72.0),
    ("Started", 76.0),
    ("Decided", 72.0),
];

/// The rows heard so far.
#[derive(Debug, Default)]
pub(crate) struct NetworkLog {
    rows: Kept<NetworkRow>,
}

impl NetworkLog {
    /// `event` in the log: a response in the row of the request it answers where that row is
    /// kept, any other event in a row of its own.
    pub(crate) fn add(&mut self, event: &NetworkEvent) {
        if let NetworkUse::HttpResponse(response) = &event.kind {
            let request = self
                .rows
                .iter_mut()
                .rev()
                .find(|row| row.request == Some(response.request_id));
            if let Some(request) = request {
                request.answered(response);
                return;
            }
        }
        self.rows.push(NetworkRow::of(event));
    }
}

impl Panel for NetworkLog {
    fn columns(&self) -> &'static [Column] {
        &COLUMNS
    }

    fn rows(&self) -> Vec<PanelRow> {
        self.rows.iter().map(NetworkRow::drawn).collect()
    }

    fn nothing_yet(&self) -> &'static str {
        words::NO_NETWORK_YET
    }
}

/// One event, as the panel lists it.
#[derive(Clone, Debug, PartialEq)]
struct NetworkRow {
    /// When it happened, in seconds since the epoch.
    at: i64,
    kind: &'static str,
    /// What it reached: a name, an address and port, a method and URL.
    name: String,
    /// The id of the HTTP request it is, which its response names.
    request: Option<u64>,
    /// Its response's status, once it came.
    status: Option<u16>,
    /// How long its response took to begin to come, in nanoseconds, once it came.
    took: Option<u64>,
    decided: Decided,
}

impl NetworkRow {
    fn of(event: &NetworkEvent) -> NetworkRow {
        let (kind, name) = match &event.kind {
            NetworkUse::NameLookup(lookup) => {
                let found = match lookup.addresses.len() {
                    0 => String::new(),
                    1 => " → 1 address".to_owned(),
                    n => format!(" → {n} addresses"),
                };
                ("Lookup", format!("{}{found}", lookup.name))
            }
            NetworkUse::TcpConnect(connection) => ("Connect", reached(connection)),
            NetworkUse::TcpAccept(connection) => ("Accept", reached(connection)),
            NetworkUse::TcpListen(port) => ("Listen", format!("port {port}")),
            NetworkUse::UdpDatagram(datagram) => {
                let kind = if datagram.sent {
                    "Datagram out"
                } else {
                    "Datagram in"
                };
                (kind, format!("{}:{}", datagram.address, datagram.port))
            }
            NetworkUse::HttpRequest(request) => {
                ("Request", format!("{} {}", request.method, request.url))
            }
            NetworkUse::HttpResponse(_) => ("Response", "to a request made earlier".to_owned()),
        };
        let mut row = NetworkRow {
            at: event.at.seconds,
            kind,
            name,
            request: match &event.kind {
                NetworkUse::HttpRequest(request) => Some(request.id),
                _ => None,
            },
            status: None,
            took: None,
            decided: Decided::of(&event.verdict),
        };
        if let NetworkUse::HttpResponse(response) = &event.kind {
            row.answered(response);
        }
        row
    }

    fn answered(&mut self, response: &HttpResponse) {
        self.status = Some(response.status);
        self.took = Some(response.time_to_first_byte);
    }

    fn drawn(&self) -> PanelRow {
        let status = self.status.map(|status| status.to_string());
        let took = self.took.map(words::took);
        let (started, decided) = (
            clock::time_of_day(self.at),
            words::decided(&self.decided).to_owned(),
        );
        let mut details = vec![started.clone()];
        details.extend(status.clone());
        details.extend(took.clone());
        details.push(decided.clone());
        PanelRow {
            cells: vec![
                self.name.clone(),
                status.unwrap_or_default(),
                self.kind.to_owned(),
                took.unwrap_or_default(),
                started,
                decided,
            ],
            title: format!("{} {}", self.kind, self.name),
            details,
            beneath: words::why_decided(&self.decided).into_iter().collect(),
        }
    }
}

/// A connection's far end, by the names it was looked up by where there are any.
fn reached(connection: &TcpConnection) -> String {
    let at = format!("{}:{}", connection.address, connection.port);
    match connection.names.first() {
        Some(name) => format!("{name} ({at})"),
        None => at,
    }
}
