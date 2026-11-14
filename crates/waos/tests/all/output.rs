//! Where a program's output goes: to the `ProgramOutput` in its environment, as it writes it and
//! in order, or, with none, to the engine's log. The program is `examples/forever`, which says
//! its count every so often and asks nothing else.
//!
//! Both tests read what forever logged, and two forevers at once would log alike, so each takes
//! forever's turn (`shell::turns`).

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use waos::{OutputStream, ProgramEnd, ProgramEnvironment, ProgramOutput};

use crate::shell;
use crate::shell::WITHIN;

/// Forever's first two reports, in order.
const FIRST_REPORTS: &str = "count 1048576\ncount 2097152\n";

/// An output that keeps every write as it came, with its stream.
#[derive(Default)]
struct Recording {
    writes: Mutex<Vec<(OutputStream, Vec<u8>)>>,
}

impl Recording {
    /// Everything written to stdout so far, in order.
    fn stdout(&self) -> Vec<u8> {
        self.writes
            .lock()
            .unwrap()
            .iter()
            .filter(|(stream, _)| *stream == OutputStream::Stdout)
            .flat_map(|(_, bytes)| bytes.iter().copied())
            .collect()
    }

    /// Waits until stdout holds `wanted` bytes.
    fn wait_for_stdout(&self, wanted: usize) -> Vec<u8> {
        let deadline = Instant::now() + WITHIN;
        loop {
            let stdout = self.stdout();
            if stdout.len() >= wanted {
                return stdout;
            }
            assert!(
                Instant::now() < deadline,
                "the program never wrote {wanted} bytes; it wrote {stdout:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl ProgramOutput for Recording {
    fn write(&self, stream: OutputStream, bytes: &[u8]) {
        self.writes.lock().unwrap().push((stream, bytes.to_vec()));
    }
}

#[test]
fn a_chosen_output_gets_what_the_program_writes_in_order_and_the_log_nothing() {
    let _forever = shell::turns::take("forever");
    let log = shell::output();
    let mark = log.mark();
    let (engine, _shell, _arrivals) = shell::engine();
    let forever = shell::load(&engine, waos_test_programs::FOREVER);
    let recording = Arc::new(Recording::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&recording) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let (running, _given) = engine.run(&forever, environment, Vec::new()).unwrap();
    let stdout = recording.wait_for_stdout(FIRST_REPORTS.len());
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
    assert_eq!(
        String::from_utf8_lossy(&stdout[..FIRST_REPORTS.len()]),
        FIRST_REPORTS
    );
    assert!(
        log.lines_of(mark, "forever").is_empty(),
        "the log got what the chosen output was for: {:?}",
        log.lines_of(mark, "forever")
    );
}

#[test]
fn with_no_output_chosen_the_program_writes_to_the_log() {
    let _forever = shell::turns::take("forever");
    let log = shell::output();
    let mark = log.mark();
    let (engine, _shell, _arrivals) = shell::engine();
    let forever = shell::load(&engine, waos_test_programs::FOREVER);
    let (running, _given) = engine
        .run(&forever, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    log.wait_for(mark, "forever", |line| line == "count 2097152");
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
    let lines = log.lines_of(mark, "forever");
    assert_eq!(&lines[..2], ["count 1048576", "count 2097152"]);
}
