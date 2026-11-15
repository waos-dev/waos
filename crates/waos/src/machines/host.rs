//! The code serving `wasdk:platform/machines` to a program: the machines it holds, a pick asked of
//! its user, a machine's root as `wasi:filesystem`'s own descriptor, which the files serve, and a
//! command built with setters and run through the machine's provider, with pipes or in a terminal,
//! each output taken once as a stream of its own.

use std::path::Path;
use std::sync::Arc;

use waos_capabilities::{
    Command, ExecError as ProvidedExecError, ExitStatus as ProvidedExitStatus, MachineFiles,
    Signal as ProvidedSignal, TerminalSize as ProvidedTerminalSize,
};
use wasmtime::component::{
    Access, Accessor, HasData, Linker, Resource, ResourceTable, StreamReader,
};

use super::{Child, Machine, Machines, Pipes, ProgramMachines, Terminal};
use crate::bindings::wasdk::platform::machines::{
    self, ExecError, ExitStatus, HostChild, HostChildWithStore, HostCommand, HostMachine,
    HostMachineWithStore, HostMachines, HostMachinesWithStore, HostPipes, HostPipesWithStore,
    HostTerminal, HostTerminalWithStore, Signal, TerminalSize,
};
use crate::files::{Descriptor, folder_of_machine, machine_paths};
use crate::stream::stream_of_chunks;

/// `command` as `machine` takes it: for this computer, the folder it starts in by the path the
/// computer names it, the program's `/c/Users` being `C:\\Users` on Windows (`machine_paths`); a
/// folder that names none is left as the program gave it.
fn as_the_machine_takes(machine: &Machine, mut command: Command) -> Command {
    let MachineFiles::Local(_) = machine.files();
    let on_machine = command
        .working_directory
        .as_deref()
        .and_then(|folder| machine_paths::on_this_computer(Path::new(folder)));
    if let Some(folder) = on_machine {
        command.working_directory = Some(folder.to_string_lossy().into_owned());
    }
    command
}

/// What the machines interface reads and writes in a program's store.
pub(crate) struct MachinesCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) machines: &'a ProgramMachines,
}

pub(crate) trait MachinesCtxView: Send {
    fn machines_ctx(&mut self) -> MachinesCtx<'_>;
}

struct HasMachines;

impl HasData for HasMachines {
    type Data<'a> = MachinesCtx<'a>;
}

pub(crate) fn add_to_linker<T: MachinesCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    machines::add_to_linker::<_, HasMachines>(linker, T::machines_ctx)
}

impl machines::Host for MachinesCtx<'_> {
    fn get_machines(&mut self) -> wasmtime::Result<Option<Resource<Machines>>> {
        if !self.machines.any_provided() {
            return Ok(None);
        }
        Ok(Some(self.table.push(Machines)?))
    }
}

impl HostMachines for MachinesCtx<'_> {
    fn granted(
        &mut self,
        _machines: Resource<Machines>,
    ) -> wasmtime::Result<Vec<Resource<Machine>>> {
        let granted = self.machines.granted();
        Ok(granted
            .into_iter()
            .map(|machine| self.table.push(machine))
            .collect::<Result<_, _>>()?)
    }

    fn drop(&mut self, machines: Resource<Machines>) -> wasmtime::Result<()> {
        self.table.delete(machines)?;
        Ok(())
    }
}

impl<T: MachinesCtxView + 'static> HostMachinesWithStore<T> for HasMachines {
    async fn pick(
        accessor: &Accessor<T, Self>,
        _machines: Resource<Machines>,
    ) -> wasmtime::Result<Option<Resource<Machine>>> {
        let picking = accessor.with(|mut access| access.get().machines.pick());
        let Some(machine) = picking.await else {
            return Ok(None);
        };
        accessor.with(|mut access| Ok(Some(access.get().table.push(machine)?)))
    }
}

impl HostMachine for MachinesCtx<'_> {
    fn name(&mut self, machine: Resource<Machine>) -> wasmtime::Result<String> {
        Ok(self.table.get(&machine)?.name())
    }

    /// A root that cannot be opened traps: the machine's own root is always there.
    fn root(&mut self, machine: Resource<Machine>) -> wasmtime::Result<Resource<Descriptor>> {
        let MachineFiles::Local(folder) = self.table.get(&machine)?.files();
        let root = folder_of_machine(&folder)
            .map_err(|error| wasmtime::format_err!("the machine's root {folder:?}: {error}"))?;
        Ok(self.table.push(root)?)
    }

    fn drop(&mut self, machine: Resource<Machine>) -> wasmtime::Result<()> {
        self.table.delete(machine)?;
        Ok(())
    }
}

