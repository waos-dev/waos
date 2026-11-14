//! `wasi:sockets/ip-name-lookup@0.3.0`: a lookup the program awaits, answered whole.

use wasmtime::component::{Accessor, Linker};
use wasmtime_wasi::p3::bindings::sockets::types::IpAddress;
use wasmtime_wasi::runtime::spawn_blocking;

use super::{HasNetwork, NetworkCtx, NetworkCtxView};
use crate::bindings::wasi::sockets0_3_0::ip_name_lookup::{self, ErrorCode, HostWithStore};
use crate::network::{Lookup, LookupError};

pub(super) fn add_to_linker<T: NetworkCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    ip_name_lookup::add_to_linker::<_, HasNetwork>(linker, T::network_ctx)
}

impl ip_name_lookup::Host for NetworkCtx<'_> {}

impl<T: NetworkCtxView + 'static> HostWithStore<T> for HasNetwork {
    /// Looks `name` up as the program's network allows; a refusal, or a name that is no host
    /// name, answers at once.
    async fn resolve_addresses(
        accessor: &Accessor<T, Self>,
        name: String,
    ) -> wasmtime::Result<Result<Vec<IpAddress>, ErrorCode>> {
        let (network, planned) = accessor.with(|mut access| {
            let network = access.get().network.clone();
            let planned = network.plan_lookup(&name);
            (network, planned)
        });
        let addresses = match planned {
            Ok(Lookup::Known(addresses)) => Ok(addresses),
            Ok(Lookup::Resolve { name, by_rule }) => {
                spawn_blocking(move || network.resolve(&name, by_rule)).await
            }
            Err(error) => Err(error),
        };
        Ok(addresses
            .map(|addresses| addresses.into_iter().map(IpAddress::from).collect())
            .map_err(error_code))
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
