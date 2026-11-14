//! One session: a shell run in a terminal on the machine the user picked, the screen its output
//! draws, and what the user selected on it. A window shows one session.
//!
//! The invariant a later editor will break: the session's entity holds the machine's terminal, and
//! dropping the terminal hangs the shell up, so nothing that outlives the window may hold the
//! entity: the tasks that read the shell's output and wait for its end hold it weakly.

use std::rc::Rc;

use waft_foundation::{App, AsyncApp, Entity, WeakEntity};
use wasdk::StreamReader;
use wasdk::machines::{self, Child, ExitStatus, Machine, Terminal, TerminalSize};

use crate::screen::{GridSize, Screen};
use crate::selection::Selection;
use crate::shell::login_shell;

/// The size a session's screen has until its window has been laid out.
const FIRST_SIZE: GridSize = GridSize {
    columns: 80,
    rows: 24,
};

/// How many bytes of the shell's output are read at once.
const READ_SIZE: usize = 16 * 1024;

/// Where a session is between asking for a machine and its shell's end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// WaOS gives this program no machines at all.
    NoMachines,
    /// The user has picked no machine for the program yet.
    NeedsMachine,
    /// The shell is being started.
    Starting,
    /// The shell runs.
    Running,
    /// The shell ended as its user asked, with `exit`: its window has nothing left to show.
    Left,
    /// The shell ended otherwise, as the text says.
    Ended(String),
    /// The shell did not start, for the reason given.
    Failed(String),
}

pub(crate) struct Session {
    pub(crate) screen: Screen,
    pub(crate) stage: Stage,
    pub(crate) selection: Option<Selection>,
    /// What an input method is composing, shown at the cursor until it is committed and typed.
    pub(crate) composing: String,
    /// The name of the machine the shell runs on, as the user knows it.
    pub(crate) machine_name: Option<String>,
    /// The size of a cell in physical pixels, which the machine's terminal is told with the
    /// screen's size.
    cell_pixels: (u16, u16),
    terminal: Option<Rc<Terminal>>,
    /// What was typed and not yet written to the terminal, in order.
    unsent: Vec<u8>,
    /// Whether a task is writing `unsent`.
    sending: bool,
}

impl Session {
    fn new() -> Session {
        Session {
            screen: Screen::new(FIRST_SIZE),
            stage: Stage::Starting,
            selection: None,
            composing: String::new(),
            machine_name: None,
            cell_pixels: (0, 0),
            terminal: None,
            unsent: Vec::new(),
            sending: false,
        }
    }

    /// The window's title: the one the shell gave, or else the machine's name.
    pub(crate) fn title(&self) -> Option<&str> {
        self.screen.title().or(self.machine_name.as_deref())
    }

    /// Whether the shell runs, so that what is typed reaches it.
    pub(crate) fn is_running(&self) -> bool {
        self.stage == Stage::Running
    }

    fn terminal_size(&self) -> TerminalSize {
        let size = self.screen.size();
        TerminalSize {
            columns: size.columns,
            rows: size.rows,
            width_in_pixels: size.columns.saturating_mul(self.cell_pixels.0),
            height_in_pixels: size.rows.saturating_mul(self.cell_pixels.1),
        }
    }
}

/// A new session, its shell started on the machine the user picked for the program, where they
/// have picked one.
pub(crate) fn open(app: &mut App) -> Entity<Session> {
    let session = app.new_entity(|_cx| Session::new());
    start(app, &session);
    session
}

/// Starts the shell on the first machine the program holds; with none, the session waits for
/// the user to choose one.
pub(crate) fn start(app: &mut App, session: &Entity<Session>) {
    let Some(machines) = machines::get_machines() else {
        set_stage(app, session, Stage::NoMachines);
        return;
    };
    match machines.granted().into_iter().next() {
        Some(machine) => run_shell(app, session, machine),
        None => set_stage(app, session, Stage::NeedsMachine),
    }
}

