//! A program's network as it happens, as Chrome DevTools' Network panel lists it: a row per event,
//! what it reached, its response's status, how WaOS decided it, how long its response took and
//! when it started; an HTTP request's row takes its response's status and time, as DevTools' row
//! of a request does.
//!
//! The invariant: a response joins the row of the request it answers wherever that row is still
//! kept, as an update of that row; only a response to a request made before the panel listened
//! has a row of its own.

use wasdk::inspect::trace::{HttpResponse, NetworkEvent, NetworkUse, TcpConnection};
use wasdk::system_clock::Instant;

use super::{Cell, Column, Decided, PanelRow, RowKind, Rows, Tile, Tone, found_in};
use crate::clock;
use crate::logic::detail::{self, Detail, Field, Glyph, Refusal, Section};
use crate::words;

/// What the panel hears, row by row.
#[derive(Debug, Default)]
pub(crate) struct NetworkLog;

impl NetworkLog {
    /// `event` in `rows`: a response in the row of the request it answers where that row is kept,
    /// any other event in a row of its own.
    pub(crate) fn add(&mut self, rows: &mut impl Rows<NetworkRow>, event: &NetworkEvent) {
        if let NetworkUse::HttpResponse(response) = &event.kind {
            let request = rows.newest(|row| row.request == Some(response.request_id));
            if let Some(index) = request {
                rows.update(index, |row| row.answered(response));
                return;
            }
        }
        rows.append(NetworkRow::of(event));
    }
}

/// The kinds of a network event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NetworkKind {
    Request,
    Lookup,
    Connection,
    Datagram,
}

impl RowKind for NetworkKind {
    const ALL: &'static [NetworkKind] = &[
        NetworkKind::Request,
        NetworkKind::Lookup,
        NetworkKind::Connection,
        NetworkKind::Datagram,
    ];

    fn name(self) -> &'static str {
        match self {
            NetworkKind::Request => "Requests",
            NetworkKind::Lookup => "Lookups",
            NetworkKind::Connection => "Connections",
            NetworkKind::Datagram => "Datagrams",
        }
    }
}

/// One event, as the panel lists it.
#[derive(Clone, Debug)]
pub(crate) struct NetworkRow {
    at: Instant,
    instance: u32,
    kind: NetworkKind,
    /// What it was, in a word: `Request`, `Connect`…
    what: &'static str,
    /// What it reached: a name, an address and port, a method and URL.
    name: String,
    /// Where it led: a lookup's addresses, a connection's names.
    led_to: Vec<String>,
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
        let (kind, what, name, led_to) = match &event.kind {
            NetworkUse::NameLookup(lookup) => (
                NetworkKind::Lookup,
                "Lookup",
                lookup.name.clone(),
                lookup.addresses.clone(),
            ),
            NetworkUse::TcpConnect(connection) => (
                NetworkKind::Connection,
                "Connect",
                reached(connection),
                connection.names.clone(),
            ),
            NetworkUse::TcpAccept(connection) => (
                NetworkKind::Connection,
                "Accept",
                reached(connection),
                connection.names.clone(),
            ),
            NetworkUse::TcpListen(port) => (
                NetworkKind::Connection,
                "Listen",
                format!("port {port}"),
                Vec::new(),
            ),
            NetworkUse::UdpDatagram(datagram) => (
                NetworkKind::Datagram,
                if datagram.sent {
                    "Datagram out"
                } else {
                    "Datagram in"
                },
                format!("{}:{}", datagram.address, datagram.port),
                Vec::new(),
            ),
            NetworkUse::HttpRequest(request) => (
                NetworkKind::Request,
                "Request",
                format!("{} {}", request.method, request.url),
                Vec::new(),
            ),
            NetworkUse::HttpResponse(_) => (
                NetworkKind::Request,
                "Response",
                words::TO_AN_EARLIER_REQUEST.to_owned(),
                Vec::new(),
            ),
        };
        let mut row = NetworkRow {
            at: event.at,
            instance: event.instance,
            kind,
            what,
            name,
            led_to,
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

    /// What it was, as the detail pane's title says it: `Request refused`.
    fn titled(&self) -> String {
        match &self.decided {
            Decided::Allowed => self.what.to_owned(),
            Decided::Refused(_) => format!("{} {}", self.what, words::REFUSED_LOWER),
            Decided::Failed => format!("{} {}", self.what, words::FAILED_LOWER),
        }
    }
}

impl PanelRow for NetworkRow {
    type Kind = NetworkKind;

    const COLUMNS: &'static [Column] = &[
        Column::rest("Name"),
        Column::numeric("Status", 56.0).stepping_out(),
        Column::points("Decided", 150.0),
        Column::numeric("Time", 72.0).stepping_out(),
        Column::numeric("Started", 130.0),
    ];

    const NOTHING_YET: &'static str = words::NO_NETWORK_YET;

    fn kind(&self) -> NetworkKind {
        self.kind
    }

