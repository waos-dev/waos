//! The first frame each way: the build, the protocol's major and, from WaOS, the requests it
//! serves, so the two halves find out what each can do before a request is sent, and a field
//! added later never breaks an older half.
//!
//! The invariant: the hello's shape is frozen. A field is added with `#[serde(default)]`, and a
//! side ignores the fields it does not know, so only a real break raises [`MAJOR`].

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::adjacent::adjacent_unknown;
use crate::messages::Request;
use crate::token::Token;

/// The protocol's major, which each hello says: raised only by a real break in the messages'
/// meaning, while a field or a message added leaves it as it is. 3 since a run became its steps,
/// `install`, `launch`, `attach` and `stop`, and `run` went: a `waos` that runs programs with
/// `run` is told to update itself rather than that WaOS lacks it.
pub const MAJOR: u32 = 3;

/// Every request a WaOS serves, by [`Request::name`]: what its hello lists.
pub const SERVED: [&str; 15] = [
    "screenshot",
    "input",
    "resize",
    "settings",
    "set",
    "inspect",
    "open",
    "install",
    "launch",
    "attach",
    "stop",
    "uninstall",
    "extension",
    "serving-programs",
    "pair",
];

/// Which half of the protocol sent a [`Hello`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(remote = "Self", tag = "kind", content = "data")]
pub enum Peer {
    WaOS,
    #[default]
    Command,
    WaosOpen,
    #[serde(skip)]
    Unknown,
}

adjacent_unknown!(Peer);

/// What each side says first: the build it is, the protocol's major it speaks, and, for WaOS,
/// the requests it serves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub major: u32,
    /// The build as people name it: "WaOS 0.4.1", "waos 0.4.1".
    pub build: String,
    /// WaOS's: the requests it serves ([`SERVED`]); `waos`'s: none.
    #[serde(default)]
    pub serves: Vec<String>,
    /// Which binary sent this hello.
    #[serde(default)]
    pub peer: Peer,
    /// `waos`'s across the network: the token the device gave this computer when the two paired,
    /// without which the device answers nothing. None on this machine's socket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<Token>,
}

impl Hello {
    /// WaOS's hello, `build` naming it, serving every request this build has.
    pub fn waos(build: impl Into<String>) -> Hello {
        Hello {
            major: MAJOR,
            build: build.into(),
            serves: SERVED.iter().map(|request| request.to_string()).collect(),
            peer: Peer::WaOS,
            token: None,
        }
    }

    /// `waos`'s hello, `build` naming it.
    pub fn command(build: impl Into<String>) -> Hello {
        Hello {
            major: MAJOR,
            build: build.into(),
            serves: Vec::new(),
            peer: Peer::Command,
            token: None,
        }
    }

    /// `waos open`'s hello, `build` naming it.
    pub fn waos_open(build: impl Into<String>) -> Hello {
        Hello {
            major: MAJOR,
            build: build.into(),
            serves: Vec::new(),
            peer: Peer::WaosOpen,
            token: None,
        }
    }

    /// Whether the half that said `self` may be sent `request` by `ours`: the majors agree and
    /// `self` serves the request; else what to say, naming both builds and which one to update.
    pub fn accepts(&self, ours: &Hello, request: &Request) -> Result<(), Mismatch> {
        if self.major != ours.major {
            return Err(Mismatch::Major {
                theirs: Box::new(self.clone()),
                ours: Box::new(ours.clone()),
            });
        }
        if !self.serves.iter().any(|served| served == request.name()) {
            return Err(Mismatch::NotServed {
                theirs: Box::new(self.clone()),
                request: request.name(),
            });
        }
        Ok(())
    }
}

/// Why the two halves cannot talk, in words naming both builds and which one to update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mismatch {
    /// The protocol's major differs: each side's hello.
    Major {
        theirs: Box<Hello>,
        ours: Box<Hello>,
    },
    /// WaOS does not serve the request: its hello, and the request's name.
    NotServed {
        theirs: Box<Hello>,
        request: &'static str,
    },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::Major { theirs, ours } => {
                let older = if theirs.major > ours.major {
                    ours.peer
                } else {
                    theirs.peer
                };
                let action = match older {
                    Peer::Command => {
                        "update waos with `curl -fsSL https://waos.app/install.sh | sh`"
                    }
                    Peer::WaOS | Peer::WaosOpen | Peer::Unknown => {
                        "update WaOS from https://waos.app"
                    }
                };
                write!(
                    f,
                    "{} speaks dev protocol {} and {} speaks {}: {action}",
                    theirs.build, theirs.major, ours.build, ours.major
                )
            }
            Mismatch::NotServed { theirs, request } => write!(
                f,
                "{} does not serve `{request}`: update WaOS from https://waos.app",
                theirs.build
            ),
        }
    }
}

impl std::error::Error for Mismatch {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::messages::{Input, Inspection, Install, Launch, Request, WindowId};
    use crate::pairing::PairStart;
    use crate::settings_rows::Row;
    use waos_package::ProgramId;

    use super::*;

    fn install() -> Request {
        Request::Install(Install {
            id: "dev.example.triangle".parse().unwrap(),
            kind: waos_package::ProgramKind::App,
            name: "Triangle".into(),
            version: "0.1.0".parse().unwrap(),
            icon: None,
            category: None,
            file_associations: Vec::new(),
            length: 0,
            digest: [0; 32],
        })
    }

