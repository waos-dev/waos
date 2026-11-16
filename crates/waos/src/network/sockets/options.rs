//! A socket's options as WASI's sockets give them, the same on every system: a hop limit of 0 and
//! a buffer size of 0 refused, a buffer size the system cannot take clamped as most systems clamp
//! it, Linux's doubled buffer sizes halved back, keep-alive times and counts kept within what every
//! system takes; and, on macOS, which does not give a connection it accepts the options set on its
//! listener, those options applied to it ([`ListenerOptions`]).

use std::time::Duration;

use rustix::fd::AsFd;
use rustix::io::Errno;
use rustix::net::sockopt;

use crate::network::host::bindings::wasi::sockets::types::{ErrorCode, IpAddressFamily};

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn unicast_hop_limit(fd: impl AsFd, family: IpAddressFamily) -> Result<u8, ErrorCode> {
    match family {
        IpAddressFamily::Ipv4 => {
            let ttl = sockopt::ip_ttl(fd)?;
            ttl.try_into().map_err(|_| ErrorCode::NotSupported)
        }
        IpAddressFamily::Ipv6 => Ok(sockopt::ipv6_unicast_hops(fd)?),
    }
}

pub(super) fn set_unicast_hop_limit(
    fd: impl AsFd,
    family: IpAddressFamily,
    value: u8,
) -> Result<(), ErrorCode> {
    // Checked here: systems differ on 0, Linux even between IPv4 and IPv6.
    if value == 0 {
        return Err(ErrorCode::InvalidArgument);
    }
    match family {
        IpAddressFamily::Ipv4 => sockopt::set_ip_ttl(fd, value.into())?,
        IpAddressFamily::Ipv6 => sockopt::set_ipv6_unicast_hops(fd, Some(value))?,
    }
    Ok(())
}

pub(super) fn receive_buffer_size(fd: impl AsFd) -> Result<u64, ErrorCode> {
    let size = sockopt::socket_recv_buffer_size(fd)?;
    Ok(size_as_set(size))
}

/// Sets the receive buffer's size, and answers the size asked of the system.
pub(super) fn set_receive_buffer_size(fd: impl AsFd, value: u64) -> Result<usize, ErrorCode> {
    let size = size_to_set(value)?;
    taken_as_hint(sockopt::set_socket_recv_buffer_size(fd, size))?;
    Ok(size)
}

pub(super) fn send_buffer_size(fd: impl AsFd) -> Result<u64, ErrorCode> {
    let size = sockopt::socket_send_buffer_size(fd)?;
    Ok(size_as_set(size))
}

/// Sets the send buffer's size, and answers the size asked of the system.
pub(super) fn set_send_buffer_size(fd: impl AsFd, value: u64) -> Result<usize, ErrorCode> {
    let size = size_to_set(value)?;
    taken_as_hint(sockopt::set_socket_send_buffer_size(fd, size))?;
    Ok(size)
}

/// A buffer size as the program set it: Linux answers twice what it was given, its bookkeeping
/// counted in.
fn size_as_set(size: usize) -> u64 {
    let size = if cfg!(target_os = "linux") {
        size / 2
    } else {
        size
    };
    size.try_into().unwrap_or(u64::MAX)
}

/// The buffer size the system is asked for: `value`, within what the call takes; 0 refused.
fn size_to_set(value: u64) -> Result<usize, ErrorCode> {
    if value == 0 {
        return Err(ErrorCode::InvalidArgument);
    }
    let value: usize = value.try_into().unwrap_or(usize::MAX);
    Ok(value.clamp(1, i32::MAX as usize))
}

