//! What the app says, in both layouts.

use waos_activity::Folder;
use wasdk::inspect::devices::StartError;

use crate::clock::now;
use crate::inspector::Seen;
use crate::panels::Decided;

pub(crate) const APP_NAME: &str = "Inspector";
pub(crate) const PROGRAMS: &str = "Programs";
pub(crate) const LOOKING: &str = "Reading the programs…";
pub(crate) const NO_DEVICE: &str = "WaOS shows Inspector no programs. It needs to be allowed to see what your other programs do, in its settings in WaOS's library.";
pub(crate) const NO_PROGRAMS: &str = "There are no programs on this device.";
pub(crate) const CHOOSE: &str = "Choose a program to see what it did.";
pub(crate) const USED_NOTHING: &str =
    "It has used nothing that needs your permission over the last seven days.";
pub(crate) const RUNNING: &str = "Running";
pub(crate) const REPORT: &str = "Report";
pub(crate) const NETWORK: &str = "Network";
pub(crate) const FILES: &str = "Files";
pub(crate) const DEVICES: &str = "Devices";
pub(crate) const OUTPUT: &str = "Output";
pub(crate) const IN_USE: &str = "In use";
pub(crate) const ACCESS: &str = "Data and sensor access";
pub(crate) const ACCESS_DETAIL: &str =
    "What it used that needs your permission, and on which of the last seven days.";
pub(crate) const NETWORK_ACTIVITY: &str = "Network activity";
pub(crate) const NETWORK_ACTIVITY_DETAIL: &str =
    "The domains it contacted, the most contacted first.";
pub(crate) const NO_DOMAINS: &str = "It contacted no domain.";
pub(crate) const QUESTIONS: &str = "Asked";
pub(crate) const LIVE: &str = "As it happens";
pub(crate) const LIVE_DETAIL: &str = "What it does from now on, one event a row, while it runs.";
pub(crate) const SEE_NETWORK: &str = "Its connections and requests";
pub(crate) const SEE_FILES: &str = "The files it opens and changes";
pub(crate) const SEE_DEVICES: &str = "Its camera, microphone and machines";
pub(crate) const SEE_OUTPUT: &str = "What it writes to its output and errors";
pub(crate) const SEE_IN_USE: &str = "What it uses now: files open, connections, captures";
pub(crate) const NOT_RUNNING: &str =
    "It is not running. Start it inspected to see what it does from its first instruction.";
pub(crate) const START_INSPECTED: &str = "Start inspected";
pub(crate) const RESTART_INSPECTED: &str = "Restart inspected";
pub(crate) const START_DETAIL: &str =
    "WaOS holds it before its first instruction until this panel listens.";
pub(crate) const START: &str = "Start";
pub(crate) const RESTART: &str = "Restart";
pub(crate) const NO_NETWORK_YET: &str =
    "Nothing yet. Its connections and requests show here as it makes them.";
pub(crate) const NO_FILES_YET: &str =
    "Nothing yet. The files it opens and changes show here as it does.";
pub(crate) const NO_DEVICES_YET: &str =
    "Nothing yet. Its camera, microphone and machines show here as it uses them.";
pub(crate) const NO_OUTPUT_YET: &str = "Nothing yet. What it writes shows here as it writes it.";
pub(crate) const NOTHING_IN_USE: &str =
    "It uses nothing now: no file open, no connection, no capture.";
pub(crate) const THROUGH_AN_OPEN_FILE: &str = "(through a file it has open)";
pub(crate) const WHERE_UNKNOWN: &str = "(the system cannot say where)";
pub(crate) const CAMERA: &str = "Camera";
pub(crate) const MICROPHONE: &str = "Microphone";
pub(crate) const WAOS_OWN: &str = "WaOS's own, simulated";
pub(crate) const IN_A_TERMINAL: &str = "in a terminal";
pub(crate) const A_COMMAND: &str = "A command";
pub(crate) const NOT_BOUND: &str = "not bound yet";
pub(crate) const SINCE: &str = "Since";
pub(crate) const LOCAL_ADDRESSES: &str = "This computer and the local network";
pub(crate) const ALLOWED: &str = "Allowed";
pub(crate) const FAILED: &str = "Failed";
pub(crate) const REFUSED: &str = "Refused";

/// What a program's row says beneath its name: its version, whether it runs, and when it last
/// used something guarded.
pub(crate) fn detail(seen: &Seen) -> String {
    let mut said = Vec::new();
    if let Some(version) = &seen.program.version {
        said.push(version.clone());
    }
    if seen.program.running {
        said.push(RUNNING.to_owned());
    }
    if let Some(last) = seen.activity.last_use {
        said.push(format!("used {}", waos_activity::when(last, now())));
    }
    said.join(" · ")
}

/// What a program used, by name, on one line; empty for nothing.
pub(crate) fn marks(seen: &Seen) -> String {
    seen.activity.marks().join(" · ")
}

