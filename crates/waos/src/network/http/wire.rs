//! What a request is sent over, as hyper takes it: a connection of the program's (`sockets`),
//! straight to its host or through a proxy, in TLS or not, and whether it goes to an HTTP proxy,
//! to which hyper sends the request's whole URL. One type for every way, so one client sends
//! them all; hyper reads and writes it as the stream it carries.

use std::io::IoSlice;
use std::pin::Pin;
use std::task::{Context, Poll};

use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};

/// A stream a request may be sent over.
pub(super) trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// A connection a request is sent over.
pub(super) struct Wire {
    io: TokioIo<Box<dyn Stream>>,
    /// Whether it goes to an HTTP proxy, which is sent the request's whole URL.
    to_proxy: bool,
}

impl Wire {
    pub(super) fn new(stream: Box<dyn Stream>, to_proxy: bool) -> Wire {
        Wire {
            io: TokioIo::new(stream),
            to_proxy,
        }
    }
}

impl Connection for Wire {
    fn connected(&self) -> Connected {
        Connected::new().proxy(self.to_proxy)
    }
}

impl Read for Wire {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: ReadBufCursor<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buffer)
    }
}

impl Write for Wire {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buffer)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.io).poll_write_vectored(cx, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
