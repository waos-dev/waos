//! Each thing told, made into the event of `wasdk:inspect`'s `trace` its capability sends: from
//! what was asked and what the rule resolved it to, from what was noted, held and ended, and from
//! what happened. Made only for a capability someone listens to (`trace`), so what costs here, a
//! file's path asked of the system or a string made, is paid only then.
//!
//! The invariant: an event says no more than the summary may know and the trace may show: what
//! was asked, where it landed and the verdict, never what a use read or wrote.

use std::time::{Duration, SystemTime};

use super::summary::{answer_key, prompt_key, switch_key};
use super::trace::{TraceEvent, TraceKind, duration_of, instant_of};
use super::{Asked, Cause, FileChange, Happened, Holding, Noted, Resolution};
use crate::inspection::bindings::wasdk::inspect::audit::MountKind as WitMountKind;
use crate::inspection::bindings::wasdk::inspect::trace::{
    Cause as WitCause, ClipboardEvent, ClipboardUse, Connection, Datagram, DeviceEvent, DeviceUse,
    FileEvent, FileUse, LinkEvent, Lookup, MachineEvent, MachineUse, NetworkEvent, NetworkUse,
    NotificationEvent, OutputChunk, OutputStream, PermissionEvent, PermissionUse, Request,
    Response, Run, Verdict,
};
use crate::shell::RecordingDevice;
use waos_capabilities::MountKind;

/// The capability `asked` is a use of.
pub(super) fn kind_of(asked: &Asked<'_>) -> TraceKind {
    match asked {
        Asked::ClipboardRead { .. } | Asked::ClipboardWrite { .. } => TraceKind::Clipboard,
        Asked::OpenCamera { .. } => TraceKind::Camera,
        Asked::OpenMicrophone { .. } => TraceKind::Microphone,
        Asked::Run { .. } => TraceKind::Machines,
        Asked::OpenLink { .. } => TraceKind::Links,
        Asked::Post => TraceKind::Notifications,
        Asked::Open { .. } | Asked::Change { .. } | Asked::Watch { .. } => TraceKind::Files,
        Asked::Lookup { .. }
        | Asked::Connect { .. }
        | Asked::Accept { .. }
        | Asked::Listen { .. }
        | Asked::Request { .. } => TraceKind::Network,
    }
}

/// The event of `asked`, resolved as `resolution` says where it was allowed, refused by `refused`
/// or failed otherwise, in the program's `instance`.
pub(super) fn decided(
    asked: &Asked<'_>,
    resolution: Option<&Resolution<'_>>,
    refused: Option<&'static str>,
    instance: u32,
) -> TraceEvent {
    let verdict = match (resolution, refused) {
        (Some(_), _) => Verdict::Allowed,
        (None, Some(setting)) => Verdict::Refused(setting.to_owned()),
        (None, None) => Verdict::Failed,
    };
    let at = instant_of(SystemTime::now());
    let network = |kind| {
        TraceEvent::Network(NetworkEvent {
            at,
            instance,
            dropped_before: 0,
            kind,
            verdict: verdict.clone(),
        })
    };
    let device = |kind| DeviceEvent {
        at,
        instance,
        dropped_before: 0,
        kind,
        verdict: verdict.clone(),
    };
    match asked {
        Asked::ClipboardRead { form, pasted } => TraceEvent::Clipboard(ClipboardEvent {
            at,
            instance,
            dropped_before: 0,
            kind: ClipboardUse::Read(((*form).to_owned(), *pasted)),
            verdict,
        }),
        Asked::ClipboardWrite { forms } => TraceEvent::Clipboard(ClipboardEvent {
            at,
            instance,
            dropped_before: 0,
            kind: ClipboardUse::Write(forms.iter().map(|form| (*form).to_owned()).collect()),
            verdict,
        }),
        Asked::OpenCamera { id, simulated } => TraceEvent::Camera(device(DeviceUse::Opened((
            Some((*id).to_owned()),
            *simulated,
        )))),
        Asked::OpenMicrophone { simulated } => {
            TraceEvent::Microphone(device(DeviceUse::Opened((None, *simulated))))
        }
        Asked::Run {
            machine,
            command,
            in_terminal,
        } => TraceEvent::Machines(MachineEvent {
            at,
            instance,
            dropped_before: 0,
            kind: MachineUse::Run(Run {
                machine: (*machine).to_owned(),
                program: command.program.clone(),
                arguments: command.arguments.clone(),
                in_terminal: *in_terminal,
            }),
            verdict,
        }),
        Asked::OpenLink { link } => TraceEvent::Links(LinkEvent {
            at,
            instance,
            dropped_before: 0,
            link: (*link).to_owned(),
            verdict,
        }),
        Asked::Post => TraceEvent::Notifications(NotificationEvent {
            at,
            instance,
            dropped_before: 0,
            verdict,
        }),
        Asked::Open { path, mode, cause } => {
            let kind = if mode.write {
                FileUse::OpenToWrite
            } else {
                FileUse::OpenToRead
            };
            files(
                Some(path),
                resolution,
                *cause,
                kind,
                (at, instance, verdict),
            )
        }
        Asked::Change { path, change } => {
            let kind = match change {
                FileChange::CreateDirectory => FileUse::CreateDirectory,
                FileChange::RemoveDirectory => FileUse::RemoveDirectory,
                FileChange::Remove => FileUse::Remove,
                FileChange::Rename { to } => FileUse::Rename((*to).to_owned()),
                FileChange::Link { to } => FileUse::Link((*to).to_owned()),
                FileChange::Symlink { contents } => FileUse::Symlink((*contents).to_owned()),
                FileChange::SetTimes => FileUse::SetTimes,
            };
            files(
                Some(path),
                resolution,
                Cause::Program,
                kind,
                (at, instance, verdict),
            )
        }
        Asked::Watch { recursive } => files(
            None,
            resolution,
            Cause::Program,
            FileUse::Watch(*recursive),
            (at, instance, verdict),
        ),
        Asked::Lookup { name, cause } => network(NetworkUse::Lookup(Lookup {
            name: (*name).to_owned(),
            cause: cause_of(*cause),
            addresses: match resolution {
                Some(Resolution::Addresses(addresses)) => {
                    addresses.iter().map(ToString::to_string).collect()
                }
                _ => Vec::new(),
            },
        })),
        Asked::Connect { address } => {
            network(NetworkUse::Connect(connection(*address, resolution)))
        }
        Asked::Accept { address } => network(NetworkUse::Accept(connection(*address, resolution))),
        Asked::Listen { address } => network(NetworkUse::Listen(address.port())),
        Asked::Request { id, method, uri } => network(NetworkUse::Request(Request {
            id: *id,
            method: method.to_string(),
            url: uri.to_string(),
        })),
    }
}

