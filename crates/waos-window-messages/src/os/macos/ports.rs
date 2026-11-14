//! Rights to Mach ports: [`ReceiveRight`], a port of this process's own, which messages are read
//! from, and [`SendRight`], someone's port, which messages are sent to. A WaOS checks its
//! bootstrap name in and gets the receive right a window process's hello arrives on; a window
//! process looks the name up and gets a send right to it; each then hands the other a send right
//! to a port of its own in a message. A send right the system made for its caller, such as Core
//! Animation's fence, is taken over (`SendRight::adopt`), carried in a message like any other, and
//! lent by its name to the system call that takes it (`SendRight::name`).
//!
//! `bootstrap_check_in` and `bootstrap_look_up` are not deprecated in macOS 27's SDK
//! (`servers/bootstrap.h`), and a name checked in that no launchd plist declares works outside the
//! App Sandbox, which WaOS and its window processes are (`../experiments/2026-10-29-present-over-mach`).
//!
//! The invariant: each value owns exactly one reference to its right and lets it go when dropped,
//! and a right moved into a message is no longer the value's (`SendRight::into_name`).

use std::ffi::CString;
use std::io;

use mach2::bootstrap::{bootstrap_check_in, bootstrap_look_up, bootstrap_port};
use mach2::kern_return::{KERN_SUCCESS, kern_return_t};
use mach2::mach_port::{
    mach_port_construct, mach_port_deallocate, mach_port_insert_right, mach_port_mod_refs,
};
use mach2::message::{MACH_MSG_TYPE_MAKE_SEND, MACH_MSG_TYPE_MAKE_SEND_ONCE, mach_msg_type_name_t};
use mach2::notify::MACH_NOTIFY_NO_SENDERS;
use mach2::port::{
    MACH_PORT_NULL, MACH_PORT_QLIMIT_LARGE, MACH_PORT_RIGHT_RECEIVE, MPO_QLIMIT,
    mach_port_limits_t, mach_port_mscount_t, mach_port_options_t, mach_port_t,
};
use mach2::traps::mach_task_self;

unsafe extern "C" {
    /// `mach/mach_port.h`, which `mach2` leaves out.
    fn mach_port_request_notification(
        task: mach_port_t,
        name: mach_port_t,
        message_id: i32,
        sync: mach_port_mscount_t,
        notify: mach_port_t,
        notify_disposition: mach_msg_type_name_t,
        previous: *mut mach_port_t,
    ) -> kern_return_t;
}

/// The receive right of a port of this process's own, with room for many waiting messages, so a
/// sender never waits for room.
#[derive(Debug)]
pub struct ReceiveRight(mach_port_t);

/// A send right to a port, this process's or another's.
#[derive(Debug)]
pub struct SendRight(mach_port_t);

impl ReceiveRight {
    /// A new port.
    pub fn new() -> io::Result<ReceiveRight> {
        let mut options = mach_port_options_t {
            flags: MPO_QLIMIT,
            mpl: mach_port_limits_t {
                mpl_qlimit: MACH_PORT_QLIMIT_LARGE,
            },
            reserved: [0; 2],
        };
        let mut name = MACH_PORT_NULL;
        // SAFETY: both pointers are to locals that outlive the call.
        let made = unsafe { mach_port_construct(mach_task_self(), &mut options, 0, &mut name) };
        checked(made, "mach_port_construct")?;
        Ok(ReceiveRight(name))
    }

    /// The port `name` names among this user's session's services, which this process takes
    /// the receive right of; anyone in the session can look it up while this process holds it.
    pub fn check_in(name: &str) -> io::Result<ReceiveRight> {
        let service = service_name(name)?;
        let mut port = MACH_PORT_NULL;
        // SAFETY: the name is NUL-terminated and `port` outlives the call.
        let checked_in = unsafe { bootstrap_check_in(bootstrap_port, service.as_ptr(), &mut port) };
        checked(checked_in, "bootstrap_check_in")?;
        Ok(ReceiveRight(port))
    }