impl<T: MachinesCtxView + 'static> HostMachineWithStore<T> for HasMachines {
    async fn exec(
        accessor: &Accessor<T, Self>,
        machine: Resource<Machine>,
        command: Resource<Command>,
    ) -> wasmtime::Result<Result<(Resource<Child>, Resource<Pipes>), ExecError>> {
        let starting = accessor.with(|mut access| -> wasmtime::Result<_> {
            let context = access.get();
            let machine = Arc::clone(context.table.get(&machine)?);
            let command = as_the_machine_takes(&machine, context.table.get(&command)?.clone());
            let run = context.machines.hold_run(&machine, &command, false);
            Ok(run.map(|run| (machine.exec(command), run)))
        })?;
        let (starting, run) = match starting {
            Ok(held) => held,
            Err(refused) => return Ok(Err(error_of(refused))),
        };
        let started = match starting.await {
            Ok(started) => started,
            Err(error) => return Ok(Err(error_of(error))),
        };
        accessor.with(|mut access| {
            let table = &mut *access.get().table;
            let child = table.push(Child::new(started.child, run))?;
            let pipes = table.push(Pipes::new(started.input, started.output, started.errors))?;
            Ok(Ok((child, pipes)))
        })
    }

    async fn exec_in_terminal(
        accessor: &Accessor<T, Self>,
        machine: Resource<Machine>,
        command: Resource<Command>,
        size: TerminalSize,
    ) -> wasmtime::Result<Result<(Resource<Child>, Resource<Terminal>), ExecError>> {
        let starting = accessor.with(|mut access| -> wasmtime::Result<_> {
            let context = access.get();
            let machine = Arc::clone(context.table.get(&machine)?);
            let command = as_the_machine_takes(&machine, context.table.get(&command)?.clone());
            let run = context.machines.hold_run(&machine, &command, true);
            Ok(run.map(|run| (machine.exec_in_terminal(command, size_of(size)), run)))
        })?;
        let (starting, run) = match starting {
            Ok(held) => held,
            Err(refused) => return Ok(Err(error_of(refused))),
        };
        let started = match starting.await {
            Ok(started) => started,
            Err(error) => return Ok(Err(error_of(error))),
        };
        accessor.with(|mut access| {
            let table = &mut *access.get().table;
            let child = table.push(Child::new(started.child, run))?;
            let terminal = table.push(Terminal::new(started.terminal, started.output))?;
            Ok(Ok((child, terminal)))
        })
    }
}

impl HostCommand for MachinesCtx<'_> {
    fn new(&mut self, program: String) -> wasmtime::Result<Resource<Command>> {
        Ok(self.table.push(Command::new(program))?)
    }

    fn add_argument(
        &mut self,
        command: Resource<Command>,
        argument: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&command)?.arguments.push(argument);
        Ok(())
    }

    fn working_directory(
        &mut self,
        command: Resource<Command>,
        path: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&command)?.working_directory = Some(path);
        Ok(())
    }

    fn variable(
        &mut self,
        command: Resource<Command>,
        name: String,
        value: String,
    ) -> wasmtime::Result<()> {
        let variables = &mut self.table.get_mut(&command)?.variables;
        variables.retain(|(kept, _)| *kept != name);
        variables.push((name, value));
        Ok(())
    }

    fn drop(&mut self, command: Resource<Command>) -> wasmtime::Result<()> {
        self.table.delete(command)?;
        Ok(())
    }
}

impl HostChild for MachinesCtx<'_> {
    fn signal(&mut self, child: Resource<Child>, signal: Signal) -> wasmtime::Result<()> {
        self.table.get(&child)?.started().signal(signal_of(signal));
        Ok(())
    }

    fn drop(&mut self, child: Resource<Child>) -> wasmtime::Result<()> {
        self.table.delete(child)?;
        Ok(())
    }
}

