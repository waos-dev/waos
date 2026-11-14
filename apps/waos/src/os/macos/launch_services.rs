//! A program's app opened through Launch Services, as Chrome launches its shims
//! (`LaunchTheFirstShimThatWorksOnFileThread`): never brought forward, since what the user must see
//! brings it forward as it shows (`activation`), and told by an argument that WaOS launched it; and
//! the end of its process, which tells a launch that failed after it started from one that went
//! on.
//!
//! The invariant: launching never waits on the main thread. Launch Services answers on a queue of
//! its own, and the process's end is waited for on a thread of its own.

#![allow(unsafe_code)]

use std::io;
use std::path::Path;
use std::ptr::null_mut;

use block2::RcBlock;
use futures::channel::oneshot;
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSError, NSString, NSURL};

/// An app Launch Services opened: its process, once it started.
pub(crate) struct OpenedApp {
    pub(crate) pid: i32,
}

/// Opens the app bundle at `bundle` through Launch Services with `arguments`, in the background;
/// answers once it started, or why it did not.
pub(crate) fn open_app(
    bundle: &Path,
    arguments: &[&str],
) -> impl Future<Output = io::Result<OpenedApp>> + Send + 'static + use<> {
    let (answer, answered) = oneshot::channel();
    let answer = std::sync::Mutex::new(Some(answer));
    let completion = RcBlock::new(move |app: *mut NSRunningApplication, error: *mut NSError| {
        let result = launched(app, error);
        if let Some(answer) = answer.lock().ok().and_then(|mut answer| answer.take()) {
            let _ = answer.send(result);
        }
    });
    let url = NSURL::fileURLWithPath(&NSString::from_str(&bundle.to_string_lossy()));
    let configuration = NSWorkspaceOpenConfiguration::configuration();
    configuration.setActivates(false);
    configuration.setAddsToRecentItems(false);
    let arguments: Vec<_> = arguments
        .iter()
        .map(|argument| NSString::from_str(argument))
        .collect();
    configuration.setArguments(&NSArray::from_retained_slice(&arguments));
    NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
        &url,
        &configuration,
        Some(&completion),
    );
    async move {
        answered
            .await
            .unwrap_or_else(|_| Err(io::Error::other("Launch Services never answered")))
    }
}

/// What Launch Services' answer says: the app's process, or why it did not start.
fn launched(app: *mut NSRunningApplication, error: *mut NSError) -> io::Result<OpenedApp> {
    // SAFETY: Launch Services hands the handler an app or an error, each alive for the call.
    match unsafe { (app.as_ref(), error.as_ref()) } {
        (Some(app), _) => Ok(OpenedApp {
            pid: app.processIdentifier(),
        }),
        (None, Some(error)) => Err(io::Error::other(error.localizedDescription().to_string())),
        (None, None) => Err(io::Error::other("Launch Services opened nothing")),
    }
}

/// Waits until the process `pid` has ended; at once if it has already.
pub(crate) fn wait_for_exit(pid: i32) -> io::Result<()> {
    // SAFETY: a new kqueue, closed below.
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        return Err(io::Error::last_os_error());
    }
    let watched = libc::kevent {
        ident: pid as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: null_mut(),
    };
    let mut fired = watched;
    // SAFETY: one change in, room for one event out, both locals that outlive the call; no
    // timeout, so it returns when the process ends.
    let waited = unsafe { libc::kevent(queue, &watched, 1, &mut fired, 1, std::ptr::null()) };
    let error = io::Error::last_os_error();
    // SAFETY: the queue made above.
    unsafe { libc::close(queue) };
    match waited {
        // A process that ended before it was watched has no events to give.
        -1 if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
        -1 => Err(error),
        _ => Ok(()),
    }
}
