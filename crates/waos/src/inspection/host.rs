//! The code serving `wasdk:inspect` to a program holding the inspector role: this device, the
//! programs on it as the embedder gives them (`InspectedPrograms`), each one's icon as it is asked
//! for, a program started held as the embedder starts it, kept until the inspector drops it, or ended, a program's audit summary as the engine's audit log keeps it, today's kept first where it
//! runs, what it uses now and each change to it, and its live trace, a stream per capability. A running program is the one the engine
//! runs under the id asked for, its name, as the embedder names it (`Program::named`). Linked only
//! for a program its user granted the role (`roles`).
//!
//! The invariant: the role is read again at each call and each stream ends as the user takes it
//! back, so a program whose role is turned off sees what a program with no embedder sees: no
//! device, no program, and no stream, from then on.

use std::future::Future;
use std::sync::Arc;

use futures::future;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos_capabilities::{Answer, AuditLog, Role};
use wasmtime::component::{
    Access, Accessor, HasData, Linker, Resource, ResourceTable, StreamReader,
};
use wasmtime_wasi::runtime::spawn_blocking;

use super::bindings::wasdk::inspect::active_uses::{self, ActiveUse, ActiveUseChange};
use super::bindings::wasdk::inspect::audit::{self, Day};
use super::bindings::wasdk::inspect::devices::{
    self, DeviceChange, HostDevice, HostDeviceWithStore, HostHeldProgram, HostHeldProgramWithStore,
    Program, ProgramChange, StartError,
};
use super::bindings::wasdk::inspect::trace::{
    self, ClipboardEvent, DeviceEvent, FileEvent, LinkEvent, MachineEvent, NetworkEvent,
    NotificationEvent, OutputChunk, PermissionEvent,
};
use super::{Device, InspectedPrograms, day_of};
use crate::access::{ProgramAccess, TraceEvent, TraceKind};
use crate::running::{HeldProgram, Programs, RunningProgram};
use crate::stream::stream_of;

/// What an inspector reaches: the programs the embedder gives, those the engine runs, the days
/// the engine's audit log keeps, and its own access, whose grants say whether it still holds the
/// role.
#[derive(Clone)]
pub(crate) struct InspectorReach {
    pub(crate) programs: Option<Arc<dyn InspectedPrograms>>,
    pub(crate) running: Programs,
    pub(crate) audit: Option<Arc<dyn AuditLog>>,
    pub(crate) access: ProgramAccess,
}

impl InspectorReach {
    /// The programs it sees, while it holds the role; none once its user took it back, or where
    /// the embedder gives none.
    fn programs(&self) -> Option<Arc<dyn InspectedPrograms>> {
        let holds = self.access.grants().role(Role::Inspector) == Some(Answer::Granted);
        self.programs.clone().filter(|_| holds)
    }

    /// The program the engine runs under `id`, while the role is held.
    fn running(&self, id: &str) -> Option<RunningProgram> {
        self.programs()?;
        self.running.named(id)
    }

    /// Resolves once its user takes the role back.
    fn role_taken(&self) -> impl Future<Output = ()> + Send + 'static {
        let reach = self.clone();
        async move {
            loop {
                // Asked for before the role is read, so a change between the two is heard.
                let changed = reach.access.grants().changed();
                if reach.programs().is_none() {
                    return;
                }
                changed.await;
            }
        }
    }

    /// `items`, until its user takes the role back.
    fn while_held<T: Send + 'static>(&self, items: BoxStream<'static, T>) -> BoxStream<'static, T> {
        items.take_until(self.role_taken().boxed()).boxed()
    }
}

/// What the inspection's interfaces read in a program's store.
pub(crate) struct InspectCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) reach: &'a InspectorReach,
}

pub(crate) trait InspectCtxView: Send {
    fn inspect_ctx(&mut self) -> InspectCtx<'_>;
}

struct HasInspect;

impl HasData for HasInspect {
    type Data<'a> = InspectCtx<'a>;
}

/// Adds every interface of `wasdk:inspect` the inspector imports: its devices, their programs'
/// audit summaries, what each uses now and their live traces.
pub(crate) fn add_to_linker<T: InspectCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    devices::add_to_linker::<_, HasInspect>(linker, T::inspect_ctx)?;
    audit::add_to_linker::<_, HasInspect>(linker, T::inspect_ctx)?;
    active_uses::add_to_linker::<_, HasInspect>(linker, T::inspect_ctx)?;
    trace::add_to_linker::<_, HasInspect>(linker, T::inspect_ctx)
}

/// What the store's reach answers, read without holding the store across an await.
fn reach_of<T: InspectCtxView + 'static>(accessor: &Accessor<T, HasInspect>) -> InspectorReach {
    accessor.with(|mut access| access.get().reach.clone())
}

/// `items`, as the program's stream.
fn stream_to<T, E>(
    access: Access<'_, T, HasInspect>,
    items: BoxStream<'static, E>,
) -> wasmtime::Result<StreamReader<E>>
where
    T: InspectCtxView + 'static,
    E: wasmtime::component::Lower + wasmtime::component::Lift + Send + Sync + 'static,
{
    stream_of(access, items, |_: &mut T, item| Ok(item))
}

