//! Windows' security, as the named pipe needs it: the user this process runs as, by their SID; a
//! security descriptor that gives that user alone access to a pipe, as its owner; and who owns a
//! pipe `cargo waos` opened.
//!
//! The invariant: this module wraps the system's security calls and is the one place in the crate
//! where `unsafe` is written; every buffer the system allocates for an answer is freed here, once.

#![allow(unsafe_code)]

use std::ffi::{OsStr, c_void};
use std::io;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::ptr;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The SID of the user this process runs as, in its text form, `S-1-5-21-…`.
pub(crate) fn user_sid() -> io::Result<String> {
    let token = process_token()?;
    let mut needed = 0;
    // SAFETY: asked with no buffer, the call only says how many bytes the answer needs; it fails
    // saying the buffer is too small, which is expected.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    // `u64`s, so the answer, which holds pointers, is aligned for them.
    let mut answer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer is `needed` bytes at least, and outlives the call.
    let read = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            answer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the system wrote a `TOKEN_USER` at the start of the buffer, aligned as `u64`s are,
    // whose SID points into the same buffer, which lives until the end of this function.
    let user = unsafe { &*answer.as_ptr().cast::<TOKEN_USER>() };
    sid_text(user.User.Sid)
}

/// This process's access token, to read who it runs as.
fn process_token() -> io::Result<OwnedHandle> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the current process's pseudo handle, which needs no closing; the token handle the
    // call answers is this function's to own.
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle the call just opened, owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

/// `sid` in its text form.
fn sid_text(sid: PSID) -> io::Result<String> {
    let mut text = ptr::null_mut();
    // SAFETY: `sid` is a valid SID the caller holds through the call; the text the call allocates
    // is freed below, once read.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the text the call allocated, not used after.
    Ok(unsafe { take_system_text(text) })
}

/// The NUL-terminated text the system allocated at `text`, read, then freed.
///
/// # Safety
///
/// `text` is a NUL-terminated UTF-16 text the system allocated with `LocalAlloc`, which nothing
/// uses after.
unsafe fn take_system_text(text: *mut u16) -> String {
    // SAFETY: the text ends with a NUL, before which every unit is read.
    let length = unsafe { (0..).take_while(|&at| *text.add(at) != 0).count() };
    // SAFETY: `length` units the system wrote.
    let read = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    // SAFETY: allocated with `LocalAlloc`, freed once.
    unsafe { LocalFree(text.cast()) };
    read
}

/// The SID of who owns the kernel object `handle` is open on, in its text form.
pub(crate) fn owner_sid(handle: BorrowedHandle<'_>) -> io::Result<String> {
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: an open handle; the owner the call answers points into the descriptor it allocates,
    // which is freed below, after the owner is read.
    let status = unsafe {
        GetSecurityInfo(
            handle.as_raw_handle(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let sid = sid_text(owner);
    // SAFETY: the descriptor the call allocated with `LocalAlloc`, freed once.
    unsafe { LocalFree(descriptor) };
    sid
}

/// A security descriptor that makes one user a pipe's owner and gives that user alone access to
/// it: `O:<sid>D:P(A;;GA;;;<sid>)`, a protected list that inherits nothing, so neither the
/// default's read access for everyone nor administrators' come with it.
pub(crate) struct UserOnly {
    descriptor: PSECURITY_DESCRIPTOR,
}

// SAFETY: the descriptor is memory of this process's own, never written after it is made, and
// freed once, when its owner drops it.
unsafe impl Send for UserOnly {}
// SAFETY: as above; the system only reads it.
unsafe impl Sync for UserOnly {}

impl UserOnly {
    /// The descriptor for the user whose SID is `sid`.
    pub(crate) fn new(sid: &str) -> io::Result<UserOnly> {
        let text: Vec<u16> = format!("O:{sid}D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain([0])
            .collect();
        let mut descriptor = ptr::null_mut();
        // SAFETY: a NUL-terminated text that outlives the call; the descriptor it allocates is
        // freed when this is dropped.
        let made = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if made == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(UserOnly { descriptor })
    }

    /// An instance of the pipe at `path`, made as `options` say, under this descriptor.
    pub(crate) fn create_pipe(
        &self,
        options: &ServerOptions,
        path: &OsStr,
    ) -> io::Result<NamedPipeServer> {
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: the attributes are a valid `SECURITY_ATTRIBUTES`, whose descriptor lives as long
        // as `self`, and outlive the call, which copies what it needs.
        unsafe {
            options
                .create_with_security_attributes_raw(path, (&raw mut attributes).cast::<c_void>())
        }
    }
}

impl Drop for UserOnly {
    fn drop(&mut self) {
        // SAFETY: the descriptor the system allocated with `LocalAlloc`, freed once.
        unsafe { LocalFree(self.descriptor) };
    }
}

/// What a test reads of a pipe's security: its owner and its access list, in the descriptor's text
/// form.
#[cfg(test)]
pub(crate) fn owner_and_access_text(handle: BorrowedHandle<'_>) -> io::Result<String> {
    use windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW;
    use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;

    let wanted = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: an open handle; the descriptor the call allocates is freed below.
    let status = unsafe {
        GetSecurityInfo(
            handle.as_raw_handle(),
            SE_KERNEL_OBJECT,
            wanted,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let mut text = ptr::null_mut();
    // SAFETY: the descriptor read above; the text the call allocates is freed below.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            wanted,
            &mut text,
            ptr::null_mut(),
        )
    };
    // SAFETY: the descriptor the system allocated, freed once.
    unsafe { LocalFree(descriptor) };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the text the call allocated, not used after.
    Ok(unsafe { take_system_text(text) })
}