/// The capability `noted` is told on.
pub(super) fn noted_kind(noted: &Noted<'_>) -> TraceKind {
    match noted {
        Noted::Named { .. } | Noted::FirstWrite { .. } => TraceKind::Files,
        Noted::Datagram { .. } => TraceKind::Network,
    }
}

/// The event of what `noted` notes, in the program's `instance`.
pub(super) fn noted(noted: &Noted<'_>, instance: u32) -> TraceEvent {
    let at = instant_of(SystemTime::now());
    match noted {
        Noted::Named { path, resolution } => files(
            Some(path),
            Some(resolution),
            Cause::Program,
            FileUse::Named,
            (at, instance, Verdict::Allowed),
        ),
        Noted::FirstWrite { mount, file } => TraceEvent::Files(FileEvent {
            at,
            instance,
            dropped_before: 0,
            path: None,
            on_machine: crate::files::path_of(file)
                .ok()
                .map(|path| path.display().to_string()),
            mount: Some(mount_kind_of(mount.kind)),
            cause: WitCause::Program,
            kind: FileUse::FirstWrite,
            verdict: Verdict::Allowed,
        }),
        Noted::Datagram {
            address,
            sent,
            refusal,
        } => TraceEvent::Network(NetworkEvent {
            at,
            instance,
            dropped_before: 0,
            kind: NetworkUse::Datagram(Datagram {
                address: address.ip().to_string(),
                port: address.port(),
                sent: *sent,
            }),
            verdict: match refusal {
                None => Verdict::Allowed,
                Some(refusal) => Verdict::Refused(refusal.setting.to_owned()),
            },
        }),
    }
}

/// The capability `holding`'s start and end are told on, where they are told.
pub(super) fn held_kind(holding: &Holding) -> Option<TraceKind> {
    match holding {
        Holding::Recording(RecordingDevice::Camera | RecordingDevice::SimulatedCamera) => {
            Some(TraceKind::Camera)
        }
        Holding::Recording(RecordingDevice::Microphone | RecordingDevice::SimulatedMicrophone) => {
            Some(TraceKind::Microphone)
        }
        Holding::Run { .. } | Holding::Request { .. } | Holding::Watch { .. } => None,
    }
}

/// The capability a held use's end is told on, where it is told: a recording's, and a run's.
pub(super) fn ended_kind(holding: &Holding) -> Option<TraceKind> {
    match holding {
        Holding::Run { .. } => Some(TraceKind::Machines),
        _ => held_kind(holding),
    }
}

/// The event of a recording's start, told on `kind`.
pub(super) fn held(kind: TraceKind, instance: u32) -> TraceEvent {
    device_event(kind, DeviceUse::Started, instance)
}

/// The event of a held use's end, after `length`, told on `kind`.
pub(super) fn ended(kind: TraceKind, length: Duration, instance: u32) -> TraceEvent {
    if kind == TraceKind::Machines {
        return TraceEvent::Machines(MachineEvent {
            at: instant_of(SystemTime::now()),
            instance,
            dropped_before: 0,
            kind: MachineUse::Ended(duration_of(length)),
            verdict: Verdict::Allowed,
        });
    }
    device_event(kind, DeviceUse::Ended(duration_of(length)), instance)
}

/// The capability what `happened` is told on.
pub(super) fn happened_kind(happened: &Happened<'_>) -> TraceKind {
    match happened {
        Happened::Asked(_)
        | Happened::Answered(..)
        | Happened::SettingsOpened(_)
        | Happened::Mounted { .. } => TraceKind::Permissions,
        Happened::MachinePicked { .. } => TraceKind::Machines,
        Happened::Responded { .. } => TraceKind::Network,
    }
}

