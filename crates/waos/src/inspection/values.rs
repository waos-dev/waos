//! The engine's own values made `wasdk:inspect`'s: a day of a program's audit summary, and what it
//! holds now.

use std::time::{Instant, SystemTime};

use waos_capabilities::{AskedQuestion, AuditDay, DeviceSession, FolderUses, NamedUses, Uses};

use super::bindings::wasdk::inspect::audit;
use super::bindings::wasdk::inspect::holdings;
use crate::access::events::mount_kind_of;
use crate::access::trace::instant_of;
use crate::access::{Holding, Holdings};
use crate::shell::RecordingDevice;

/// `day`, one day of a program's audit summary, as `wasdk:inspect` says it.
pub fn day_of(day: &AuditDay) -> audit::Day {
    audit::Day {
        start: instant_of(day.start),
        domains: day
            .domains
            .iter()
            .map(|uses| named_uses_of(uses, String::clone))
            .collect(),
        local_addresses: day
            .local_addresses
            .iter()
            .map(|uses| named_uses_of(uses, ToString::to_string))
            .collect(),
        udp_datagrams: day.udp_datagrams,
        folders: day.folders.iter().map(folder_of).collect(),
        folder_refusals: day.folder_refusals,
        clipboard_reads: uses_of(&day.clipboard_reads),
        pastes: day.pastes,
        clipboard_writes: uses_of(&day.clipboard_writes),
        camera: day.camera.iter().map(device_session_of).collect(),
        microphone: day.microphone.iter().map(device_session_of).collect(),
        commands: day
            .commands
            .iter()
            .map(|uses| {
                named_uses_of(uses, |command| {
                    format!("{} {}", command.machine, command.command)
                })
            })
            .collect(),
        links: day
            .links
            .iter()
            .map(|uses| named_uses_of(uses, String::clone))
            .collect(),
        notifications: uses_of(&day.notifications),
        questions: day.questions.iter().map(question_asked_of).collect(),
        beyond_caps: audit::Overflow {
            domains: day.beyond_caps.domains,
            local_addresses: day.beyond_caps.local_addresses,
            folders: day.beyond_caps.folders,
            changed: day.beyond_caps.changed,
            commands: day.beyond_caps.commands,
            links: day.beyond_caps.links,
            device_sessions: day.beyond_caps.device_sessions,
            questions: day.beyond_caps.questions,
        },
    }
}

/// What a program holds now, as `wasdk:inspect` says it, each use held since the time of day
/// `now` and `at` say it started.
pub(crate) fn now_of(holdings: Holdings) -> holdings::Now {
    let (now, at) = (Instant::now(), SystemTime::now());
    holdings::Now {
        held: holdings
            .held
            .into_iter()
            .map(|held| holdings::Held {
                holding: holding_of(held.holding),
                since: instant_of(at - now.duration_since(held.since)),
            })
            .collect(),
        open_files: holdings
            .open_files
            .into_iter()
            .map(|open| holdings::OpenFile {
                path: open.path.map(|path| path.display().to_string()),
                mount: mount_kind_of(open.kind),
                read: open.mode.read,
                write: open.mode.write,
            })
            .collect(),
    }
}

fn holding_of(holding: Holding) -> holdings::Holding {
    match holding {
        Holding::Recording(device) => holdings::Holding::Recording(match device {
            RecordingDevice::Camera => holdings::RecordingDevice::Camera,
            RecordingDevice::Microphone => holdings::RecordingDevice::Microphone,
            RecordingDevice::SimulatedCamera => holdings::RecordingDevice::SimulatedCamera,
            RecordingDevice::SimulatedMicrophone => holdings::RecordingDevice::SimulatedMicrophone,
        }),
        Holding::Run {
            machine,
            program,
            in_terminal,
        } => holdings::Holding::Run(holdings::Run {
            machine,
            program,
            in_terminal,
        }),
        Holding::Request { method, uri } => holdings::Holding::Request(holdings::Request {
            method: method.to_string(),
            url: uri.to_string(),
        }),
        Holding::Watch { folder, recursive } => holdings::Holding::Watch(holdings::Watch {
            folder: folder.display().to_string(),
            recursive,
        }),
    }
}

fn uses_of(uses: &Uses) -> audit::Uses {
    audit::Uses {
        allowed: uses.allowed,
        refused: uses.refused,
        first: uses.first.map(instant_of),
        last: uses.last.map(instant_of),
    }
}

fn named_uses_of<T>(uses: &NamedUses<T>, name: impl Fn(&T) -> String) -> audit::NamedUses {
    audit::NamedUses {
        name: name(&uses.name),
        uses: uses_of(&uses.uses),
    }
}

fn folder_of(folder: &FolderUses) -> audit::FolderUses {
    audit::FolderUses {
        path: folder.path.display().to_string(),
        kind: mount_kind_of(folder.kind),
        opened_to_read: folder.opened_to_read,
        opened_to_write: folder.opened_to_write,
        created: folder.created,
        removed: folder.removed,
        renamed: folder.renamed,
        changed: folder
            .changed
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
    }
}

fn device_session_of(session: &DeviceSession) -> audit::DeviceSession {
    audit::DeviceSession {
        start: instant_of(session.start),
        seconds: session.seconds,
        simulated: session.simulated,
    }
}

fn question_asked_of(asked: &AskedQuestion) -> audit::QuestionAsked {
    audit::QuestionAsked {
        question: asked.question.clone(),
        at: instant_of(asked.at),
        answer: asked.answer.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn a_day_reads_as_json_of_the_wit_values() {
        let start = UNIX_EPOCH + Duration::from_secs(1_763_164_800);
        let at = start + Duration::from_secs(60);
        let mut day = AuditDay::starting(start);
        let mut uses = Uses::default();
        uses.count(true, at);
        uses.count(false, at);
        day.domains.push(NamedUses {
            name: "example.com".to_owned(),
            uses,
        });
        day.clipboard_reads.count(false, at);
        let json = serde_json::to_value(day_of(&day)).unwrap();
        assert_eq!(json["start"]["seconds"], 1_763_164_800);
        assert_eq!(json["domains"][0]["name"], "example.com");
        assert_eq!(json["domains"][0]["uses"]["allowed"], 1);
        assert_eq!(json["domains"][0]["uses"]["refused"], 1);
        assert_eq!(json["domains"][0]["uses"]["last"]["seconds"], 1_763_164_860);
        assert_eq!(json["clipboard-reads"]["refused"], 1);
        assert_eq!(json["notifications"]["first"], serde_json::Value::Null);
    }
}
