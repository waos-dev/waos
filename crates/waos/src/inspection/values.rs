//! The engine's own values made `wasdk:inspect`'s: a day of a program's audit summary, and what it
//! uses now and each change to it.

use std::time::SystemTime;

use waos_capabilities::{AskedQuestion, AuditDay, DeviceSession, FolderUses, NamedUses, Uses};

use super::bindings::wasdk::inspect::active_uses;
use super::bindings::wasdk::inspect::audit;
use crate::access::events::mount_kind_of;
use crate::access::trace::instant_of;
use crate::access::{ActiveUseChange, ActiveUseKind, ActiveUseNow};
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

/// `active`, a use a program makes now, as `wasdk:inspect` says it: an open file where the
/// system says it is now, since the time of day its start was.
pub(crate) fn active_use_of(active: ActiveUseNow) -> active_uses::ActiveUse {
    let since = SystemTime::now() - active.since.elapsed();
    active_uses::ActiveUse {
        id: active.id,
        kind: active_use_kind_of(active.kind),
        since: instant_of(since),
    }
}

/// `change`, to what a program uses now, as `wasdk:inspect` says it.
pub(crate) fn active_use_change_of(change: ActiveUseChange) -> active_uses::ActiveUseChange {
    match change {
        ActiveUseChange::Started(started) => {
            active_uses::ActiveUseChange::Started(active_use_of(started))
        }
        ActiveUseChange::Ended(id) => active_uses::ActiveUseChange::Ended(id),
        ActiveUseChange::Resync(active) => {
            active_uses::ActiveUseChange::Resync(active.into_iter().map(active_use_of).collect())
        }
    }
}

fn active_use_kind_of(kind: ActiveUseKind) -> active_uses::ActiveUseKind {
    use active_uses::ActiveUseKind as Kind;
    let capture = |simulated| active_uses::Capture { simulated };
    match kind {
        ActiveUseKind::Recording(RecordingDevice::Camera) => Kind::Camera(capture(false)),
        ActiveUseKind::Recording(RecordingDevice::SimulatedCamera) => Kind::Camera(capture(true)),
        ActiveUseKind::Recording(RecordingDevice::Microphone) => Kind::Microphone(capture(false)),
        ActiveUseKind::Recording(RecordingDevice::SimulatedMicrophone) => {
            Kind::Microphone(capture(true))
        }
        ActiveUseKind::Command {
            machine,
            command,
            in_terminal,
        } => Kind::Command(active_uses::RunningCommand {
            machine,
            command,
            in_terminal,
        }),
        ActiveUseKind::HttpRequest { method, uri } => Kind::HttpRequest(active_uses::HttpRequest {
            method: method.to_string(),
            url: uri.to_string(),
        }),
        ActiveUseKind::FolderWatch { folder, recursive } => {
            Kind::FolderWatch(active_uses::FolderWatch {
                folder: folder.display().to_string(),
                recursive,
            })
        }
        ActiveUseKind::OpenFile { file, mount, mode } => Kind::OpenFile(active_uses::OpenFile {
            path: file
                .upgrade()
                .and_then(|file| crate::files::path_of(&file).ok())
                .map(|path| path.display().to_string()),
            mount: mount_kind_of(mount),
            read: mode.read,
            write: mode.write,
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