/// The event of what `happened`, in the program's `instance`.
pub(super) fn happened(happened: &Happened<'_>, instance: u32) -> TraceEvent {
    let at = instant_of(SystemTime::now());
    let permission = |kind| {
        TraceEvent::Permissions(PermissionEvent {
            at,
            instance,
            dropped_before: 0,
            kind,
        })
    };
    match happened {
        Happened::Asked(prompt) => permission(PermissionUse::Asked(prompt_key(prompt))),
        Happened::Answered(prompt, answer) => {
            let answer = answer.map(|answer| answer_key(answer).to_owned());
            permission(PermissionUse::Answered((prompt_key(prompt), answer)))
        }
        Happened::SettingsOpened(switch) => permission(PermissionUse::SettingsOpened(
            switch_key(*switch).to_owned(),
        )),
        Happened::Mounted { on_machine } => {
            permission(PermissionUse::Mounted(on_machine.display().to_string()))
        }
        Happened::MachinePicked { machine } => TraceEvent::Machines(MachineEvent {
            at,
            instance,
            dropped_before: 0,
            kind: MachineUse::Picked((*machine).to_owned()),
            verdict: Verdict::Allowed,
        }),
        Happened::Responded {
            request,
            status,
            took,
        } => TraceEvent::Network(NetworkEvent {
            at,
            instance,
            dropped_before: 0,
            kind: NetworkUse::Response(Response {
                request: *request,
                status: *status,
                took: duration_of(*took),
            }),
            verdict: Verdict::Allowed,
        }),
    }
}

/// The event of what the program wrote to its standard output or errors.
pub(super) fn output(errors: bool, bytes: &[u8], instance: u32) -> TraceEvent {
    TraceEvent::Output(OutputChunk {
        at: instant_of(SystemTime::now()),
        instance,
        dropped_before: 0,
        channel: if errors {
            OutputStream::Stderr
        } else {
            OutputStream::Stdout
        },
        bytes: bytes.to_vec(),
    })
}

/// A file's event: the path the program named, where the use landed, what it was for and what.
fn files(
    path: Option<&str>,
    resolution: Option<&Resolution<'_>>,
    cause: Cause,
    kind: FileUse,
    (at, instance, verdict): (
        crate::inspection::bindings::wasi::clocks::system_clock::Instant,
        u32,
        Verdict,
    ),
) -> TraceEvent {
    let (on_machine, mount) = match resolution {
        Some(Resolution::File {
            kind,
            folder,
            beneath,
            ..
        }) => (
            crate::files::path_of(folder)
                .ok()
                .map(|folder| folder.join(beneath).display().to_string()),
            Some(mount_kind_of(*kind)),
        ),
        Some(Resolution::OnMachine(path)) => (
            Some(path.display().to_string()),
            Some(WitMountKind::Machine),
        ),
        Some(Resolution::Both(both)) => {
            return files(path, Some(&both.0), cause, kind, (at, instance, verdict));
        }
        _ => (None, None),
    };
    TraceEvent::Files(FileEvent {
        at,
        instance,
        dropped_before: 0,
        path: path.map(str::to_owned),
        on_machine,
        mount,
        cause: cause_of(cause),
        kind,
        verdict,
    })
}

fn connection(address: std::net::SocketAddr, resolution: Option<&Resolution<'_>>) -> Connection {
    Connection {
        address: address.ip().to_string(),
        port: address.port(),
        names: match resolution {
            Some(Resolution::Names(names)) => names.to_vec(),
            _ => Vec::new(),
        },
    }
}

fn device_event(kind: TraceKind, use_: DeviceUse, instance: u32) -> TraceEvent {
    let event = DeviceEvent {
        at: instant_of(SystemTime::now()),
        instance,
        dropped_before: 0,
        kind: use_,
        verdict: Verdict::Allowed,
    };
    if kind == TraceKind::Camera {
        TraceEvent::Camera(event)
    } else {
        TraceEvent::Microphone(event)
    }
}

fn cause_of(cause: Cause) -> WitCause {
    match cause {
        Cause::Program => WitCause::Program,
        Cause::Request => WitCause::Request,
        Cause::Player => WitCause::Player,
        Cause::Opener => WitCause::Opener,
        Cause::ImageCodecs => WitCause::ImageCodecs,
        Cause::FileDialog => WitCause::FileDialog,
        Cause::Clipboard => WitCause::Clipboard,
    }
}

pub(crate) fn mount_kind_of(kind: MountKind) -> WitMountKind {
    match kind {
        MountKind::OwnData => WitMountKind::OwnData,
        MountKind::OwnCache => WitMountKind::OwnCache,
        MountKind::OwnTemporary => WitMountKind::OwnTemporary,
        MountKind::Granted => WitMountKind::Granted,
        MountKind::Picked => WitMountKind::Picked,
        MountKind::Pasted => WitMountKind::Pasted,
        MountKind::Machine => WitMountKind::Machine,
    }
}
