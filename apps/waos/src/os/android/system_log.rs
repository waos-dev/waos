//! The system's log, which `adb logcat` reads: each trace one entry under WaOS's tag, at the
//! priority of its level.

// Liblog is a C API, and the `ndk` crate wraps none of it; that is the `unsafe` here.
#![allow(unsafe_code)]

use std::ffi::CString;
use std::io::{self, Write};

use ndk_sys::android_LogPriority as Priority;
use tracing::{Level, Metadata};
use tracing_subscriber::fmt::MakeWriter;

/// The tag every entry WaOS writes carries.
const TAG: &[u8] = b"waos\0";

/// Where the traces go on Android: the system's log.
pub(crate) struct SystemLog;

impl<'writer> MakeWriter<'writer> for SystemLog {
    type Writer = LogEntry;

    fn make_writer(&'writer self) -> LogEntry {
        LogEntry::new(Priority::ANDROID_LOG_INFO)
    }

    fn make_writer_for(&'writer self, meta: &Metadata<'_>) -> LogEntry {
        LogEntry::new(match *meta.level() {
            Level::ERROR => Priority::ANDROID_LOG_ERROR,
            Level::WARN => Priority::ANDROID_LOG_WARN,
            Level::INFO => Priority::ANDROID_LOG_INFO,
            Level::DEBUG => Priority::ANDROID_LOG_DEBUG,
            Level::TRACE => Priority::ANDROID_LOG_VERBOSE,
        })
    }
}

/// One trace, written to the log as one entry once it is whole.
pub(crate) struct LogEntry {
    priority: Priority,
    text: Vec<u8>,
}

impl LogEntry {
    fn new(priority: Priority) -> LogEntry {
        LogEntry {
            priority,
            text: Vec::new(),
        }
    }
}

impl Write for LogEntry {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.text.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LogEntry {
    fn drop(&mut self) {
        let text = std::mem::take(&mut self.text);
        let line = text.strip_suffix(b"\n").unwrap_or(&text);
        let line: Vec<u8> = line.iter().copied().filter(|byte| *byte != 0).collect();
        let Ok(line) = CString::new(line) else {
            return;
        };
        // SAFETY: both pointers are to nul-terminated bytes that outlive the call, and liblog is
        // part of the platform every Android process links.
        unsafe {
            ndk_sys::__android_log_write(
                self.priority.0 as std::ffi::c_int,
                TAG.as_ptr().cast(),
                line.as_ptr(),
            );
        }
    }
}