    fn screenshot() -> Request {
        Request::Screenshot {
            id: "dev.example.triangle".parse::<ProgramId>().unwrap(),
            window: None,
        }
    }

    #[test]
    fn served_lists_every_request() {
        let id: ProgramId = "dev.example.triangle".parse().unwrap();
        let all = [
            Request::Screenshot {
                id: id.clone(),
                window: None,
            },
            Request::Input {
                id: id.clone(),
                window: Some(WindowId(1)),
                input: Input::Tap { x: 0.0, y: 0.0 },
            },
            Request::Resize {
                id: id.clone(),
                window: None,
                width: 100.0,
                height: 100.0,
            },
            Request::Settings { id: id.clone() },
            Request::Set {
                id: id.clone(),
                row: Row::Machine(None),
            },
            Request::Inspect {
                id: id.clone(),
                asked: Inspection::ActiveUses,
            },
            Request::Open {
                id: id.clone(),
                files: Vec::<PathBuf>::new(),
            },
            install(),
            Request::Launch(Launch {
                id: id.development(),
                arguments: Vec::new(),
                variables: Vec::new(),
            }),
            Request::Attach {
                id: id.development(),
            },
            Request::Stop {
                id: id.development(),
            },
            Request::Uninstall {
                id: id.development(),
            },
            Request::Extension {
                id: id.clone(),
                name: "ext.waft.debugDumpApp".to_owned(),
                params: Vec::new(),
            },
            Request::ServingPrograms,
            Request::Pair(PairStart {
                computer: "mac".to_owned(),
                spake: vec![1, 2, 3],
            }),
        ];
        // Match exhaustively so adding a `Request` variant fails to compile here until `all` and
        // `SERVED` list it too.
        for request in &all {
            match request {
                Request::Screenshot { .. }
                | Request::Input { .. }
                | Request::Resize { .. }
                | Request::Settings { .. }
                | Request::Set { .. }
                | Request::Inspect { .. }
                | Request::Open { .. }
                | Request::Install(_)
                | Request::Launch(_)
                | Request::Attach { .. }
                | Request::Stop { .. }
                | Request::Uninstall { .. }
                | Request::Extension { .. }
                | Request::ServingPrograms
                | Request::Pair(_)
                | Request::Unknown => {}
            }
        }
        let names: Vec<&str> = all.iter().map(Request::name).collect();
        assert_eq!(names, SERVED);
        let waos = Hello::waos("WaOS 0.3.0");
        let cli = Hello::command("waos 0.3.0");
        for request in &all {
            assert_eq!(waos.accepts(&cli, request), Ok(()));
        }
        let empty = Hello {
            serves: Vec::new(),
            ..Hello::waos("WaOS 0.3.0")
        };
        assert!(matches!(
            empty.accepts(&cli, &install()),
            Err(Mismatch::NotServed {
                request: "install",
                ..
            })
        ));
    }

    #[test]
    fn a_request_a_waos_does_not_serve_is_refused_naming_it() {
        let theirs = Hello {
            serves: vec!["install".to_owned()],
            ..Hello::waos("WaOS 0.3.0")
        };
        let ours = Hello::command("waos 0.3.0");
        assert_eq!(theirs.accepts(&ours, &install()), Ok(()));
        let refused = theirs.accepts(&ours, &screenshot()).unwrap_err();
        assert!(matches!(refused, Mismatch::NotServed { .. }));
        assert!(
            refused.to_string().contains("screenshot")
                && refused.to_string().contains("WaOS 0.3.0"),
            "{refused}"
        );
    }

    #[test]
    fn a_major_that_differs_names_both_builds_and_the_one_to_update() {
        let ours = Hello::command("waos 0.3.0");
        let newer = Hello {
            major: MAJOR + 1,
            ..Hello::waos("WaOS 0.4.0")
        };
        let refused = newer.accepts(&ours, &install()).unwrap_err();
        assert!(
            refused.to_string().contains("update waos")
                && refused.to_string().contains("WaOS 0.4.0")
                && refused.to_string().contains("waos 0.3.0"),
            "{refused}"
        );
        let older = Hello {
            major: MAJOR - 1,
            ..Hello::waos("WaOS 0.2.0")
        };
        let refused = older.accepts(&ours, &install()).unwrap_err();
        assert!(
            refused.to_string().contains("update WaOS")
                && refused.to_string().contains("WaOS 0.2.0"),
            "{refused}"
        );

        let open = Hello::waos_open("waos 0.3.0");
        let refused = newer.accepts(&open, &install()).unwrap_err();
        assert!(
            refused.to_string().contains("update WaOS")
                && !refused.to_string().contains("cargo install")
                && refused.to_string().contains("waos 0.3.0"),
            "{refused}"
        );
    }

    #[test]
    fn a_field_added_to_the_hello_leaves_an_older_reader_whole() {
        /// A newer side's hello, with a field this build does not know.
        #[derive(serde::Serialize)]
        struct NewerHello {
            major: u32,
            build: String,
            serves: Vec<String>,
            new_field: u32,
        }

        let newer = NewerHello {
            major: MAJOR,
            build: "WaOS 0.4.0".to_owned(),
            serves: Vec::new(),
            new_field: 7,
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&newer, &mut bytes).unwrap();
        let hello: Hello = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(hello.build, "WaOS 0.4.0");
        assert_eq!(hello.major, MAJOR);
        assert_eq!(hello.peer, Peer::Command);
    }
}