/// Asks the user to pick a machine, and starts the shell on the one picked.
pub(crate) fn choose_machine(app: &mut App, session: &Entity<Session>) {
    let Some(machines) = machines::get_machines() else {
        return;
    };
    let session = session.downgrade();
    app.spawn(async move |cx| {
        let Some(machine) = machines.pick().await else {
            return;
        };
        cx.update(|app| {
            if let Some(session) = session.upgrade() {
                run_shell(app, &session, machine);
            }
        });
    });
}

/// Types `bytes` into the shell, after what was typed before, and shows the screen's end, where
/// what is typed appears.
pub(crate) fn type_bytes(app: &mut App, session: &Entity<Session>, bytes: &[u8]) {
    let starts_sending = session.update(app, |session, cx| {
        if !session.is_running() || bytes.is_empty() {
            return false;
        }
        session.unsent.extend_from_slice(bytes);
        session.selection = None;
        session.screen.set_scroll_offset(0);
        cx.notify();
        !std::mem::replace(&mut session.sending, true)
    });
    if starts_sending {
        send_unsent(app, session.downgrade());
    }
}

/// Shows `composing`, what an input method is composing, at the cursor.
pub(crate) fn set_composing(app: &mut App, session: &Entity<Session>, composing: &str) {
    session.update(app, |session, cx| {
        if session.composing != composing {
            session.composing = composing.to_owned();
            cx.notify();
        }
    });
}

/// Scrolls the screen by `lines`, up into the scrollback where positive.
pub(crate) fn scroll_by(app: &mut App, session: &Entity<Session>, lines: isize) {
    session.update(app, |session, cx| {
        let offset = session.screen.scroll_offset();
        session.screen.scroll_by(lines);
        if session.screen.scroll_offset() != offset {
            cx.notify();
        }
    });
}

/// Selects `selection`, or nothing.
pub(crate) fn select(app: &mut App, session: &Entity<Session>, selection: Option<Selection>) {
    session.update(app, |session, cx| {
        if session.selection != selection {
            session.selection = selection;
            cx.notify();
        }
    });
}

/// Forgets the scrollback and has the shell draw its screen again, as Control-L asks it.
pub(crate) fn clear(app: &mut App, session: &Entity<Session>) {
    session.update(app, |session, cx| {
        session.screen.clear_scrollback();
        session.selection = None;
        cx.notify();
    });
    type_bytes(app, session, b"\x0c");
}

/// Lets go of the machine's terminal, which hangs the shell up: what a window does as it closes.
pub(crate) fn hang_up(app: &mut App, session: &Entity<Session>) {
    session.update(app, |session, _cx| {
        session.terminal = None;
        session.unsent.clear();
    });
}

/// Gives the screen `size`, with cells of `cell_pixels` physical pixels, and tells the shell.
pub(crate) fn resize(
    app: &mut App,
    session: &Entity<Session>,
    size: GridSize,
    cell_pixels: (u16, u16),
) {
    session.update(app, |session, cx| {
        if session.screen.size() == size && session.cell_pixels == cell_pixels {
            return;
        }
        session.screen.resize(size);
        session.cell_pixels = cell_pixels;
        session.selection = None;
        if let Some(terminal) = &session.terminal {
            terminal.resize(session.terminal_size());
        }
        cx.notify();
    });
}

fn set_stage(app: &mut App, session: &Entity<Session>, stage: Stage) {
    session.update(app, |session, cx| {
        session.stage = stage;
        cx.notify();
    });
}

/// Runs the user's shell on `machine` in a terminal of the screen's size, then draws what it
/// writes and waits for its end.
fn run_shell(app: &mut App, session: &Entity<Session>, machine: Machine) {
    let size = session.update(app, |session, cx| {
        session.machine_name = Some(machine.name());
        session.stage = Stage::Starting;
        cx.notify();
        session.terminal_size()
    });
    let session = session.downgrade();
    app.spawn(
        async move |cx| match machine.exec_in_terminal(&login_shell(), size).await {
            Ok((child, terminal)) => shell_started(cx, &session, child, terminal),
            Err(error) => cx.update(|app| {
                if let Some(session) = session.upgrade() {
                    set_stage(app, &session, Stage::Failed(why(&error)));
                }
            }),
        },
    );
}