/// When `at` was, against now: `2 minutes ago`.
pub(crate) fn ago(at: i64) -> String {
    waos_activity::when(at, now())
}

/// How often a domain was contacted, and how often refused.
pub(crate) fn contacted(times: u32, refused: u32) -> String {
    let said = times_said(times);
    if refused == 0 {
        said
    } else {
        format!("{said}, refused {refused}")
    }
}

/// The files a program opened in its folders, in short: `opened 12 files in 2 folders, refused
/// once`.
pub(crate) fn opened_files(folders: &[Folder], refused: u32) -> String {
    let opened: u32 = folders
        .iter()
        .map(|folder| folder.opened_to_read + folder.opened_to_write)
        .sum();
    let mut said = format!(
        "opened {} in {}",
        counted(opened, "file", "files"),
        counted(folders.len() as u32, "folder", "folders")
    );
    if refused > 0 {
        said.push_str(&format!(", refused {}", times_said(refused)));
    }
    said
}

/// `n` times, in words: `once`, `twice`, `3 times`.
fn times_said(n: u32) -> String {
    match n {
        1 => "once".to_owned(),
        2 => "twice".to_owned(),
        n => format!("{n} times"),
    }
}

fn counted(n: u32, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// `nanoseconds` long, as a panel's Time column says it: `840 ms`, `2.4 s`, `3 min`.
pub(crate) fn took(nanoseconds: u64) -> String {
    let ms = nanoseconds / 1_000_000;
    match ms {
        0..10_000 => format!("{ms} ms"),
        10_000..120_000 => format!("{:.1} s", ms as f64 / 1000.0),
        _ => format!("{} min", ms / 60_000),
    }
}

/// `n` bytes, in the largest unit that keeps a whole number before the point: `512 B`, `12.3 KB`.
pub(crate) fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut size = n as f64 / 1000.0;
    let mut unit = 0;
    while size >= 1000.0 && unit < UNITS.len() - 1 {
        size /= 1000.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

/// The addresses of this computer and the local network a program reached, in short.
pub(crate) fn local_addresses(n: usize) -> String {
    let addresses = counted(u32::try_from(n).unwrap_or(u32::MAX), "address", "addresses");
    format!("{addresses} of this computer and the local network")
}

/// Which stream of which instance wrote a line: `Errors`, `Worker 2, output`.
pub(crate) fn stream(instance: u32, errors: bool) -> String {
    match (instance, errors) {
        (0, false) => "Output".to_owned(),
        (0, true) => "Errors".to_owned(),
        (worker, false) => format!("Worker {worker}, output"),
        (worker, true) => format!("Worker {worker}, errors"),
    }
}

/// Why WaOS did not start a program as the inspector asked.
pub(crate) fn start_refused(error: &StartError) -> String {
    match error {
        StartError::Unknown => "WaOS knows no program by this id to start.".to_owned(),
        StartError::Failed(why) => format!("WaOS could not start it: {why}"),
    }
}

/// Which camera or microphone captures: `WaOS's simulated camera`, `This computer's microphone`.
pub(crate) fn capturing(device: &str, simulated: bool) -> String {
    let device = device.to_lowercase();
    if simulated {
        format!("WaOS's simulated {device}")
    } else {
        format!("This computer's {device}")
    }
}

/// Where a path is on this computer, beneath its row.
pub(crate) fn on_this_computer(path: &str) -> String {
    format!("On this computer: {path}")
}

/// How WaOS decided an event, in a word.
pub(crate) fn decided(decided: &Decided) -> &'static str {
    match decided {
        Decided::Allowed => ALLOWED,
        Decided::Refused(_) => REFUSED,
        Decided::Failed => FAILED,
    }
}

/// Why WaOS refused an event and where its user changes that, for a refused one.
pub(crate) fn why_decided(decided: &Decided) -> Option<String> {
    match decided {
        Decided::Refused(setting) => Some(refused_by(setting)),
        Decided::Allowed | Decided::Failed => None,
    }
}

/// Why a setting of the key `setting` refused a use, and where its user changes it.
fn refused_by(setting: &str) -> String {
    let (why, row) = match setting {
        "network" => ("Its network switch is off", "Network"),
        "domains" => ("The domain is not among those it may reach", "Network"),
        "local-network" => (
            "It may not reach this computer and the local network",
            LOCAL_ADDRESSES,
        ),
        "folders" => ("It is outside the folders it was given", "Folders"),
        "clipboard-read" => (
            "Its reading of the clipboard is off",
            "Reading the clipboard",
        ),
        "clipboard-write" => (
            "Its writing of the clipboard is off",
            "Writing the clipboard",
        ),
        "camera" => ("It may not use the camera", "Camera"),
        "microphone" => ("It may not use the microphone", "Microphone"),
        "notifications" => ("It may not post notifications", "Notifications"),
        other => {
            return format!(
                "Its {other} setting refused it. Change it in its settings in WaOS's library."
            );
        }
    };
    format!("{why}. Change it under {row}, in its settings in WaOS's library.")
}
