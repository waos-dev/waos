//! `wasi:sockets/types@0.3.0`'s `udp-socket`: each method the socket's own call on the handle the
//! program holds, a datagram's send and receive awaited off the program's table.

use std::net::SocketAddr;

use wasmtime::component::{Accessor, Resource, ResourceTable};

use super::bindings::wasi::sockets::types::{
    HostUdpSocket, HostUdpSocketWithStore, IpAddressFamily, IpSocketAddress,
};
use super::{HasNetwork, NetworkCtx, SocketError, SocketResult};
use crate::network::sockets::UdpSocket;

fn socket<'a>(
    table: &'a ResourceTable,
    socket: &Resource<UdpSocket>,
) -> SocketResult<&'a UdpSocket> {
    table.get(socket).map_err(SocketError::trap)
}

fn socket_mut<'a>(
    table: &'a mut ResourceTable,
    socket: &Resource<UdpSocket>,
) -> SocketResult<&'a mut UdpSocket> {
    table.get_mut(socket).map_err(SocketError::trap)
}

impl<T> HostUdpSocketWithStore<T> for HasNetwork {
    async fn send(
        accessor: &Accessor<T, Self>,
        handle: Resource<UdpSocket>,
        data: Vec<u8>,
        address: Option<IpSocketAddress>,
    ) -> SocketResult<()> {
        let sent = accessor.with(|mut access| {
            let socket = socket_mut(access.get().table, &handle)?;
            SocketResult::Ok(socket.send(data, address.map(SocketAddr::from)))
        })?;
        Ok(sent.await?)
    }

    async fn receive(
        accessor: &Accessor<T, Self>,
        handle: Resource<UdpSocket>,
    ) -> SocketResult<(Vec<u8>, IpSocketAddress)> {
        let received = accessor.with(|mut access| {
            let socket = socket_mut(access.get().table, &handle)?;
            SocketResult::Ok(socket.receive())
        })?;
        let (data, from) = received.await?;
        Ok((data, from.into()))
    }
}

impl HostUdpSocket for NetworkCtx<'_> {
    async fn create(&mut self, family: IpAddressFamily) -> SocketResult<Resource<UdpSocket>> {
        let socket = UdpSocket::new(self.network.clone(), family).await?;
        self.table.push(socket).map_err(SocketError::trap)
    }

    fn bind(&mut self, handle: Resource<UdpSocket>, address: IpSocketAddress) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.bind(SocketAddr::from(address))?)
    }

    fn connect(
        &mut self,
        handle: Resource<UdpSocket>,
        address: IpSocketAddress,
    ) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.connect(SocketAddr::from(address))?)
    }

    fn disconnect(&mut self, handle: Resource<UdpSocket>) -> SocketResult<()> {
        Ok(socket_mut(self.table, &handle)?.disconnect()?)
    }

    fn get_local_address(&mut self, handle: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(socket_mut(self.table, &handle)?.local_address()?.into())
    }

    fn get_remote_address(&mut self, handle: Resource<UdpSocket>) -> SocketResult<IpSocketAddress> {
        Ok(socket(self.table, &handle)?.remote_address()?.into())
    }

    fn get_address_family(
        &mut self,
        handle: Resource<UdpSocket>,
    ) -> wasmtime::Result<IpAddressFamily> {
        Ok(self.table.get(&handle)?.address_family())
    }

    fn get_unicast_hop_limit(&mut self, handle: Resource<UdpSocket>) -> SocketResult<u8> {
        Ok(socket(self.table, &handle)?.unicast_hop_limit()?)
    }

    fn set_unicast_hop_limit(
        &mut self,
        handle: Resource<UdpSocket>,
        value: u8,
    ) -> SocketResult<()> {
        Ok(socket(self.table, &handle)?.set_unicast_hop_limit(value)?)
    }

    fn get_receive_buffer_size(&mut self, handle: Resource<UdpSocket>) -> SocketResult<u64> {
        Ok(socket(self.table, &handle)?.receive_buffer_size()?)
    }

    fn set_receive_buffer_size(
        &mut self,
        handle: Resource<UdpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        Ok(socket(self.table, &handle)?.set_receive_buffer_size(value)?)
    }

    fn get_send_buffer_size(&mut self, handle: Resource<UdpSocket>) -> SocketResult<u64> {
        Ok(socket(self.table, &handle)?.send_buffer_size()?)
    }

    fn set_send_buffer_size(
        &mut self,
        handle: Resource<UdpSocket>,
        value: u64,
    ) -> SocketResult<()> {
        Ok(socket(self.table, &handle)?.set_send_buffer_size(value)?)
    }

    fn drop(&mut self, handle: Resource<UdpSocket>) -> wasmtime::Result<()> {
        self.table.delete(handle)?;
        Ok(())
    }
}
