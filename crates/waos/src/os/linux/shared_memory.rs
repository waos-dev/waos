//! Pixels in shared memory: a `memfd` of the size a picture needs, mapped read-write here, which
//! the shell hands the compositor to map (Wayland's `wl_shm`). The engine writes a frame into it
//! once the program's GPU has drawn it, and reads it for screenshots.
//!
//! The invariant a later editor must keep: the memory never shrinks once made, as a compositor
//! that maps it reads it whole (it is sealed against shrinking), and it is written only while no
//! present of it waits for the compositor's release, as any buffer of a pool is.

use std::ffi::c_void;
use std::ops::{Deref, DerefMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Mutex, MutexGuard};

use anyhow::{Result, bail};

use crate::sync::locked;

/// `length` bytes of shared memory, zeroed, mapped here for the CPU.
pub(crate) struct SharedMemory {
    fd: OwnedFd,
    mapped: Mapped,
    /// Held while the bytes are read or written.
    access: Mutex<()>,
}

/// The mapping's address and length.
struct Mapped {
    address: *mut c_void,
    length: usize,
}

// SAFETY: the mapping is plain memory, reached only through `SharedMemory::bytes`, under `access`.
unsafe impl Send for Mapped {}
unsafe impl Sync for Mapped {}

impl SharedMemory {
    pub(crate) fn new(length: usize) -> Result<SharedMemory> {
        if length == 0 {
            bail!("shared memory of no bytes");
        }
        // SAFETY: calls of the C library on a name it copies and the fd it returns, which is owned
        // from here on, and a mapping of exactly the bytes the file was given.
        unsafe {
            let fd = libc::memfd_create(
                c"waos picture".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            );
            if fd < 0 {
                bail!("making shared memory: {}", std::io::Error::last_os_error());
            }
            let fd = OwnedFd::from_raw_fd(fd);
            if libc::ftruncate(fd.as_raw_fd(), length as libc::off_t) != 0 {
                bail!("sizing shared memory: {}", std::io::Error::last_os_error());
            }
            libc::fcntl(fd.as_raw_fd(), libc::F_ADD_SEALS, libc::F_SEAL_SHRINK);
            let address = libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            );
            if address == libc::MAP_FAILED {
                bail!("mapping shared memory: {}", std::io::Error::last_os_error());
            }
            Ok(SharedMemory {
                fd,
                mapped: Mapped { address, length },
                access: Mutex::new(()),
            })
        }
    }

    /// The memory's fd, which the shell duplicates to hand the compositor.
    pub(crate) fn fd(&self) -> &OwnedFd {
        &self.fd
    }

    /// The bytes, held until the guard goes.
    pub(crate) fn bytes(&self) -> Bytes<'_> {
        Bytes {
            _access: locked(&self.access),
            mapped: &self.mapped,
        }
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        // SAFETY: the mapping made in `new`, unmapped once, as nothing reaches it any more.
        unsafe {
            libc::munmap(self.mapped.address, self.mapped.length);
        }
    }
}

/// The memory's bytes, while held.
pub(crate) struct Bytes<'a> {
    _access: MutexGuard<'a, ()>,
    mapped: &'a Mapped,
}

impl Deref for Bytes<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // SAFETY: the mapping lives as long as the memory the guard borrows, and only one guard is
        // held at a time.
        unsafe { std::slice::from_raw_parts(self.mapped.address.cast(), self.mapped.length) }
    }
}

impl DerefMut for Bytes<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: as for `deref`; the guard is the one access there is.
        unsafe { std::slice::from_raw_parts_mut(self.mapped.address.cast(), self.mapped.length) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_is_read_through_another_mapping_of_its_fd() {
        let memory = SharedMemory::new(8).unwrap();
        memory.bytes()[..4].copy_from_slice(&[1, 2, 3, 4]);
        let other = std::fs::read(format!("/proc/self/fd/{}", memory.fd().as_raw_fd())).unwrap();
        assert_eq!(other, [1, 2, 3, 4, 0, 0, 0, 0]);
    }
}