impl devices::Host for InspectCtx<'_> {}
impl HostDevice for InspectCtx<'_> {}
impl HostHeldProgram for InspectCtx<'_> {}
impl audit::Host for InspectCtx<'_> {}
impl active_uses::Host for InspectCtx<'_> {}
impl trace::Host for InspectCtx<'_> {}

impl<T: InspectCtxView + 'static> devices::HostWithStore<T> for HasInspect {
    /// This device alone, while the role is held; no device reached yet is another.
    fn devices(mut access: Access<'_, T, Self>) -> wasmtime::Result<Vec<Resource<Device>>> {
        let ctx = access.get();
        if ctx.reach.programs().is_none() {
            return Ok(Vec::new());
        }
        Ok(vec![ctx.table.push(Device)?])
    }

    /// No other device is reached yet, so nothing comes, until the role is taken back.
    fn device_changes(
        mut access: Access<'_, T, Self>,
    ) -> wasmtime::Result<StreamReader<DeviceChange>> {
        let changes = access.get().reach.while_held(stream::pending().boxed());
        stream_to(access, changes)
    }
}

impl<T: InspectCtxView + 'static> HostDeviceWithStore<T> for HasInspect {
    fn name(
        mut access: Access<'_, T, Self>,
        _device: Resource<Device>,
    ) -> wasmtime::Result<String> {
        let programs = access.get().reach.programs();
        Ok(programs
            .map(|programs| programs.device_name())
            .unwrap_or_default())
    }

    fn is_this_device(
        _access: Access<'_, T, Self>,
        _device: Resource<Device>,
    ) -> wasmtime::Result<bool> {
        Ok(true)
    }

    async fn programs(
        accessor: &Accessor<T, Self>,
        _device: Resource<Device>,
    ) -> wasmtime::Result<Vec<Program>> {
        let asked = reach_of(accessor)
            .programs()
            .map(|programs| programs.programs());
        Ok(match asked {
            Some(asked) => asked.await,
            None => Vec::new(),
        })
    }

    fn program_changes(
        mut access: Access<'_, T, Self>,
        _device: Resource<Device>,
    ) -> wasmtime::Result<StreamReader<ProgramChange>> {
        let reach = access.get().reach;
        let changes = match reach.programs() {
            Some(programs) => reach.while_held(programs.program_changes()),
            None => stream::empty().boxed(),
        };
        stream_to(access, changes)
    }

    async fn icon(
        accessor: &Accessor<T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<Vec<u8>>> {
        let asked = reach_of(accessor)
            .programs()
            .map(|programs| programs.icon(&id));
        Ok(match asked {
            Some(asked) => asked.await,
            None => None,
        })
    }

    /// The program started held, while the role is held; a program the embedder knows not, for
    /// an inspector without it.
    async fn start_held(
        accessor: &Accessor<T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Result<Resource<HeldProgram>, StartError>> {
        let Some(programs) = reach_of(accessor).programs() else {
            return Ok(Err(StartError::Unknown));
        };
        let held = match programs.start_held(&id).await {
            Ok(held) => held,
            Err(error) => return Ok(Err(error)),
        };
        let pushed = accessor.with(|mut access| access.get().table.push(held))?;
        Ok(Ok(pushed))
    }

    /// The program ended, while the role is held; nothing for an inspector without it.
    async fn end(
        accessor: &Accessor<T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<()> {
        if let Some(programs) = reach_of(accessor).programs() {
            programs.end(&id).await;
        }
        Ok(())
    }

    fn drop(mut access: Access<'_, T, Self>, device: Resource<Device>) -> wasmtime::Result<()> {
        access.get().table.delete(device)?;
        Ok(())
    }
}

impl<T: InspectCtxView + 'static> HostHeldProgramWithStore<T> for HasInspect {
    /// The program goes on from its first instruction.
    fn drop(mut access: Access<'_, T, Self>, held: Resource<HeldProgram>) -> wasmtime::Result<()> {
        access.get().table.delete(held)?;
        Ok(())
    }
}

impl<T: InspectCtxView + 'static> audit::HostWithStore<T> for HasInspect {
    /// The days kept of the program, newest first, today's kept first where it runs, so they say
    /// all it did until now.
    async fn days(
        accessor: &Accessor<T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Vec<Day>> {
        let reach = reach_of(accessor);
        let (Some(_), Some(log)) = (reach.programs(), reach.audit.clone()) else {
            return Ok(Vec::new());
        };
        if let Some(program) = reach.running(&id) {
            program.keep_summary();
        }
        // The days are read from their files.
        let days = spawn_blocking(move || log.days(&id)).await;
        Ok(days.iter().map(day_of).collect())
    }
}

impl<T: InspectCtxView + 'static> active_uses::HostWithStore<T> for HasInspect {
    /// What the program uses now, and each change from then on, until it ends or the role is
    /// taken back.
    fn active_uses(
        mut access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<(Vec<ActiveUse>, StreamReader<ActiveUseChange>)>> {
        let reach = access.get().reach;
        let Some((now, changes)) = reach.running(&id).and_then(|program| program.active_uses())
        else {
            return Ok(None);
        };
        let changes = reach.while_held(changes);
        Ok(Some((now, stream_to(access, changes)?)))
    }
}

/// The events of `kind` the program `id` sends from now on, each as `wasdk:inspect`'s event of
/// its capability, until it ends or the role is taken back; none for a program that does not run.
fn traced<T, E>(
    mut access: Access<'_, T, HasInspect>,
    id: &str,
    kind: TraceKind,
    event_of: fn(TraceEvent) -> Option<E>,
) -> wasmtime::Result<Option<StreamReader<E>>>
where
    T: InspectCtxView + 'static,
    E: wasmtime::component::Lower + wasmtime::component::Lift + Send + Sync + 'static,
{
    let reach = access.get().reach;
    let Some(events) = reach.running(id).and_then(|program| program.trace(kind)) else {
        return Ok(None);
    };
    let events = events.filter_map(move |event| future::ready(event_of(event)));
    let events = reach.while_held(events.boxed());
    stream_to(access, events).map(Some)
}

impl<T: InspectCtxView + 'static> trace::HostWithStore<T> for HasInspect {
    fn network(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<NetworkEvent>>> {
        traced(access, &id, TraceKind::Network, |event| match event {
            TraceEvent::Network(event) => Some(event),
            _ => None,
        })
    }

    fn files(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<FileEvent>>> {
        traced(access, &id, TraceKind::Files, |event| match event {
            TraceEvent::Files(event) => Some(event),
            _ => None,
        })
    }

    fn clipboard(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<ClipboardEvent>>> {
        traced(access, &id, TraceKind::Clipboard, |event| match event {
            TraceEvent::Clipboard(event) => Some(event),
            _ => None,
        })
    }

    fn camera(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<DeviceEvent>>> {
        traced(access, &id, TraceKind::Camera, |event| match event {
            TraceEvent::Camera(event) => Some(event),
            _ => None,
        })
    }

    fn microphone(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<DeviceEvent>>> {
        traced(access, &id, TraceKind::Microphone, |event| match event {
            TraceEvent::Microphone(event) => Some(event),
            _ => None,
        })
    }

    fn machines(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<MachineEvent>>> {
        traced(access, &id, TraceKind::Machines, |event| match event {
            TraceEvent::Machines(event) => Some(event),
            _ => None,
        })
    }

    fn permissions(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<PermissionEvent>>> {
        traced(access, &id, TraceKind::Permissions, |event| match event {
            TraceEvent::Permissions(event) => Some(event),
            _ => None,
        })
    }

    fn links(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<LinkEvent>>> {
        traced(access, &id, TraceKind::Links, |event| match event {
            TraceEvent::Links(event) => Some(event),
            _ => None,
        })
    }

    fn notifications(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<NotificationEvent>>> {
        traced(access, &id, TraceKind::Notifications, |event| match event {
            TraceEvent::Notifications(event) => Some(event),
            _ => None,
        })
    }

    fn output(
        access: Access<'_, T, Self>,
        _device: Resource<Device>,
        id: String,
    ) -> wasmtime::Result<Option<StreamReader<OutputChunk>>> {
        traced(access, &id, TraceKind::Output, |event| match event {
            TraceEvent::Output(event) => Some(event),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use futures::future::BoxFuture;

    use waos_capabilities::ProgramGrants;

    use super::*;
    use crate::access::Answers;

    /// The embedder's programs, of which the test reads none.
    struct NoPrograms;

    impl InspectedPrograms for NoPrograms {
        fn device_name(&self) -> String {
            String::new()
        }

        fn programs(&self) -> BoxFuture<'static, Vec<Program>> {
            future::ready(Vec::new()).boxed()
        }

        fn program_changes(&self) -> BoxStream<'static, ProgramChange> {
            stream::pending().boxed()
        }

        fn icon(&self, _id: &str) -> BoxFuture<'static, Option<Vec<u8>>> {
            future::ready(None).boxed()
        }

        fn start_held(&self, _id: &str) -> BoxFuture<'static, Result<HeldProgram, StartError>> {
            future::ready(Err(StartError::Unknown)).boxed()
        }

        fn end(&self, _id: &str) -> BoxFuture<'static, ()> {
            future::ready(()).boxed()
        }
    }

    #[test]
    fn what_an_inspector_sees_ends_as_its_role_is_taken_back() {
        let answers = Arc::new(Answers::default());
        answers.set_role(Role::Inspector, Answer::Granted);
        let reach = InspectorReach {
            programs: Some(Arc::new(NoPrograms)),
            running: Programs::default(),
            audit: None,
            access: answers.access(),
        };
        let mut held = reach.while_held(stream::pending::<()>().boxed());
        assert!(
            held.next().now_or_never().is_none(),
            "open while the role is held"
        );
        assert!(reach.programs().is_some());
        answers.set_role(Role::Inspector, Answer::Denied);
        assert_eq!(held.next().now_or_never(), Some(None), "ended at once");
        assert!(reach.programs().is_none(), "and nothing seen from then on");
    }
}
