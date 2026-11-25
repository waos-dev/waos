//! The dev server announced on the local network through Windows' own mDNS responder:
//! `DnsServiceRegister`, its instance named as the computer is, until it is deregistered.
//!
//! The invariant a later editor will break: the system calls the completion with the context
//! from another thread, for the registration and again for the deregistration, and reads the
//! instance until then, so both outlive the deregistration: one that does not complete in time
//! leaves them, never frees them.

#![allow(unsafe_code)]

use std::ffi::{OsStr, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use windows_sys::Win32::Foundation::DNS_REQUEST_PENDING;
use windows_sys::Win32::NetworkManagement::Dns::{
    DNS_QUERY_REQUEST_VERSION1, DNS_SERVICE_INSTANCE, DNS_SERVICE_REGISTER_REQUEST,
    DnsServiceConstructInstance, DnsServiceDeRegister, DnsServiceFreeInstance, DnsServiceRegister,
};
use windows_sys::core::PCWSTR;

/// How long a deregistration may take before its context is left to the system.
const DEREGISTER_WAIT: Duration = Duration::from_secs(2);

/// The dev server's service on the network, there until dropped.
pub(crate) struct DevServerAnnouncement {
    request: Box<DNS_SERVICE_REGISTER_REQUEST>,
    completed: Receiver<u32>,
    context: *mut SyncSender<u32>,
}

impl Drop for DevServerAnnouncement {
    fn drop(&mut self) {
        // SAFETY: the request this value registered, its instance still its own.
        let status = unsafe { DnsServiceDeRegister(&*self.request, ptr::null_mut()) };
        let completed = status as i32 != DNS_REQUEST_PENDING
            || self.completed.recv_timeout(DEREGISTER_WAIT).is_ok();
        if !completed {
            tracing::warn!("Windows did not say the dev server's announcement ended");
            return;
        }
        // SAFETY: the instance this value constructed and the context it leaked, which neither
        // the system nor a completion reaches once the deregistration ended.
        unsafe {
            DnsServiceFreeInstance(self.request.pServiceInstance);
            drop(Box::from_raw(self.context));
        }
    }
}

/// Announces `service` on `port`, its TXT record `txt`, named as this computer is.
pub(crate) async fn announce_dev_server(
    service: &str,
    port: u16,
    txt: &[(&str, String)],
) -> io::Result<DevServerAnnouncement> {
    let computer = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "WaOS".to_owned());
    let instance_name = wide(&format!("{computer}.{service}.local"));
    let host_name = wide(&format!("{computer}.local"));
    let keys: Vec<Vec<u16>> = txt.iter().map(|(key, _)| wide(key)).collect();
    let values: Vec<Vec<u16>> = txt.iter().map(|(_, value)| wide(value)).collect();
    let key_pointers: Vec<PCWSTR> = keys.iter().map(|key| key.as_ptr()).collect();
    let value_pointers: Vec<PCWSTR> = values.iter().map(|value| value.as_ptr()).collect();
    // SAFETY: NUL-terminated names and as many keys as values, which the call copies; no
    // address, so the system answers with this computer's own.
    let instance = unsafe {
        DnsServiceConstructInstance(
            instance_name.as_ptr(),
            host_name.as_ptr(),
            ptr::null(),
            ptr::null(),
            port,
            0,
            0,
            key_pointers.len() as u32,
            key_pointers.as_ptr(),
            value_pointers.as_ptr(),
        )
    };
    if instance.is_null() {
        return Err(io::Error::other("Windows made no service instance"));
    }
    let (sender, completed) = mpsc::sync_channel(2);
    let context = Box::into_raw(Box::new(sender));
    let request = Box::new(DNS_SERVICE_REGISTER_REQUEST {
        Version: DNS_QUERY_REQUEST_VERSION1,
        InterfaceIndex: 0,
        pServiceInstance: instance,
        pRegisterCompletionCallback: Some(registration_completed),
        pQueryContext: context.cast(),
        hCredentials: ptr::null_mut(),
        unicastEnabled: 0,
    });
    // SAFETY: a request whose instance and context outlive the registration, as this value holds
    // them until its drop deregisters it.
    let status = unsafe { DnsServiceRegister(&*request, ptr::null_mut()) };
    if status as i32 != DNS_REQUEST_PENDING {
        // SAFETY: the instance and the context made above, which no registration reaches.
        unsafe {
            DnsServiceFreeInstance(instance);
            drop(Box::from_raw(context));
        }
        return Err(io::Error::other(format!(
            "Windows refused the registration (error {status})"
        )));
    }
    Ok(DevServerAnnouncement {
        request,
        completed,
        context,
    })
}

/// What the system says of a registration or a deregistration: said where it failed, and told
/// to the announcement waiting for it.
unsafe extern "system" fn registration_completed(
    status: u32,
    context: *const c_void,
    instance: *const DNS_SERVICE_INSTANCE,
) {
    if status != 0 {
        tracing::warn!(status, "Windows did not announce the dev server");
    }
    if !instance.is_null() {
        // SAFETY: the instance the system made for this completion, the completion's to free.
        unsafe { DnsServiceFreeInstance(instance) };
    }
    // SAFETY: the announcement's context, which outlives every completion.
    let sender = unsafe { &*context.cast::<SyncSender<u32>>() };
    let _ = sender.try_send(status);
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}
