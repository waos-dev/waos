//! Code hashes, as Chrome checks its app shims' (`IsAcceptablyAdHocCodeSigned`,
//! `VerifyCodeDirectoryHash`): the code directory hash of a bundle WaOS signed, read from the
//! bundle on disk, and the one of a running process, found by the audit token a message of it
//! carried, whose signature Security checks first. A process whose hash is the one WaOS kept when
//! it signed the program's bundle is that bundle's code, unchanged.
//!
//! The invariant: a process is found by its audit token, never by its pid alone, which another
//! process may take once it ends; and its signature is checked as it runs, not as its file reads.

#![allow(unsafe_code)]

use std::io;
use std::path::Path;
use std::ptr::{NonNull, null, null_mut};

use objc2_core_foundation::{CFData, CFDictionary, CFRetained, CFType, CFURL};
use objc2_security::{
    SecCSFlags, SecCode, SecStaticCode, kSecCSSigningInformation, kSecCodeInfoUnique,
    kSecGuestAttributeAudit,
};
use waos_window_messages::os::macos::AuditToken;

/// The code hash of the bundle at `bundle`, as it is signed on disk.
pub(crate) fn code_hash_of_bundle(bundle: &Path) -> io::Result<Vec<u8>> {
    let url = CFURL::from_file_path(bundle)
        .ok_or_else(|| io::Error::other(format!("{} as a URL", bundle.display())))?;
    let mut code: *const SecStaticCode = null();
    // SAFETY: `code` outlives the call, which sets it to a static code it retains for us.
    let made = unsafe {
        SecStaticCode::create_with_path(&url, SecCSFlags::DefaultFlags, NonNull::from(&mut code))
    };
    checked(made, "SecStaticCodeCreateWithPath")?;
    // SAFETY: the call succeeded, so `code` is a retained static code that is ours.
    let code = unsafe { CFRetained::from_raw(NonNull::new(code.cast_mut()).expect("a code")) };
    code_hash(&code)
}

/// The code hash of the process that sent a message with `sender`, once Security has checked the
/// process's signature as it runs.
pub(crate) fn code_hash_of_process(sender: &AuditToken) -> io::Result<Vec<u8>> {
    let token = CFData::from_bytes(&sender.bytes());
    // SAFETY: Security's own constant, which lives for the process.
    let key = unsafe { kSecGuestAttributeAudit };
    let attributes =
        CFDictionary::<CFType, CFType>::from_slices(&[key.as_ref()], &[token.as_ref()]);
    let mut guest: *mut SecCode = null_mut();
    // SAFETY: the attributes are a dictionary of CF types, and `guest` outlives the call, which
    // sets it to a code it retains for us.
    let found = unsafe {
        SecCode::copy_guest_with_attributes(
            None,
            Some(attributes.as_opaque()),
            SecCSFlags::DefaultFlags,
            NonNull::from(&mut guest),
        )
    };
    checked(found, "SecCodeCopyGuestWithAttributes")?;
    // SAFETY: the call succeeded, so `guest` is a retained code that is ours.
    let guest = unsafe { CFRetained::from_raw(NonNull::new(guest).expect("a guest")) };
    // SAFETY: a valid code, checked against no requirement but its own signature.
    let valid = unsafe { guest.check_validity(SecCSFlags::DefaultFlags, None) };
    checked(valid, "SecCodeCheckValidity")?;
    let mut code: *const SecStaticCode = null();
    // SAFETY: `code` outlives the call, which sets it to the static code it retains for us.
    let copied =
        unsafe { guest.copy_static_code(SecCSFlags::DefaultFlags, NonNull::from(&mut code)) };
    checked(copied, "SecCodeCopyStaticCode")?;
    // SAFETY: the call succeeded, so `code` is a retained static code that is ours.
    let code = unsafe { CFRetained::from_raw(NonNull::new(code.cast_mut()).expect("a code")) };
    code_hash(&code)
}

/// The code directory hash `code` is signed with.
fn code_hash(code: &SecStaticCode) -> io::Result<Vec<u8>> {
    let mut information: *const CFDictionary = null();
    // SAFETY: `information` outlives the call, which sets it to a dictionary it retains for us.
    let copied = unsafe {
        SecCode::copy_signing_information(
            code,
            SecCSFlags(kSecCSSigningInformation),
            NonNull::from(&mut information),
        )
    };
    checked(copied, "SecCodeCopySigningInformation")?;
    // SAFETY: the call succeeded, so `information` is a retained dictionary that is ours.
    let information: CFRetained<CFDictionary> =
        unsafe { CFRetained::from_raw(NonNull::new(information.cast_mut()).expect("information")) };
    // SAFETY: Security's signing information is a dictionary of CF types under CF strings.
    let information: &CFDictionary<CFType, CFType> = unsafe { information.cast_unchecked() };
    // SAFETY: Security's own constant, which lives for the process.
    let unique = unsafe { kSecCodeInfoUnique };
    let hash = information
        .get(unique.as_ref())
        .and_then(|hash| hash.downcast::<CFData>().ok())
        .ok_or_else(|| io::Error::other("the code is not signed"))?;
    Ok(hash.to_vec())
}

/// Security's `status` of `what`, as an error when it failed.
fn checked(status: i32, what: &str) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("{what}: OSStatus {status}")))
    }
}