/// Keeps the started shell's terminal in the session, at the size the screen has by now, and
/// starts the tasks that read its output and wait for its end.
fn shell_started(
    cx: &mut AsyncApp,
    session: &WeakEntity<Session>,
    child: Child,
    terminal: Terminal,
) {
    cx.update(|app| {
        let Some(strong) = session.upgrade() else {
            return;
        };
        let output = terminal.output();
        strong.update(app, |session, cx| {
            terminal.resize(session.terminal_size());
            session.terminal = Some(Rc::new(terminal));
            session.stage = Stage::Running;
            cx.notify();
        });
        if let Some(output) = output {
            draw_output(app, session.clone(), output);
        }
        wait_for_end(app, session.clone(), child);
    });
}

/// Draws what the shell writes on the session's screen, and types back what it asks of the
/// terminal, until its terminal closes or the session is gone.
fn draw_output(app: &mut App, session: WeakEntity<Session>, mut output: StreamReader<u8>) {
    app.spawn(async move |cx| {
        loop {
            let (_result, chunk) = output.read(Vec::with_capacity(READ_SIZE)).await;
            if chunk.is_empty() || !cx.update(|app| draw(app, &session, &chunk)) {
                break;
            }
        }
    });
}

/// Draws `output` on the session's screen; false once the session is gone.
fn draw(app: &mut App, session: &WeakEntity<Session>, output: &[u8]) -> bool {
    let Some(session) = session.upgrade() else {
        return false;
    };
    let replies = session.update(app, |session, cx| {
        let asked = session.screen.feed(output);
        if asked.screen_switched {
            session.selection = None;
        }
        cx.notify();
        asked.replies
    });
    type_bytes(app, &session, &replies);
    true
}

/// Says how the shell ended, once it has.
fn wait_for_end(app: &mut App, session: WeakEntity<Session>, child: Child) {
    app.spawn(async move |cx| {
        let status = child.exit_status().await;
        cx.update(|app| {
            let Some(session) = session.upgrade() else {
                return;
            };
            session.update(app, |session, cx| {
                session.terminal = None;
                session.unsent.clear();
                session.stage = stage_after(&status);
                cx.notify();
            });
        });
    });
}

/// Writes what was typed to the terminal, in order, until nothing is left.
fn send_unsent(app: &mut App, session: WeakEntity<Session>) {
    app.spawn(async move |cx| {
        while let Some((terminal, bytes)) = cx.update(|app| next_to_send(app, &session)) {
            if terminal.write(bytes).await.is_err() {
                break;
            }
        }
        cx.update(|app| {
            if let Some(session) = session.upgrade() {
                session.update(app, |session, _cx| session.sending = false);
            }
        });
    });
}

/// The terminal and what is typed and unsent, taken from the session; none once nothing is left,
/// or the session or its terminal is gone.
fn next_to_send(app: &mut App, session: &WeakEntity<Session>) -> Option<(Rc<Terminal>, Vec<u8>)> {
    let session = session.upgrade()?;
    session.update(app, |session, _cx| {
        let terminal = session.terminal.clone()?;
        let bytes = std::mem::take(&mut session.unsent);
        (!bytes.is_empty()).then_some((terminal, bytes))
    })
}

fn why(error: &machines::ExecError) -> String {
    match error {
        machines::ExecError::Refused(reason) | machines::ExecError::Failed(reason) => {
            reason.clone()
        }
    }
}

/// The stage of a session whose shell ended with `status`.
fn stage_after(status: &ExitStatus) -> Stage {
    match status {
        ExitStatus::Exited(0) => Stage::Left,
        ExitStatus::Exited(code) => Stage::Ended(format!("The shell ended with code {code}.")),
        ExitStatus::Signaled(signal) => {
            Stage::Ended(format!("The shell was ended by SIG{signal}."))
        }
        ExitStatus::Unknown => Stage::Ended("The shell ended.".to_owned()),
    }
}