    fn glyph(&self) -> Glyph {
        match self.kind {
            NetworkKind::Request => Glyph::Request,
            NetworkKind::Lookup => Glyph::Lookup,
            NetworkKind::Connection if self.what == "Listen" => Glyph::Listener,
            NetworkKind::Connection => Glyph::Connection,
            NetworkKind::Datagram => Glyph::Datagram,
        }
    }

    fn cell(&self, index: usize) -> Cell {
        match index {
            0 => {
                let name = Cell::code(self.name.clone());
                if self.decided.refused() {
                    name.toned(Tone::Refused)
                } else {
                    name
                }
            }
            1 => Cell::code(
                self.status
                    .map(|status| status.to_string())
                    .unwrap_or_default(),
            ),
            2 => self.decided.cell(),
            3 => Cell::code(self.took.map(words::took).unwrap_or_default()).toned(Tone::Quiet),
            _ => Cell::code(clock::precise(self.at)).toned(Tone::Quiet),
        }
    }

    fn refused(&self) -> bool {
        self.decided.refused()
    }

    fn matches(&self, text: &str) -> bool {
        let led_to: Vec<&str> = self.led_to.iter().map(String::as_str).collect();
        found_in(text, &[&self.name, self.what]) || found_in(text, &led_to)
    }

    fn tile(&self) -> Tile {
        let mut details = vec![clock::time_of_day(self.at.seconds)];
        details.extend(self.status.map(|status| status.to_string()));
        details.extend(self.took.map(words::took));
        details.push(words::decided(&self.decided).to_owned());
        Tile {
            code: true,
            title: format!("{} {}", self.what, self.name),
            details,
            beneath: words::why_decided(&self.decided).into_iter().collect(),
        }
    }

    fn detail(&self) -> Detail {
        let mut facts = vec![
            Field::plain(words::KIND, self.what),
            Field::plain(words::INSTANCE, words::instance(self.instance)),
            Field::code(words::AT, clock::precise(self.at)),
        ];
        if self.kind == NetworkKind::Request {
            facts.insert(1, Field::plain(words::THROUGH, words::WAOS_HTTP_CLIENT));
        }
        if !self.led_to.is_empty() {
            facts.push(Field::code(words::LED_TO, self.led_to.join(", ")));
        }
        let mut sections = vec![Section {
            title: words::EVENT,
            fields: facts,
        }];
        if let (Some(status), Some(took)) = (self.status, self.took) {
            sections.push(Section {
                title: words::RESPONSE,
                fields: vec![
                    Field::code(words::STATUS, status.to_string()),
                    Field::code(words::FIRST_BYTE_AFTER, words::took(took)),
                ],
            });
        }
        sections.push(detail::decision(&self.decided));
        Detail {
            glyph: self.glyph(),
            title: self.titled(),
            subtitle: self.name.clone(),
            refusal: Refusal::of(&self.decided),
            sections,
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

#[cfg(test)]
mod tests {
    use wasdk::inspect::trace::{HttpRequest, Verdict};

    use super::*;

    fn event(kind: NetworkUse, verdict: Verdict) -> NetworkEvent {
        NetworkEvent {
            at: Instant {
                seconds: 1,
                nanoseconds: 0,
            },
            instance: 0,
            dropped_before: 0,
            kind,
            verdict,
        }
    }

    fn request(id: u64) -> NetworkUse {
        NetworkUse::HttpRequest(HttpRequest {
            id,
            method: "GET".to_owned(),
            url: format!("https://example.com/{id}"),
        })
    }

    fn response(id: u64) -> NetworkUse {
        NetworkUse::HttpResponse(HttpResponse {
            request_id: id,
            status: 200,
            time_to_first_byte: 12_000_000,
        })
    }

    #[test]
    fn a_response_updates_the_row_of_its_request() {
        let (mut log, mut rows) = (NetworkLog, Vec::new());
        log.add(&mut rows, &event(request(1), Verdict::Allowed));
        log.add(&mut rows, &event(request(2), Verdict::Allowed));
        log.add(&mut rows, &event(response(1), Verdict::Allowed));
        assert_eq!(rows.len(), 2, "no row of its own");
        assert_eq!(rows[0].status, Some(200));
        assert_eq!(rows[1].status, None);
    }

    #[test]
    fn a_response_to_a_request_not_heard_has_a_row_of_its_own() {
        let (mut log, mut rows) = (NetworkLog, Vec::new());
        log.add(&mut rows, &event(response(7), Verdict::Allowed));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].cell(1).text, "200");
    }

    #[test]
    fn a_refused_request_names_its_setting() {
        let (mut log, mut rows) = (NetworkLog, Vec::new());
        log.add(
            &mut rows,
            &event(request(1), Verdict::Refused("network".to_owned())),
        );
        assert_eq!(rows[0].titled(), "Request refused");
        let refusal = Refusal::of(&rows[0].decided).map(|refusal| refusal.setting);
        assert_eq!(refusal, Some("network".to_owned()));
        assert!(rows[0].refused());
    }
}
