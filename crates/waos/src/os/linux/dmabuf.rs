//! A dmabuf as the engine holds one: made by GBM on the first render node, linear ARGB8888,
//! exported read-write; its bytes read by the CPU; and the fence a writer of it must wait for,
//! exported as a sync file.
//!
//! The invariants a later editor must keep: a buffer is exported with `DRM_RDWR`, since
//! `gbm_bo_get_fd` exports read-only and a driver that maps the memory, as lavapipe does, refuses
//! such an fd (measured); the CPU reads the memory only between `DMA_BUF_IOCTL_SYNC`'s start and
//! end; and a buffer is drawn into again only after the fence its last reader left (`writers_fence`),
//! since a compositor gives a buffer back with that fence still pending now and then (measured: a
//! few of 187 buffers).

use std::ffi::{c_int, c_void};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow, bail};

/// `DRM_FORMAT_ARGB8888`, BGRA in memory.
pub(crate) const ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
pub(crate) const MOD_LINEAR: u64 = 0;
/// The driver's implicit layout, which a compositor may offer alone.
pub(crate) const MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

/// One dmabuf of one plane in ARGB8888, and the GBM buffer object behind it where the engine
/// allocated it.
pub(crate) struct Dmabuf {
    pub(crate) fd: OwnedFd,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) stride: u32,
    pub(crate) offset: u32,
    pub(crate) modifier: u64,
    _allocation: Option<GbmBufferObject>,
}

impl Dmabuf {
    /// A dmabuf another host made.
    pub(crate) fn of(
        fd: OwnedFd,
        [width, height]: [u32; 2],
        stride: u32,
        offset: u32,
        modifier: u64,
    ) -> Dmabuf {
        Dmabuf {
            fd,
            width,
            height,
            stride,
            offset,
            modifier,
            _allocation: None,
        }
    }

    /// Whether GBM can open a DRM render node on this machine.
    pub(crate) fn can_allocate() -> bool {
        Gbm::get().is_some()
    }

    /// A linear dmabuf of `width` by `height` pixels made by GBM on the first render node.
    pub(crate) fn allocate(width: u32, height: u32) -> Result<Dmabuf> {
        let gbm = Gbm::get().ok_or_else(|| anyhow!("GBM makes no buffers on this machine"))?;
        gbm.allocate(width, height)
    }

    /// The buffer's rows, `width * 4` bytes each, read by the CPU.
    pub(crate) fn read_rows(&self) -> Result<Vec<u8>> {
        if self.modifier != MOD_LINEAR {
            bail!(
                "the CPU reads only a linear buffer, not modifier {:#x}",
                self.modifier
            );
        }
        let length = self.offset as usize + self.stride as usize * self.height as usize;
        let row_bytes = self.width as usize * 4;
        let mut rows = Vec::with_capacity(row_bytes * self.height as usize);
        // SAFETY: a live dmabuf at least `length` bytes long, mapped shared for reading, read only
        // between the sync's start and end, and unmapped before this returns.
        unsafe {
            let mapped = libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ,
                libc::MAP_SHARED,
                self.fd.as_raw_fd(),
                0,
            );
            if mapped == libc::MAP_FAILED {
                bail!("mapping the dmabuf: {}", std::io::Error::last_os_error());
            }
            sync(&self.fd, SYNC_START | SYNC_READ);
            let bytes = std::slice::from_raw_parts(mapped.cast::<u8>(), length);
            for row in 0..self.height as usize {
                let start = self.offset as usize + row * self.stride as usize;
                rows.extend_from_slice(&bytes[start..start + row_bytes]);
            }
            sync(&self.fd, SYNC_END | SYNC_READ);
            libc::munmap(mapped, length);
        }
        Ok(rows)
    }

    /// A sync file of every fence a writer of the buffer must wait for: its readers' and writers'
    /// (`DMA_BUF_IOCTL_EXPORT_SYNC_FILE`, Linux 6.0).
    pub(crate) fn writers_fence(&self) -> Result<OwnedFd> {
        let mut export = ExportSyncFile {
            flags: SYNC_WRITE as u32,
            fd: -1,
        };
        // SAFETY: the ioctl fills `export` with a new fd it owns from here.
        let exported = unsafe {
            libc::ioctl(
                self.fd.as_raw_fd(),
                DMA_BUF_IOCTL_EXPORT_SYNC_FILE as _,
                &mut export,
            )
        };
        if exported != 0 {
            bail!(
                "exporting the dmabuf's fence: {}",
                std::io::Error::last_os_error()
            );
        }
        // SAFETY: the fd the ioctl made, owned by no one else.
        Ok(unsafe { OwnedFd::from_raw_fd(export.fd) })
    }
}

#[repr(C)]
struct DmaBufSync {
    flags: u64,
}

#[repr(C)]
struct ExportSyncFile {
    flags: u32,
    fd: i32,
}

const SYNC_READ: u64 = 1;
const SYNC_WRITE: u64 = 2;
const SYNC_START: u64 = 0;
const SYNC_END: u64 = 4;
const DMA_BUF_IOCTL_SYNC: u64 = ioctl_code(1, b'b', 0, size_of::<DmaBufSync>());
const DMA_BUF_IOCTL_EXPORT_SYNC_FILE: u64 = ioctl_code(3, b'b', 2, size_of::<ExportSyncFile>());

