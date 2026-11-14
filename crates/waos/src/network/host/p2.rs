//! `wasi:sockets/ip-name-lookup@0.2.12`: a lookup whose answer the program reads address by
//! address, polling until the system's resolver has answered.

use std::net::IpAddr;
use std::vec;

use futures::FutureExt;
use wasmtime::component::{Linker, Resource};
use wasmtime_wasi::p2::{DynPollable, Network, SocketError};
use wasmtime_wasi::runtime::{AbortOnDropJoinHandle, spawn_blocking};
use wasmtime_wasi_io::poll::{Pollable, subscribe};

use super::{HasNetwork, NetworkCtx, NetworkCtxView};
use crate::bindings::wasi::sockets0_2_12::ip_name_lookup::{
    self, HostResolveAddressStream, IpAddress,
};
use crate::network::{Lookup, LookupError, ProgramNetwork};
use wasmtime_wasi::p2::bindings::sockets::network::{self, ErrorCode, HostNetwork};

pub(super) fn add_to_linker<T: NetworkCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    ip_name_lookup::add_to_linker::<_, HasNetwork>(linker, T::network_ctx)
}

/// A lookup's answer, as the program reads it: the system's resolver still asked, or the
/// addresses left to read, or why there are none.
pub struct ResolveAddressStream(Answer);

enum Answer {
    Resolving(AbortOnDropJoinHandle<Result<Vec<IpAddr>, LookupError>>),
    Resolved(Result<vec::IntoIter<IpAddr>, LookupError>),
}

impl ResolveAddressStream {
    /// The answer of `lookup`: at once for addresses known, after the system's resolver
    /// otherwise.
    fn of(lookup: Lookup, network: &ProgramNetwork) -> ResolveAddressStream {
        match lookup {
            Lookup::Known(addresses) => {
                ResolveAddressStream(Answer::Resolved(Ok(addresses.into_iter())))
            }
            Lookup::Resolve { name, by_rule } => {
                let network = network.clone();
                let resolving = spawn_blocking(move || network.resolve(&name, by_rule));
                ResolveAddressStream(Answer::Resolving(resolving))
            }
        }
    }

    /// The next address, none after the last, or `would-block` while the resolver is asked.
    fn next(&mut self) -> Result<Option<IpAddr>, ErrorCode> {
        if let Answer::Resolving(resolving) = &mut self.0 {
            let Some(resolved) = resolving.now_or_never() else {
                return Err(ErrorCode::WouldBlock);
            };
            self.0 = Answer::Resolved(resolved.map(Vec::into_iter));
        }
        match &mut self.0 {
            Answer::Resolved(Ok(addresses)) => Ok(addresses.next()),
            Answer::Resolved(Err(error)) => Err(error_code(*error)),
            Answer::Resolving(_) => unreachable!("resolved above"),
        }
    }
}

#[wasmtime_wasi_io::async_trait]
impl Pollable for ResolveAddressStream {
    async fn ready(&mut self) {
        if let Answer::Resolving(resolving) = &mut self.0 {
            let resolved = resolving.await;
            self.0 = Answer::Resolved(resolved.map(Vec::into_iter));
        }
    }
}

impl ip_name_lookup::Host for NetworkCtx<'_> {
    /// Looks `name` up as the program's network allows; a refusal, or a name that is no host
    /// name, answers at once.
    fn resolve_addresses(
        &mut self,
        network: Resource<Network>,
        name: String,
    ) -> Result<Resource<ResolveAddressStream>, SocketError> {
        // The network handle is the capability to look a name up; nothing else of it is used.
        self.table.get(&network)?;
        let lookup = self.network.plan_lookup(&name).map_err(error_code)?;
        let answer = ResolveAddressStream::of(lookup, self.network);
        Ok(self.table.push(answer)?)
    }
}

/// What the bindings ask of a lookup's host for `network`'s errors, which its functions answer
/// with. The interface itself is `wasmtime-wasi`'s, linked by `wasi`; these are not linked from
/// here.
impl network::Host for NetworkCtx<'_> {
    fn convert_error_code(&mut self, error: SocketError) -> wasmtime::Result<ErrorCode> {
        error.downcast()
    }

    fn network_error_code(
        &mut self,
        error: Resource<wasmtime::Error>,
    ) -> wasmtime::Result<Option<ErrorCode>> {
        let error = self.table.get(&error)?;
        Ok(error.downcast_ref::<std::io::Error>().map(ErrorCode::from))
    }
}

impl HostNetwork for NetworkCtx<'_> {
    fn drop(&mut self, network: Resource<Network>) -> wasmtime::Result<()> {
        self.table.delete(network)?;
        Ok(())
    }
}

impl HostResolveAddressStream for NetworkCtx<'_> {
    fn resolve_next_address(
        &mut self,
        answer: Resource<ResolveAddressStream>,
    ) -> Result<Option<IpAddress>, SocketError> {
        let next = self.table.get_mut(&answer)?.next()?;
        Ok(next.map(IpAddress::from))
    }

    fn subscribe(
        &mut self,
        answer: Resource<ResolveAddressStream>,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        subscribe(self.table, answer)
    }

    fn drop(&mut self, answer: Resource<ResolveAddressStream>) -> wasmtime::Result<()> {
        self.table.delete(answer)?;
        Ok(())
    }
}

/// WASI's code for why a lookup answers no address.
fn error_code(error: LookupError) -> ErrorCode {
    match error {
        LookupError::Refused => ErrorCode::AccessDenied,
        LookupError::Invalid => ErrorCode::InvalidArgument,
        LookupError::Unresolvable => ErrorCode::NameUnresolvable,
    }
}
