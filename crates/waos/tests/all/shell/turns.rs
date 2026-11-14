//! Turns at a program's lines in the log ([`super::output`]): the engine logs what a program
//! prints with no output of its own under the program's name, so two runs of one program print
//! alike. A test that reads a program's lines there takes the program's turn first, and so does a
//! test that runs the program with no output of its own beside such tests, so the lines are of one
//! run; every other test of the binary runs alongside.
//!
//! The invariant: a test that takes two turns takes them in the order of their programs' names, as
//! every other test taking both does, or two such tests can wait on each other forever.

use std::sync::{Condvar, Mutex};

/// The programs whose turn a test holds, and the signal that one was given back.
static TURNS: Turns = Turns {
    held: Mutex::new(Vec::new()),
    given_back: Condvar::new(),
};

struct Turns {
    held: Mutex<Vec<&'static str>>,
    given_back: Condvar,
}

/// A test's turn at a program, given back when dropped, after a test that failed holding it too.
pub struct Turn {
    program: &'static str,
}

/// Waits until no other test holds `program`'s turn, and takes it.
pub fn take(program: &'static str) -> Turn {
    let mut held = TURNS.held.lock().unwrap();
    while held.contains(&program) {
        held = TURNS.given_back.wait(held).unwrap();
    }
    held.push(program);
    Turn { program }
}

impl Drop for Turn {
    fn drop(&mut self) {
        TURNS
            .held
            .lock()
            .unwrap()
            .retain(|program| *program != self.program);
        TURNS.given_back.notify_all();
    }
}