/// A buffer size set as a hint, as most systems take it: the BSDs refuse one out of range with
/// `ENOBUFS`, which is taken as set.
fn taken_as_hint(set: Result<(), Errno>) -> Result<(), ErrorCode> {
    match set {
        Err(Errno::NOBUFS) | Ok(()) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// A keep-alive time in nanoseconds, kept at a second at least, so the system never rounds it to
/// 0, and within Linux's limit, the lowest of the systems.
pub(super) fn clamp_keep_alive_time(value: u64) -> u64 {
    value.clamp(NANOS_PER_SEC, (i16::MAX as u64) * NANOS_PER_SEC)
}

/// A keep-alive count, within Linux's limit, the lowest of the systems.
pub(super) fn clamp_keep_alive_count(value: u32) -> u32 {
    value.clamp(1, i8::MAX as u32)
}

pub(super) fn keep_alive_time(time: Duration) -> u64 {
    time.as_nanos().try_into().unwrap_or(u64::MAX)
}

#[cfg(not(target_os = "macos"))]
pub(super) use inherited::ListenerOptions;
#[cfg(target_os = "macos")]
pub(super) use not_inherited::ListenerOptions;

/// Where a connection accepted takes its listener's options from the system itself.
#[cfg(not(target_os = "macos"))]
mod inherited {
    use tokio::net::TcpStream;

    use crate::network::host::bindings::wasi::sockets::types::IpAddressFamily;

    /// Nothing to keep: the system gives each accepted connection its listener's options.
    #[derive(Default, Clone)]
    pub(crate) struct ListenerOptions;

    impl ListenerOptions {
        pub(crate) fn set_keep_alive_idle_time(&mut self, _value: u64) {}

        pub(crate) fn set_hop_limit(&mut self, _value: u8) {}

        pub(crate) fn set_receive_buffer_size(&mut self, _value: usize) {}

        pub(crate) fn set_send_buffer_size(&mut self, _value: usize) {}

        pub(crate) fn apply(&self, _family: IpAddressFamily, _stream: &TcpStream) {}
    }
}

/// macOS, which gives an accepted connection none of these options of its listener's.
#[cfg(target_os = "macos")]
mod not_inherited {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering::Relaxed};
    use std::time::Duration;

    use rustix::net::sockopt;
    use tokio::net::TcpStream;

    use crate::network::host::bindings::wasi::sockets::types::IpAddressFamily;

    /// The options the program set on a listener, each 0 until set, which every connection it
    /// accepts is given.
    #[derive(Default, Clone)]
    pub(crate) struct ListenerOptions(Arc<Set>);

    #[derive(Default)]
    struct Set {
        receive_buffer_size: AtomicUsize,
        send_buffer_size: AtomicUsize,
        hop_limit: AtomicU8,
        /// In nanoseconds.
        keep_alive_idle_time: AtomicU64,
    }

    impl ListenerOptions {
        pub(crate) fn set_keep_alive_idle_time(&mut self, value: u64) {
            self.0.keep_alive_idle_time.store(value, Relaxed);
        }

        pub(crate) fn set_hop_limit(&mut self, value: u8) {
            self.0.hop_limit.store(value, Relaxed);
        }

        pub(crate) fn set_receive_buffer_size(&mut self, value: usize) {
            self.0.receive_buffer_size.store(value, Relaxed);
        }

        pub(crate) fn set_send_buffer_size(&mut self, value: usize) {
            self.0.send_buffer_size.store(value, Relaxed);
        }

        /// Gives `stream` each option set, as well as the system takes it.
        pub(crate) fn apply(&self, family: IpAddressFamily, stream: &TcpStream) {
            let receive_buffer_size = self.0.receive_buffer_size.load(Relaxed);
            if receive_buffer_size > 0 {
                _ = sockopt::set_socket_recv_buffer_size(stream, receive_buffer_size);
            }
            let send_buffer_size = self.0.send_buffer_size.load(Relaxed);
            if send_buffer_size > 0 {
                _ = sockopt::set_socket_send_buffer_size(stream, send_buffer_size);
            }
            // IP_TTL is inherited; IPV6_UNICAST_HOPS is not.
            let hop_limit = self.0.hop_limit.load(Relaxed);
            if family == IpAddressFamily::Ipv6 && hop_limit > 0 {
                _ = sockopt::set_ipv6_unicast_hops(stream, Some(hop_limit));
            }
            let keep_alive_idle_time = self.0.keep_alive_idle_time.load(Relaxed);
            if keep_alive_idle_time > 0 {
                _ = sockopt::set_tcp_keepidle(stream, Duration::from_nanos(keep_alive_idle_time));
            }
        }
    }
}
