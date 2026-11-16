//! WASI's code for what the system answers a socket's call with: its error number where it gives
//! one, the kind of the I/O error otherwise.

use rustix::io::Errno;
use tracing::debug;

use crate::network::host::bindings::wasi::sockets::types::ErrorCode;

impl From<std::io::Error> for ErrorCode {
    fn from(value: std::io::Error) -> Self {
        (&value).into()
    }
}

impl From<&std::io::Error> for ErrorCode {
    fn from(value: &std::io::Error) -> Self {
        if let Some(errno) = Errno::from_io_error(value) {
            return errno.into();
        }
        match value.kind() {
            std::io::ErrorKind::AddrInUse => Self::AddressInUse,
            std::io::ErrorKind::AddrNotAvailable => Self::AddressNotBindable,
            std::io::ErrorKind::ConnectionAborted => Self::ConnectionAborted,
            std::io::ErrorKind::ConnectionRefused => Self::ConnectionRefused,
            std::io::ErrorKind::ConnectionReset => Self::ConnectionReset,
            std::io::ErrorKind::InvalidInput => Self::InvalidArgument,
            std::io::ErrorKind::NotConnected => Self::InvalidState,
            std::io::ErrorKind::OutOfMemory => Self::OutOfMemory,
            std::io::ErrorKind::PermissionDenied => Self::AccessDenied,
            std::io::ErrorKind::TimedOut => Self::Timeout,
            std::io::ErrorKind::Unsupported => Self::NotSupported,
            std::io::ErrorKind::HostUnreachable => Self::RemoteUnreachable,
            std::io::ErrorKind::NetworkUnreachable => Self::RemoteUnreachable,
            std::io::ErrorKind::NetworkDown => Self::RemoteUnreachable,
            std::io::ErrorKind::BrokenPipe => Self::ConnectionBroken,
            _ => {
                debug!("unknown I/O error: {value}");
                Self::Other(None)
            }
        }
    }
}

impl From<Errno> for ErrorCode {
    fn from(value: Errno) -> Self {
        (&value).into()
    }
}

impl From<&Errno> for ErrorCode {
    fn from(value: &Errno) -> Self {
        match *value {
            #[cfg(not(windows))]
            Errno::PERM => Self::AccessDenied,
            Errno::ACCESS => Self::AccessDenied,
            Errno::ADDRINUSE => Self::AddressInUse,
            Errno::ADDRNOTAVAIL => Self::AddressNotBindable,
            Errno::TIMEDOUT => Self::Timeout,
            #[cfg(not(windows))]
            Errno::PIPE => Self::ConnectionBroken,
            Errno::CONNREFUSED => Self::ConnectionRefused,
            Errno::CONNRESET => Self::ConnectionReset,
            Errno::CONNABORTED => Self::ConnectionAborted,
            Errno::INVAL => Self::InvalidArgument,
            Errno::HOSTUNREACH => Self::RemoteUnreachable,
            Errno::HOSTDOWN => Self::RemoteUnreachable,
            Errno::NETDOWN => Self::RemoteUnreachable,
            Errno::NETUNREACH => Self::RemoteUnreachable,
            #[cfg(target_os = "linux")]
            Errno::NONET => Self::RemoteUnreachable,
            Errno::ISCONN => Self::InvalidState,
            Errno::NOTCONN => Self::InvalidState,
            Errno::DESTADDRREQ => Self::InvalidState,
            Errno::MSGSIZE => Self::DatagramTooLarge,
            #[cfg(not(windows))]
            Errno::NOMEM => Self::OutOfMemory,
            Errno::NOBUFS => Self::OutOfMemory,
            Errno::OPNOTSUPP => Self::NotSupported,
            Errno::NOPROTOOPT => Self::NotSupported,
            Errno::PFNOSUPPORT => Self::NotSupported,
            Errno::PROTONOSUPPORT => Self::NotSupported,
            Errno::PROTOTYPE => Self::NotSupported,
            Errno::SOCKTNOSUPPORT => Self::NotSupported,
            Errno::AFNOSUPPORT => Self::NotSupported,
            // A non-blocking connect's `EINPROGRESS` is taken by tokio before it reaches here.
            _ => {
                debug!("unknown I/O error: {value}");
                Self::Other(None)
            }
        }
    }
}
