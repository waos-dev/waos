//! `wasdk:platform/machines`: the machines a program acts on as their user (the processes
//! proposal's decisions 1 to 6), each the provider's in its environment ([`Machines`], one per
//! kind), which give its files from its root and run its commands.
//!
//! The engine keeps which machines the program holds: those its user picked for it, kept among
//! its grants by their ids and read at each `granted`; a machine handed to the program, granted or
//! picked, stays its until it ends, as an open file does, so a command run on it is told through
//! the program's access (`access`), never refused, and held from its start until it ends, watched
//! on a thread of its own whatever the program does with its handle ([`DecidedCommand`]). A command ends with
//! the program that started it, as a tree beneath it ([`StartedCommands`]): dropping its handle
//! leaves it running, the program ending ends it, whichever of its instances started it. A pick
//! is asked of the user through the shell's [`ShellMachinePicker`], which shows what the providers
//! offer, unless the user set the machine
//! the picker answers among the program's settings: then that machine is the pick, and nothing is
//! shown; one no provider offers now is no pick, as a cancel is, never another. The program never
//! sees the machines it was not given. The interface is served to a program by `host`.
//!
//! The invariant: a program reaches a machine only once its user picked it; what the machine runs
//! then runs as its user, outside the program's sandbox, which nothing here confines, and never
//! outlives the program.

pub(crate) mod host;

use std::future::{Future, ready};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use waos_capabilities::{
    ChildInput, Command, ExecError, Machine as ProvidedMachine, MachineChoice, Machines as Provider,
};

use crate::access::{ActiveUseKind, Asked, Happened, ProgramAccess, Refusal};
use crate::asking::{self, Outcome};
use crate::running::RunningProgram;
use crate::shell::ShellMachinePicker;
use crate::sync::locked;

/// `machines.machines`: the program's machines, which its store holds.
pub struct Machines;

/// `machines.machine`: one machine the program holds.
pub type Machine = Arc<dyn ProvidedMachine>;

/// `machines.child`: a command the program started on a machine, which its program's
/// [`StartedCommands`] holds too.
pub type Child = Arc<dyn waos_capabilities::Child>;

/// A command decided and not started yet: what it is as an active use once it starts, and the program's
/// commands, which it joins.
pub(crate) struct DecidedCommand {
    access: ProgramAccess,
    active_use: ActiveUseKind,
    commands: StartedCommands,
}

/// The commands a program started, each ended as the last of the program's instances goes with
/// it: its main one and every worker hold it. Every clone is the same.
#[derive(Clone, Default)]
pub(crate) struct StartedCommands(Arc<Started>);

#[derive(Default)]
struct Started(Mutex<Vec<StartedCommand>>);

/// A command started, and whether it has ended.
struct StartedCommand {
    child: Child,
    ended: Arc<AtomicBool>,
}

/// `machines.pipes`: a command's input, and its output and errors until the program takes them.
pub struct Pipes {
    input: Box<dyn ChildInput>,
    output: Option<BoxStream<'static, Vec<u8>>>,
    errors: Option<BoxStream<'static, Vec<u8>>>,
}

/// `machines.terminal`: a command's terminal, and its output until the program takes it.
pub struct Terminal {
    terminal: Box<dyn waos_capabilities::Terminal>,
    output: Option<BoxStream<'static, Vec<u8>>>,
}

/// What a program holds of machines: the providers its environment gives it, what its user
/// picked for it, and the shell's picker.
pub(crate) struct ProgramMachines {
    providers: Vec<Arc<dyn Provider>>,
    /// Where the machines the user picked are kept, and each run told.
    access: ProgramAccess,
    picker: Option<Arc<dyn ShellMachinePicker>>,
    program: RunningProgram,
    /// The commands the program started, from any of its instances, which end with it.
    commands: StartedCommands,
}

impl ProgramMachines {
    /// `program`'s machines, from `providers`, as the grants `access` reads keep them and `picker`
    /// asks the user, the commands run on them joining `commands`.
    pub(crate) fn new(
        providers: Vec<Arc<dyn Provider>>,
        access: ProgramAccess,
        picker: Option<Arc<dyn ShellMachinePicker>>,
        (program, commands): (RunningProgram, StartedCommands),
    ) -> ProgramMachines {
        ProgramMachines {
            providers,
            access,
            picker,
            program,
            commands,
        }
    }

    /// Where the program's uses of its machines are told, and what it holds of them kept.
    pub(crate) fn access(&self) -> &ProgramAccess {
        &self.access
    }

    /// Whether the program may be given any machine.
    pub(crate) fn any_provided(&self) -> bool {
        !self.providers.is_empty()
    }

