//! What a command is on this system, under one set of names: the group it starts in, which takes
//! its signals (`process_group` on Unix's systems, a job on Windows), how its end is waited for and told, and
//! its terminal (`pseudo_terminal`, a pseudoconsole on Windows).

#[cfg(unix)]
pub(crate) use crate::process_group::{ProcessGroup as CommandGroup, start_with_pipes};
#[cfg(unix)]
pub(crate) use crate::pseudo_terminal::{TerminalSession, start_in_terminal};
#[cfg(windows)]
pub(crate) use crate::windows::{
    CommandJob as CommandGroup, TerminalSession, start_in_terminal, start_with_pipes,
};
