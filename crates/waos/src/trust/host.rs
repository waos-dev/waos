//! The code serving `wasdk:platform/trust` to a program: its handle on the system's checker, and
//! each check made on one of tokio's threads for blocking work, while the program goes on.

use std::sync::Arc;

use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable};
use wasmtime_wasi::runtime::spawn_blocking;

use super::Trust;
use crate::bindings::wasdk::platform::trust::{self, HostTrust, HostTrustWithStore, TrustError};

/// What the trust interface reads and writes in a program's store.
pub(crate) struct TrustCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
}

pub(crate) trait TrustCtxView: Send {
    fn trust_ctx(&mut self) -> TrustCtx<'_>;
}

struct HasTrust;

impl HasData for HasTrust {
    type Data<'a> = TrustCtx<'a>;
}

pub(crate) fn add_to_linker<T: TrustCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    trust::add_to_linker::<_, HasTrust>(linker, T::trust_ctx)
}

impl trust::Host for TrustCtx<'_> {
    fn get_trust(&mut self) -> wasmtime::Result<Option<Resource<Trust>>> {
        Trust::system()
            .map(|trust| Ok(self.table.push(trust)?))
            .transpose()
    }
}

impl HostTrust for TrustCtx<'_> {
    fn drop(&mut self, trust: Resource<Trust>) -> wasmtime::Result<()> {
        self.table.delete(trust)?;
        Ok(())
    }
}

impl<T: TrustCtxView + 'static> HostTrustWithStore<T> for HasTrust {
    async fn verify_server(
        accessor: &Accessor<T, Self>,
        trust: Resource<Trust>,
        chain: Vec<Vec<u8>>,
        server_name: String,
    ) -> wasmtime::Result<Result<(), TrustError>> {
        let verifier = accessor.with(|mut access| {
            let trust = access.get().table.get(&trust)?;
            Ok::<_, wasmtime::Error>(Arc::clone(&trust.verifier))
        })?;
        let trust = Trust { verifier };
        Ok(spawn_blocking(move || trust.verify(&chain, &server_name)).await)
    }
}