    /// The machines the user picked for the program, as its grants keep them now, each a
    /// provider still offers.
    pub(crate) fn granted(&self) -> Vec<Machine> {
        let providers = self.providers.clone();
        self.access
            .grants()
            .machines()
            .iter()
            .filter_map(|id| machine_of(&providers, id))
            .collect()
    }

    /// Asks the user to pick one of the machines the providers offer, or takes the one the user
    /// set as the answer, and resolves to it, kept among the program's grants; to none when the
    /// user cancels, nobody can ask, or no provider offers the one set. If the user sets the
    /// answer while the picker waits, the picker's future is dropped and the stored answer wins.
    pub(crate) fn pick(&self) -> impl Future<Output = Option<Machine>> + Send + 'static + use<> {
        let grants = self.access.clone_grants();
        let changed = grants.changed();
        let picked: BoxFuture<'static, Option<String>> =
            match (grants.machine_answer(), &self.picker) {
                (Some(set), _) => ready(Some(set)).boxed(),
                (None, Some(picker)) => {
                    let shown = picker.pick(&self.program, self.offered());
                    async move {
                        match asking::or_setting(
                            grants,
                            changed,
                            |grants| grants.machine_answer(),
                            shown,
                        )
                        .await
                        {
                            Outcome::Prompt(picked) => picked,
                            Outcome::Setting(set) => Some(set),
                        }
                    }
                    .boxed()
                }
                (None, None) => ready(None).boxed(),
            };
        let (providers, access) = (self.providers.clone(), self.access.clone());
        let program = self.program.name().to_owned();
        async move {
            let id = picked.await?;
            let Some(machine) = machine_of(&providers, &id) else {
                tracing::warn!(
                    program,
                    machine = id,
                    "no provider offers the machine picked"
                );
                return None;
            };
            access.tell(Happened::MachinePicked { machine: &id });
            access.grants().keep_machine(&id);
            Some(machine)
        }
    }

    /// The run of `command` on `machine`, in a terminal where `in_terminal`, decided and told,
    /// to be held once it starts.
    pub(crate) fn decide_run(
        &self,
        machine: &Machine,
        command: &Command,
        in_terminal: bool,
    ) -> Result<DecidedCommand, ExecError> {
        let machine = machine.name();
        decide_run(&self.access, &machine, command, in_terminal)?;
        let run = DecidedCommand::of(&self.access, machine, command, in_terminal);
        Ok(DecidedCommand {
            commands: self.commands.clone(),
            ..run
        })
    }

    /// What every provider offers, in the providers' order.
    fn offered(&self) -> Vec<MachineChoice> {
        self.providers
            .iter()
            .flat_map(|provider| provider.offered())
            .collect()
    }
}

/// The run of `command` on the machine named `machine`, decided and told: a machine handed to the
/// program is its whole, so no grant refuses what it runs.
fn decide_run(
    access: &ProgramAccess,
    machine: &str,
    command: &Command,
    in_terminal: bool,
) -> Result<(), ExecError> {
    let asked = Asked::RunCommand {
        machine,
        command,
        in_terminal,
    };
    access
        .decide(asked, |_grants| Ok::<_, Refusal>(()))
        .map_err(|refusal| {
            ExecError::Refused(format!(
                "refused by the program's setting {}",
                refusal.setting
            ))
        })
}

impl DecidedCommand {
    /// The run of `command` on the machine named `machine` for the program `access` is of, among
    /// commands of its own.
    fn of(
        access: &ProgramAccess,
        machine: String,
        command: &Command,
        in_terminal: bool,
    ) -> DecidedCommand {
        DecidedCommand {
            access: access.clone(),
            active_use: ActiveUseKind::Command {
                machine,
                command: command.program.clone(),
                in_terminal,
            },
            commands: StartedCommands::default(),
        }
    }

    /// The command `child` is on joins the program's commands, which end with it, and its run is
    /// held from now until it ends, watched on a thread of its own, since the program that ran it
    /// may end first and end it; where no thread starts, the run is held no longer.
    pub(crate) fn started(self, child: &Child) {
        let held = self.access.hold(self.active_use);
        let ended = Arc::new(AtomicBool::new(false));
        self.commands.add(child, Arc::clone(&ended));
        let exit = child.exit_status();
        let watching = std::thread::Builder::new()
            .name("run's end".to_owned())
            .spawn(move || {
                futures::executor::block_on(exit);
                ended.store(true, Ordering::Release);
                drop(held);
            });
        if let Err(error) = watching {
            tracing::warn!(%error, "no thread watches a command's end");
        }
    }
}

impl StartedCommands {
    /// Adds `child`, a command started, which has ended once `ended` says so; those ended are let
    /// go as the list fills.
    fn add(&self, child: &Child, ended: Arc<AtomicBool>) {
        let mut started = locked(&self.0.0);
        if started.len() == started.capacity() {
            started.retain(|command| !command.ended.load(Ordering::Acquire));
        }
        started.push(StartedCommand {
            child: Arc::clone(child),
            ended,
        });
    }
}

