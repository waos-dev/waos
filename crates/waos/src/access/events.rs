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
use super::{ActiveUseKind, Asked, Cause, FileChange, Happened, Noted, Resolution};
use crate::inspection::bindings::wasdk::inspect::audit::MountKind as WitMountKind;
use crate::inspection::bindings::wasdk::inspect::trace::{
    Cause as WitCause, ClipboardEvent, ClipboardRead, ClipboardUse, DeviceEvent, DeviceOpened,
    DeviceUse, FileEvent, FileUse, HttpRequest, HttpResponse, LinkEvent, MachineEvent, MachineUse,
    NameLookup, NetworkEvent, NetworkUse, NotificationEvent, OutputChunk, OutputStream,
    PermissionEvent, PermissionUse, QuestionAnswer, StartedCommand, TcpConnection, UdpDatagram,
    Verdict, WatchOptions,
};
use crate::shell::RecordingDevice;
use waos_capabilities::MountKind;

/// The capability `asked` is a use of.
pub(super) fn kind_of(asked: &Asked<'_>) -> TraceKind {
    match asked {
        Asked::ClipboardRead { .. } | Asked::ClipboardWrite { .. } => TraceKind::Clipboard,
        Asked::OpenCamera { .. } => TraceKind::Camera,
        Asked::OpenMicrophone { .. } => TraceKind::Microphone,
        Asked::RunCommand { .. } => TraceKind::Machines,
        Asked::OpenLink { .. } => TraceKind::Links,
        Asked::PostNotification => TraceKind::Notifications,
        Asked::OpenFile { .. } | Asked::ChangeFile { .. } | Asked::WatchFolder { .. } => {
            TraceKind::Files
        }
        Asked::LookUpName { .. }
        | Asked::ConnectTcp { .. }
        | Asked::AcceptTcp { .. }
        | Asked::ListenTcp { .. }
        | Asked::SendHttpRequest { .. } => TraceKind::Network,
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
            kind: ClipboardUse::Read(ClipboardRead {
                form: (*form).to_owned(),
                pasted: *pasted,
            }),
            verdict,
        }),
        Asked::ClipboardWrite { forms } => TraceEvent::Clipboard(ClipboardEvent {
            at,
            instance,
            dropped_before: 0,
            kind: ClipboardUse::Write(forms.iter().map(|form| (*form).to_owned()).collect()),
            verdict,
        }),
        Asked::OpenCamera { id, simulated } => {
            TraceEvent::Camera(device(DeviceUse::Opened(DeviceOpened {
                camera_id: Some((*id).to_owned()),
                simulated: *simulated,
            })))
        }
        Asked::OpenMicrophone { simulated } => {
            TraceEvent::Microphone(device(DeviceUse::Opened(DeviceOpened {
                camera_id: None,
                simulated: *simulated,
            })))
        }
        Asked::RunCommand {
            machine,
            command,
            in_terminal,
        } => TraceEvent::Machines(MachineEvent {
            at,
            instance,
            dropped_before: 0,
            kind: MachineUse::CommandStarted(StartedCommand {
                machine: (*machine).to_owned(),
                command: command.program.clone(),
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
        Asked::PostNotification => TraceEvent::Notifications(NotificationEvent {
            at,
            instance,
            dropped_before: 0,
            verdict,
        }),
        Asked::OpenFile { path, mode, cause } => {
            let kind = if mode.write {
                FileUse::OpenToWrite
            } else {
                FileUse::OpenToRead
            };
            file_event(
                Some(path),
                resolution,
                *cause,
                kind,
                (at, instance, verdict),
            )
        }
        Asked::ChangeFile { path, change } => {
            let kind = match change {
                FileChange::CreateDirectory => FileUse::CreateDirectory,
                FileChange::RemoveDirectory => FileUse::RemoveDirectory,
                FileChange::Remove => FileUse::Remove,
                FileChange::Rename { to } => FileUse::Rename((*to).to_owned()),
                FileChange::HardLink { to } => FileUse::HardLink((*to).to_owned()),
                FileChange::Symlink { contents } => FileUse::Symlink((*contents).to_owned()),
                FileChange::SetTimes => FileUse::SetTimes,
            };
            file_event(
                Some(path),
                resolution,
                Cause::Program,
                kind,
                (at, instance, verdict),
            )
        }
        Asked::WatchFolder { recursive } => file_event(
            None,
            resolution,
            Cause::Program,
            FileUse::FolderWatch(WatchOptions {
                recursive: *recursive,
            }),
            (at, instance, verdict),
        ),
        Asked::LookUpName { name, cause } => network(NetworkUse::NameLookup(NameLookup {
            name: (*name).to_owned(),
            cause: cause_of(*cause),
            addresses: match resolution {
                Some(Resolution::Addresses(addresses)) => {
                    addresses.iter().map(ToString::to_string).collect()
                }
                _ => Vec::new(),
            },
        })),
        Asked::ConnectTcp { address } => {
            network(NetworkUse::TcpConnect(tcp_connection(*address, resolution)))
        }
        Asked::AcceptTcp { address } => {
            network(NetworkUse::TcpAccept(tcp_connection(*address, resolution)))
        }
        Asked::ListenTcp { address } => network(NetworkUse::TcpListen(address.port())),
        Asked::SendHttpRequest { id, method, uri } => {
            network(NetworkUse::HttpRequest(HttpRequest {
                id: *id,
                method: method.to_string(),
                url: uri.to_string(),
            }))
        }
    }
}

/// The capability `noted` is told on.
pub(super) fn noted_kind(noted: &Noted<'_>) -> TraceKind {
    match noted {
        Noted::MetadataRead { .. } | Noted::FirstWrite { .. } => TraceKind::Files,
        Noted::UdpDatagram { .. } => TraceKind::Network,
    }
}

/// The event of what `noted` notes, in the program's `instance`.
pub(super) fn noted(noted: &Noted<'_>, instance: u32) -> TraceEvent {
    let at = instant_of(SystemTime::now());
    match noted {
        Noted::MetadataRead { path, resolution } => file_event(
            Some(path),
            Some(resolution),
            Cause::Program,
            FileUse::MetadataRead,
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
        Noted::UdpDatagram {
            address,
            sent,
            refusal,
        } => TraceEvent::Network(NetworkEvent {
            at,
            instance,
            dropped_before: 0,
            kind: NetworkUse::UdpDatagram(UdpDatagram {
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

/// The capability `kind`'s start and end are told on, where they are told: a recording's.
pub(super) fn held_kind(kind: &ActiveUseKind) -> Option<TraceKind> {
    match kind {
        ActiveUseKind::Recording(RecordingDevice::Camera | RecordingDevice::SimulatedCamera) => {
            Some(TraceKind::Camera)
        }
        ActiveUseKind::Recording(
            RecordingDevice::Microphone | RecordingDevice::SimulatedMicrophone,
        ) => Some(TraceKind::Microphone),
        ActiveUseKind::Command { .. }
        | ActiveUseKind::HttpRequest { .. }
        | ActiveUseKind::FolderWatch { .. }
        | ActiveUseKind::OpenFile { .. }
        | ActiveUseKind::TcpConnection { .. }
        | ActiveUseKind::TcpListener { .. }
        | ActiveUseKind::UdpSocket { .. } => None,
    }
}

/// The capability a use's end is told on, where it is told: a recording's, and a command's.
pub(super) fn ended_kind(kind: &ActiveUseKind) -> Option<TraceKind> {
    match kind {
        ActiveUseKind::Command { .. } => Some(TraceKind::Machines),
        _ => held_kind(kind),
    }
}

/// The event of a recording's start, told on `kind`.
pub(super) fn held(kind: TraceKind, instance: u32) -> TraceEvent {
    device_event(kind, DeviceUse::CaptureStarted, instance)
}

/// The event of a held use's end, after `length`, told on `kind`.
pub(super) fn ended(kind: TraceKind, length: Duration, instance: u32) -> TraceEvent {
    if kind == TraceKind::Machines {
        return TraceEvent::Machines(MachineEvent {
            at: instant_of(SystemTime::now()),
            instance,
            dropped_before: 0,
            kind: MachineUse::CommandEnded(duration_of(length)),
            verdict: Verdict::Allowed,
        });
    }
    device_event(kind, DeviceUse::CaptureEnded(duration_of(length)), instance)
}

/// The capability what `happened` is told on.
pub(super) fn happened_kind(happened: &Happened<'_>) -> TraceKind {
    match happened {
        Happened::QuestionAsked(_)
        | Happened::QuestionAnswered(..)
        | Happened::SettingsOpened(_)
        | Happened::PickMounted { .. } => TraceKind::Permissions,
        Happened::MachinePicked { .. } => TraceKind::Machines,
        Happened::HttpResponded { .. } => TraceKind::Network,
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
        Happened::QuestionAsked(prompt) => {
            permission(PermissionUse::QuestionAsked(prompt_key(prompt)))
        }
        Happened::QuestionAnswered(prompt, answer) => {
            let answer = answer.map(|answer| answer_key(answer).to_owned());
            permission(PermissionUse::QuestionAnswered(QuestionAnswer {
                question: prompt_key(prompt),
                answer,
            }))
        }
        Happened::SettingsOpened(switch) => permission(PermissionUse::SettingsOpened(
            switch_key(*switch).to_owned(),
        )),
        Happened::PickMounted { on_machine } => {
            permission(PermissionUse::PickMounted(on_machine.display().to_string()))
        }
        Happened::MachinePicked { machine } => TraceEvent::Machines(MachineEvent {
            at,
            instance,
            dropped_before: 0,
            kind: MachineUse::MachinePicked((*machine).to_owned()),
            verdict: Verdict::Allowed,
        }),
        Happened::HttpResponded {
            request_id,
            status,
            time_to_first_byte,
        } => TraceEvent::Network(NetworkEvent {
            at,
            instance,
            dropped_before: 0,
            kind: NetworkUse::HttpResponse(HttpResponse {
                request_id: *request_id,
                status: *status,
                time_to_first_byte: duration_of(*time_to_first_byte),
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
fn file_event(
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
            return file_event(path, Some(&both.0), cause, kind, (at, instance, verdict));
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

fn tcp_connection(
    address: std::net::SocketAddr,
    resolution: Option<&Resolution<'_>>,
) -> TcpConnection {
    TcpConnection {
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
        Cause::HttpRequest => WitCause::HttpRequest,
        Cause::MediaPlayer => WitCause::MediaPlayer,
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