impl<T: MachinesCtxView + 'static> HostChildWithStore<T> for HasMachines {
    async fn exit_status(
        accessor: &Accessor<T, Self>,
        child: Resource<Child>,
    ) -> wasmtime::Result<ExitStatus> {
        let ending = accessor.with(|mut access| -> wasmtime::Result<_> {
            Ok(access.get().table.get(&child)?.started().exit_status())
        })?;
        Ok(exit_status_of(ending.await))
    }
}

impl HostPipes for MachinesCtx<'_> {
    fn close_input(&mut self, pipes: Resource<Pipes>) -> wasmtime::Result<()> {
        self.table.get(&pipes)?.input.close();
        Ok(())
    }

    fn drop(&mut self, pipes: Resource<Pipes>) -> wasmtime::Result<()> {
        self.table.delete(pipes)?;
        Ok(())
    }
}

impl<T: MachinesCtxView + 'static> HostPipesWithStore<T> for HasMachines {
    async fn write(
        accessor: &Accessor<T, Self>,
        pipes: Resource<Pipes>,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<(), ExecError>> {
        let writing = accessor.with(|mut access| -> wasmtime::Result<_> {
            Ok(access.get().table.get(&pipes)?.input.write(data))
        })?;
        Ok(writing.await.map_err(error_of))
    }

    fn output(
        mut access: Access<T, Self>,
        pipes: Resource<Pipes>,
    ) -> wasmtime::Result<Option<StreamReader<u8>>> {
        let output = access.get().table.get_mut(&pipes)?.output.take();
        output
            .map(|chunks| stream_of_chunks(access, chunks))
            .transpose()
    }

    fn errors(
        mut access: Access<T, Self>,
        pipes: Resource<Pipes>,
    ) -> wasmtime::Result<Option<StreamReader<u8>>> {
        let errors = access.get().table.get_mut(&pipes)?.errors.take();
        errors
            .map(|chunks| stream_of_chunks(access, chunks))
            .transpose()
    }
}

impl HostTerminal for MachinesCtx<'_> {
    fn resize(&mut self, terminal: Resource<Terminal>, size: TerminalSize) -> wasmtime::Result<()> {
        self.table.get(&terminal)?.terminal.resize(size_of(size));
        Ok(())
    }

    fn drop(&mut self, terminal: Resource<Terminal>) -> wasmtime::Result<()> {
        self.table.delete(terminal)?;
        Ok(())
    }
}

impl<T: MachinesCtxView + 'static> HostTerminalWithStore<T> for HasMachines {
    async fn write(
        accessor: &Accessor<T, Self>,
        terminal: Resource<Terminal>,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<(), ExecError>> {
        let writing = accessor.with(|mut access| -> wasmtime::Result<_> {
            Ok(access.get().table.get(&terminal)?.terminal.write(data))
        })?;
        Ok(writing.await.map_err(error_of))
    }

    fn output(
        mut access: Access<T, Self>,
        terminal: Resource<Terminal>,
    ) -> wasmtime::Result<Option<StreamReader<u8>>> {
        let output = access.get().table.get_mut(&terminal)?.output.take();
        output
            .map(|chunks| stream_of_chunks(access, chunks))
            .transpose()
    }
}

fn size_of(size: TerminalSize) -> ProvidedTerminalSize {
    ProvidedTerminalSize {
        columns: size.columns,
        rows: size.rows,
        width_in_pixels: size.width_in_pixels,
        height_in_pixels: size.height_in_pixels,
    }
}

fn signal_of(signal: Signal) -> ProvidedSignal {
    match signal {
        Signal::Hangup => ProvidedSignal::Hangup,
        Signal::Interrupt => ProvidedSignal::Interrupt,
        Signal::Terminate => ProvidedSignal::Terminate,
        Signal::Kill => ProvidedSignal::Kill,
    }
}

fn exit_status_of(status: ProvidedExitStatus) -> ExitStatus {
    match status {
        ProvidedExitStatus::Exited(code) => ExitStatus::Exited(code),
        ProvidedExitStatus::Signaled(name) => ExitStatus::Signaled(name),
        ProvidedExitStatus::Unknown => ExitStatus::Unknown,
    }
}

fn error_of(error: ProvidedExecError) -> ExecError {
    match error {
        ProvidedExecError::Refused(why) => ExecError::Refused(why),
        ProvidedExecError::Failed(why) => ExecError::Failed(why),
    }
}