/// Linux's `_IOC`: `direction` 1 writes, 3 reads and writes.
const fn ioctl_code(direction: u64, kind: u8, number: u8, size: usize) -> u64 {
    (direction << 30) | ((size as u64) << 16) | ((kind as u64) << 8) | number as u64
}

fn sync(fd: &OwnedFd, flags: u64) {
    let mut sync = DmaBufSync { flags };
    // SAFETY: a live dmabuf fd and the ioctl's own structure.
    unsafe { libc::ioctl(fd.as_raw_fd(), DMA_BUF_IOCTL_SYNC as _, &mut sync) };
}

/// libgbm's calls, loaded once, with a device on the first render node.
struct Gbm {
    node: File,
    device: usize,
    create: unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    stride: unsafe extern "C" fn(*mut c_void) -> u32,
    offset: unsafe extern "C" fn(*mut c_void, c_int) -> u32,
    handle: unsafe extern "C" fn(*mut c_void) -> u64,
}

// SAFETY: the device is only read after `load`, and each buffer object is used by one owner.
unsafe impl Send for Gbm {}
unsafe impl Sync for Gbm {}

const GBM_BO_USE_RENDERING: u32 = 1 << 2;
const GBM_BO_USE_LINEAR: u32 = 1 << 4;

#[repr(C)]
struct PrimeHandle {
    handle: u32,
    flags: u32,
    fd: i32,
}
const DRM_IOCTL_PRIME_HANDLE_TO_FD: u64 = ioctl_code(3, b'd', 0x2d, size_of::<PrimeHandle>());

/// A GBM buffer object, destroyed when its buffer goes.
struct GbmBufferObject(usize);

impl Drop for GbmBufferObject {
    fn drop(&mut self) {
        if let Some(gbm) = Gbm::get() {
            // SAFETY: a buffer object this device made, destroyed once.
            unsafe { (gbm.destroy)(self.0 as *mut c_void) };
        }
    }
}

impl Gbm {
    fn get() -> Option<&'static Gbm> {
        static GBM: OnceLock<Option<Gbm>> = OnceLock::new();
        GBM.get_or_init(|| {
            Gbm::load()
                .inspect_err(|error| tracing::debug!("GBM render node unavailable: {error:#}"))
                .ok()
        })
        .as_ref()
    }

    fn load() -> Result<Gbm> {
        let node = (128..192)
            .map(|minor| format!("/dev/dri/renderD{minor}"))
            .find_map(|path| OpenOptions::new().read(true).write(true).open(path).ok())
            .context("no DRM render node opens")?;
        // SAFETY: libgbm's documented entry points, with their C signatures; the library stays
        // loaded for the process's life.
        unsafe {
            let library = Box::leak(Box::new(libloading::Library::new("libgbm.so.1")?));
            let create_device: unsafe extern "C" fn(c_int) -> *mut c_void =
                symbol(library, b"gbm_create_device")?;
            let device = create_device(node.as_raw_fd());
            if device.is_null() {
                bail!("GBM made no device on the render node");
            }
            Ok(Gbm {
                device: device as usize,
                create: symbol(library, b"gbm_bo_create")?,
                destroy: symbol(library, b"gbm_bo_destroy")?,
                stride: symbol(library, b"gbm_bo_get_stride")?,
                offset: symbol(library, b"gbm_bo_get_offset")?,
                handle: symbol(library, b"gbm_bo_get_handle")?,
                node,
            })
        }
    }

    fn allocate(&self, width: u32, height: u32) -> Result<Dmabuf> {
        // SAFETY: the device made in `load`; the buffer object is owned by the `Dmabuf` from
        // here, and its handle is exported through the render node it was made on.
        unsafe {
            let object = (self.create)(
                self.device as *mut c_void,
                width,
                height,
                ARGB8888,
                GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR,
            );
            if object.is_null() {
                bail!(
                    "GBM allocated no buffer: {}",
                    std::io::Error::last_os_error()
                );
            }
            let allocation = GbmBufferObject(object as usize);
            let mut prime = PrimeHandle {
                handle: (self.handle)(object) as u32,
                flags: (libc::O_CLOEXEC | libc::O_RDWR) as u32,
                fd: -1,
            };
            if libc::ioctl(
                self.node.as_raw_fd(),
                DRM_IOCTL_PRIME_HANDLE_TO_FD as _,
                &mut prime,
            ) != 0
            {
                bail!("exporting the buffer: {}", std::io::Error::last_os_error());
            }
            Ok(Dmabuf {
                fd: OwnedFd::from_raw_fd(prime.fd),
                width,
                height,
                stride: (self.stride)(object),
                offset: (self.offset)(object, 0),
                modifier: MOD_LINEAR,
                _allocation: Some(allocation),
            })
        }
    }
}

/// The function `name` of `library`, of the type the caller names.
///
/// # Safety
///
/// `T` must be the C signature `name` has.
unsafe fn symbol<T: Copy>(library: &libloading::Library, name: &[u8]) -> Result<T> {
    // SAFETY: the caller names the symbol's type.
    let symbol = unsafe { library.get::<T>(name) }?;
    Ok(*symbol)
}
