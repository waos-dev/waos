//! The dev server announced on the local network through Bonjour, as macOS announces its own
//! services: `DNSServiceRegister`, its instance named as the computer is in System Settings, until
//! the registration is deallocated.
//!
//! The invariant a later editor will break: the registration lasts exactly as long as the value
//! it is kept in, whose drop deallocates it, which takes the service off the network.

#![allow(unsafe_code)]

use std::ffi::{CString, c_char, c_void};
use std::io;
use std::ptr;

/// A registration with the system's mDNS responder.
type DNSServiceRef = *mut c_void;

/// What a registration's completion is told, which WaOS does not listen for: Bonjour picks the
/// name, and an announcement that fails leaves the direct address working.
type DNSServiceRegisterReply = Option<
    unsafe extern "C" fn(
        DNSServiceRef,
        u32,
        i32,
        *const c_char,
        *const c_char,
        *const c_char,
        *mut c_void,
    ),
>;

// Bonjour's API, in libSystem, which every program on macOS links.
unsafe extern "C" {
    fn DNSServiceRegister(
        service: *mut DNSServiceRef,
        flags: u32,
        interface_index: u32,
        name: *const c_char,
        registration_type: *const c_char,
        domain: *const c_char,
        host: *const c_char,
        port_in_network_order: u16,
        txt_length: u16,
        txt_record: *const c_void,
        reply: DNSServiceRegisterReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefDeallocate(service: DNSServiceRef);
}

/// The dev server's service on the network, there until dropped.
pub(crate) struct DevServerAnnouncement(DNSServiceRef);

impl Drop for DevServerAnnouncement {
    fn drop(&mut self) {
        // SAFETY: a registration this value made and owns, deallocated once.
        unsafe { DNSServiceRefDeallocate(self.0) };
    }
}

/// Announces `service` on `port`, its TXT record `txt`, named as this computer is.
pub(crate) async fn announce_dev_server(
    service: &str,
    port: u16,
    txt: &[(&str, String)],
) -> io::Result<DevServerAnnouncement> {
    let registration_type = CString::new(service).map_err(io::Error::other)?;
    let record = txt_record(txt)?;
    let length = u16::try_from(record.len()).map_err(io::Error::other)?;
    let mut registered: DNSServiceRef = ptr::null_mut();
    // SAFETY: the out pointer is this frame's; the strings and the record outlive the call, which
    // copies them; a null name, domain and host are Bonjour's defaults, and a null reply asks for
    // no callback.
    let error = unsafe {
        DNSServiceRegister(
            &mut registered,
            0,
            0,
            ptr::null(),
            registration_type.as_ptr(),
            ptr::null(),
            ptr::null(),
            port.to_be(),
            length,
            record.as_ptr().cast(),
            None,
            ptr::null_mut(),
        )
    };
    if error != 0 {
        return Err(io::Error::other(format!(
            "Bonjour refused the registration (error {error})"
        )));
    }
    Ok(DevServerAnnouncement(registered))
}

/// `txt` as DNS-SD writes a TXT record: each entry `key=value` after its length in a byte.
fn txt_record(txt: &[(&str, String)]) -> io::Result<Vec<u8>> {
    let mut record = Vec::new();
    for (key, value) in txt {
        let entry = format!("{key}={value}");
        let length = u8::try_from(entry.len()).map_err(io::Error::other)?;
        record.push(length);
        record.extend_from_slice(entry.as_bytes());
    }
    Ok(record)
}