    /// A send right to this port, to hand to another process.
    pub fn make_send(&self) -> io::Result<SendRight> {
        // SAFETY: inserting a send right under the name of a receive right this value holds.
        let made = unsafe {
            mach_port_insert_right(mach_task_self(), self.0, self.0, MACH_MSG_TYPE_MAKE_SEND)
        };
        checked(made, "mach_port_insert_right")?;
        Ok(SendRight(self.0))
    }

    /// Asks the port to receive [`Arrival::NoSenders`](super::Arrival::NoSenders) once no send
    /// right to it is left, after one was made: how a process hears that the other one is gone.
    pub fn notify_when_no_senders(&self) -> io::Result<()> {
        let mut previous = MACH_PORT_NULL;
        // SAFETY: the port is this value's; the notification is sent to it, through a send-once
        // right the kernel makes; `previous` outlives the call.
        let requested = unsafe {
            mach_port_request_notification(
                mach_task_self(),
                self.0,
                MACH_NOTIFY_NO_SENDERS,
                1,
                self.0,
                MACH_MSG_TYPE_MAKE_SEND_ONCE,
                &mut previous,
            )
        };
        checked(requested, "mach_port_request_notification")?;
        if previous != MACH_PORT_NULL {
            deallocate(previous);
        }
        Ok(())
    }

    pub(super) fn name(&self) -> mach_port_t {
        self.0
    }
}

impl Drop for ReceiveRight {
    fn drop(&mut self) {
        // SAFETY: the receive right is this value's; a send right made from it lives on as a
        // dead name, which its own value lets go.
        unsafe { mach_port_mod_refs(mach_task_self(), self.0, MACH_PORT_RIGHT_RECEIVE, -1) };
    }
}

impl SendRight {
    /// The port checked in under `name` among this user's session's services.
    pub fn look_up(name: &str) -> io::Result<SendRight> {
        let service = service_name(name)?;
        let mut port = MACH_PORT_NULL;
        // SAFETY: the name is NUL-terminated and `port` outlives the call.
        let looked_up = unsafe { bootstrap_look_up(bootstrap_port, service.as_ptr(), &mut port) };
        checked(looked_up, "bootstrap_look_up")?;
        Ok(SendRight(port))
    }

    /// Takes over the send right a message brought under `name`.
    pub(super) fn received(name: mach_port_t) -> SendRight {
        SendRight(name)
    }

    /// Takes over the send right this process holds under `name`, which a system call made for
    /// its caller, as Core Animation's `createFencePort` does.
    ///
    /// # Safety
    ///
    /// The caller holds one reference to a send right under `name`, which it gives to the value:
    /// nothing else lets it go.
    pub unsafe fn adopt(name: mach_port_t) -> SendRight {
        SendRight(name)
    }

    /// The right's name in this process, for a system call that takes one, as Core Animation's
    /// `setFencePort:` does; the value keeps the right.
    pub fn name(&self) -> mach_port_t {
        self.0
    }

    /// The right's name, which the value no longer lets go: a message moving it owns it now.
    pub(super) fn into_name(self) -> mach_port_t {
        let name = self.0;
        std::mem::forget(self);
        name
    }
}

impl Drop for SendRight {
    fn drop(&mut self) {
        deallocate(self.0);
    }
}

/// Lets go of this process's right named `port`.
pub(super) fn deallocate(port: mach_port_t) {
    // SAFETY: deallocating a name this process holds a right under; a stale name only fails.
    unsafe { mach_port_deallocate(mach_task_self(), port) };
}

/// A bootstrap name as the bootstrap server takes it.
fn service_name(name: &str) -> io::Result<CString> {
    CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in a name"))
}

/// `result` of the call `what`, as an error when it failed.
pub(super) fn checked(result: kern_return_t, what: &str) -> io::Result<()> {
    if result == KERN_SUCCESS {
        Ok(())
    } else {
        Err(failure(result, what))
    }
}

/// The call `what` failed with `result`.
pub(super) fn failure(result: kern_return_t, what: &str) -> io::Error {
    io::Error::other(format!("{what}: {result:#x}"))
}
