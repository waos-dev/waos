//! `wasi:sockets/types@0.3.0`'s `tcp-socket`: each method the socket's own call on the handle the
//! program holds.

use std::net::SocketAddr;
use std::task::{Context, Poll};

use wasmtime::AsContextMut;
use wasmtime::component::{Access, Accessor, FutureReader, Resource, ResourceTable, StreamReader};

use super::bindings::wasi::sockets::types::{
    Duration, ErrorCode, HostTcpSocket, HostTcpSocketWithStore, IpAddressFamily, IpSocketAddress,
};
use super::streams::{self, ListenStreamProducer, StreamEnding};
use super::{HasNetwork, NetworkCtx, SocketError, SocketResult};
use crate::network::sockets::TcpSocket;

fn socket<'a>(
    table: &'a ResourceTable,
    socket: &Resource<TcpSocket>,
) -> SocketResult<&'a TcpSocket> {
    table.get(socket).map_err(SocketError::trap)
}

fn socket_mut<'a>(
    table: &'a mut ResourceTable,
    socket: &Resource<TcpSocket>,
) -> SocketResult<&'a mut TcpSocket> {
    table.get_mut(socket).map_err(SocketError::trap)
}

impl NetworkCtx<'_> {
    fn start_connect(
        &mut self,
        handle: &Resource<TcpSocket>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        Ok(socket_mut(self.table, handle)?.start_connect(SocketAddr::from(address))?)
    }

    fn poll_finish_connect(
        &mut self,
        cx: &mut Context<'_>,
        handle: &Resource<TcpSocket>,
    ) -> Poll<SocketResult<()>> {
        match socket_mut(self.table, handle) {
            Ok(socket) => socket.poll_finish_connect(cx).map_err(SocketError::from),
            Err(trap) => Poll::Ready(Err(trap)),
        }
    }
}

impl<T: Send> HostTcpSocketWithStore<T> for HasNetwork {
    async fn connect(
        accessor: &Accessor<T, Self>,
        handle: Resource<TcpSocket>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        accessor.with(|mut access| access.get().start_connect(&handle, address))?;
        std::future::poll_fn(|cx| {
            accessor.with(|mut access| access.get().poll_finish_connect(cx, &handle))
        })
        .await
    }

    fn listen(
        mut access: Access<'_, T, Self>,
        handle: Resource<TcpSocket>,
    ) -> SocketResult<StreamReader<Resource<TcpSocket>>> {
        let getter = access.getter();
        let listener = socket_mut(access.get().table, &handle)?.listen()?;
        StreamReader::new(&mut access, ListenStreamProducer { listener, getter })
            .map_err(SocketError::trap)
    }

    fn send(
        mut access: Access<'_, T, Self>,
        handle: Resource<TcpSocket>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<FutureReader<Result<(), ErrorCode>>> {
        let stream = access.get().table.get_mut(&handle)?.take_send_stream();
        streams::send(access.as_context_mut(), stream, data)
    }

    fn receive(
        mut access: Access<'_, T, Self>,
        handle: Resource<TcpSocket>,
    ) -> wasmtime::Result<(StreamReader<u8>, StreamEnding)> {
        let stream = access.get().table.get_mut(&handle)?.take_receive_stream();
        streams::receive(access.as_context_mut(), stream)
    }
}

impl HostTcpSocket for NetworkCtx<'_> {
    fn create(&mut self, family: IpAddressFamily) -> SocketResult<Resource<TcpSocket>> {
        let socket = TcpSocket::new(self.network.clone(), family)?;
        self.table.push(socket).map_err(SocketError::trap)
    }

    fn bind(&mut self, handle: Resource<TcpSocket>, address: IpSocketAddress) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.bind(SocketAddr::from(address))?)
    }

    fn get_local_address(&mut self, handle: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(socket_mut(self.table, &handle)?.local_address()?.into())
    }

    fn get_remote_address(&mut self, handle: Resource<TcpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(socket_mut(self.table, &handle)?.remote_address()?.into())
    }

    fn get_is_listening(&mut self, handle: Resource<TcpSocket>) -> wasmtime::Result<bool> {
        Ok(self.table.get(&handle)?.is_listening())
    }

    fn get_address_family(
        &mut self,
        handle: Resource<TcpSocket>,
    ) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&handle)?.address_family())
    }

    fn set_listen_backlog_size(
        &mut self,
        handle: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.set_listen_backlog_size(value)?)
    }

    fn get_keep_alive_enabled(&mut self, handle: Resource<TcpSocket>) -> SocketResult<bool> {
        Ok(socket(self.table, &handle)?.keep_alive_enabled()?)
    }

    fn set_keep_alive_enabled(
        &mut self,
        handle: Resource<TcpSocket>,
        value: bool,
    ) -> SocketResult<()> {
        Ok(socket(self.table, &handle)?.set_keep_alive_enabled(value)?)
    }

    fn get_keep_alive_idle_time(&mut self, handle: Resource<TcpSocket>) -> SocketResult<Duration> {
        Ok(socket(self.table, &handle)?.keep_alive_idle_time()?)
    }

    fn set_keep_alive_idle_time(
        &mut self,
        handle: Resource<TcpSocket>,
        value: Duration,
    ) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.set_keep_alive_idle_time(value)?)
    }

    fn get_keep_alive_interval(&mut self, handle: Resource<TcpSocket>) -> SocketResult<Duration> {
        Ok(socket(self.table, &handle)?.keep_alive_interval()?)
    }

    fn set_keep_alive_interval(
        &mut self,
        handle: Resource<TcpSocket>,
        value: Duration,
    ) -> SocketResult<()> {
        Ok(socket(self.table, &handle)?.set_keep_alive_interval(value)?)
    }

    fn get_keep_alive_count(&mut self, handle: Resource<TcpSocket>) -> SocketResult<u32> {
        Ok(socket(self.table, &handle)?.keep_alive_count()?)
    }

    fn set_keep_alive_count(
        &mut self,
        handle: Resource<TcpSocket>,
        value: u32,
    ) -> SocketResult<()> {
        Ok(socket(self.table, &handle)?.set_keep_alive_count(value)?)
    }

    fn get_hop_limit(&mut self, handle: Resource<TcpSocket>) -> SocketResult<u8> {
        Ok(socket(self.table, &handle)?.hop_limit()?)
    }

    fn set_hop_limit(&mut self, handle: Resource<TcpSocket>, value: u8) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.set_hop_limit(value)?)
    }

    fn get_receive_buffer_size(&mut self, handle: Resource<TcpSocket>) -> SocketResult<u64> {
        Ok(socket(self.table, &handle)?.receive_buffer_size()?)
    }

    fn set_receive_buffer_size(
        &mut self,
        handle: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.set_receive_buffer_size(value)?)
    }

    fn get_send_buffer_size(&mut self, handle: Resource<TcpSocket>) -> SocketResult<u64> {
        Ok(socket(self.table, &handle)?.send_buffer_size()?)
    }

    fn set_send_buffer_size(
        &mut self,
        handle: Resource<TcpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.set_send_buffer_size(value)?)
    }

    fn drop(&mut self, handle: Resource<TcpSocket>) -> wasmtime::Result<()> {
        self.table.delete(handle)?;
        Ok(())
    }
}