impl Drop for Started {
    /// Ends every command that has not ended: the program they are beneath has.
    fn drop(&mut self) {
        let started = self.0.get_mut().unwrap_or_else(PoisonError::into_inner);
        for command in started.drain(..) {
            if !command.ended.load(Ordering::Acquire) {
                command.child.end();
            }
        }
    }
}

/// The machine `id` names, among what `providers` offer.
fn machine_of(providers: &[Arc<dyn Provider>], id: &str) -> Option<Machine> {
    providers.iter().find_map(|provider| provider.machine(id))
}

impl Pipes {
    fn new(
        input: Box<dyn ChildInput>,
        output: BoxStream<'static, Vec<u8>>,
        errors: BoxStream<'static, Vec<u8>>,
    ) -> Pipes {
        Pipes {
            input,
            output: Some(output),
            errors: Some(errors),
        }
    }
}

impl Terminal {
    fn new(
        terminal: Box<dyn waos_capabilities::Terminal>,
        output: BoxStream<'static, Vec<u8>>,
    ) -> Terminal {
        Terminal {
            terminal,
            output: Some(output),
        }
    }
}

#[cfg(test)]
mod tests {
    use waos_capabilities::ExitStatus;

    use super::*;
    use crate::access::Told;
    use crate::permissions::NothingGranted;

    #[test]
    fn a_run_is_told_allowed_whatever_is_granted() {
        let access = ProgramAccess::new(Arc::new(NothingGranted));
        assert!(decide_run(&access, "This Mac", &Command::new("git"), false).is_ok());
        assert!(
            access.refusals().is_empty(),
            "a machine given is given whole"
        );
        assert!(matches!(access.told()[..], [Told::Allowed(..)]));
    }

    /// A child whose command ends when the test says, or when it is ended.
    struct Ending {
        exit: futures::future::Shared<BoxFuture<'static, ExitStatus>>,
        ended: Mutex<Option<futures::channel::oneshot::Sender<ExitStatus>>>,
    }

    impl Ending {
        fn child() -> Arc<Ending> {
            let (end, exit) = futures::channel::oneshot::channel();
            let exit = exit
                .map(|status| status.unwrap_or(ExitStatus::Unknown))
                .boxed();
            Arc::new(Ending {
                exit: exit.shared(),
                ended: Mutex::new(Some(end)),
            })
        }

        fn ends(&self, status: ExitStatus) {
            if let Some(end) = self.ended.lock().unwrap().take() {
                let _ = end.send(status);
            }
        }
    }

    impl waos_capabilities::Child for Ending {
        fn signal(&self, _signal: waos_capabilities::Signal) {}

        fn end(&self) {
            self.ends(ExitStatus::Signaled("TERM".to_owned()));
        }

        fn exit_status(&self) -> BoxFuture<'static, ExitStatus> {
            self.exit.clone().boxed()
        }
    }

    /// Waits until `access` holds nothing, or fails the test.
    fn until_nothing_held(access: &ProgramAccess, why: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !access.uses_but_open_files().is_empty() {
            assert!(std::time::Instant::now() < deadline, "{why}");
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_run_is_held_until_its_command_ends_whatever_becomes_of_its_child() {
        let access = ProgramAccess::new(Arc::new(NothingGranted));
        let ending = Ending::child();
        let child: Child = ending.clone();
        DecidedCommand::of(&access, "This Mac".to_owned(), &Command::new("git"), false)
            .started(&child);
        drop(child);
        assert_eq!(
            access.uses_but_open_files().len(),
            1,
            "running after its child was let go"
        );
        ending.ends(ExitStatus::Exited(0));
        until_nothing_held(&access, "ended with its command");
    }

    #[test]
    fn a_programs_commands_end_as_the_last_of_its_instances_goes() {
        let access = ProgramAccess::new(Arc::new(NothingGranted));
        let commands = StartedCommands::default();
        let worker = commands.clone();
        let (still_running, done) = (Ending::child(), Ending::child());
        for ending in [&still_running, &done] {
            let run = DecidedCommand {
                commands: worker.clone(),
                ..DecidedCommand::of(
                    &access,
                    "This Mac".to_owned(),
                    &Command::new("sleep"),
                    false,
                )
            };
            run.started(&(Arc::clone(ending) as Child));
        }
        done.ends(ExitStatus::Exited(0));
        drop(worker);
        assert!(
            still_running.exit.clone().now_or_never().is_none(),
            "the main instance runs on"
        );
        drop(commands);
        assert_eq!(
            futures::executor::block_on(still_running.exit.clone()),
            ExitStatus::Signaled("TERM".to_owned()),
            "ended with the program"
        );
        until_nothing_held(&access, "its run ends as its command does");
    }
}
