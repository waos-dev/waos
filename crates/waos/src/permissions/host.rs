//! The code serving `wasdk:platform/permissions` to a program: a `permission` handle for each
//! guarded capability, its state, the request that asks the user, and its changes.

use wasmtime::component::{
    Access, FutureReader, HasData, Linker, Resource, ResourceTable, StreamReader,
};

use crate::bindings::wasdk::platform::permissions::{
    self, HostPermission, HostPermissionWithStore, State,
};
use crate::stream::stream_of;
use waos_capabilities::{Question, Switch};

use super::{Permission, ProgramPermissions};

/// What the permissions interface reads and writes in a program's store.
pub(crate) struct PermissionsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) permissions: &'a ProgramPermissions,
}

pub(crate) trait PermissionsCtxView: Send {
    fn permissions_ctx(&mut self) -> PermissionsCtx<'_>;
}

struct HasPermissions;

impl HasData for HasPermissions {
    type Data<'a> = PermissionsCtx<'a>;
}

pub(crate) fn add_to_linker<T: PermissionsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    permissions::add_to_linker::<_, HasPermissions>(linker, T::permissions_ctx)
}

impl PermissionsCtx<'_> {
    /// A handle on `permission`.
    fn handle(&mut self, permission: Permission) -> wasmtime::Result<Resource<Permission>> {
        Ok(self.table.push(permission)?)
    }
}

impl permissions::Host for PermissionsCtx<'_> {
    fn network(&mut self) -> wasmtime::Result<Resource<Permission>> {
        self.handle(Permission::Switch(Switch::Network))
    }

    fn local_network(&mut self) -> wasmtime::Result<Resource<Permission>> {
        self.handle(Permission::Switch(Switch::LocalNetwork))
    }

    fn clipboard_read(&mut self) -> wasmtime::Result<Resource<Permission>> {
        self.handle(Permission::Switch(Switch::ClipboardRead))
    }

    fn clipboard_write(&mut self) -> wasmtime::Result<Resource<Permission>> {
        self.handle(Permission::Switch(Switch::ClipboardWrite))
    }

    fn notifications(&mut self) -> wasmtime::Result<Resource<Permission>> {
        self.handle(Permission::Question(Question::Notifications))
    }

    fn camera(&mut self) -> wasmtime::Result<Resource<Permission>> {
        self.handle(Permission::Question(Question::Camera))
    }

    fn microphone(&mut self) -> wasmtime::Result<Resource<Permission>> {
        self.handle(Permission::Question(Question::Microphone))
    }
}

impl HostPermission for PermissionsCtx<'_> {
    fn state(&mut self, permission: Resource<Permission>) -> wasmtime::Result<State> {
        let permission = *self.table.get(&permission)?;
        Ok(self.permissions.state(permission))
    }

    fn drop(&mut self, permission: Resource<Permission>) -> wasmtime::Result<()> {
        self.table.delete(permission)?;
        Ok(())
    }
}

impl<T: PermissionsCtxView + 'static> HostPermissionWithStore<T> for HasPermissions {
    fn request(
        mut access: Access<T, Self>,
        permission: Resource<Permission>,
    ) -> wasmtime::Result<FutureReader<State>> {
        let context = access.get();
        let permission = *context.table.get(&permission)?;
        let requested = context.permissions.request(permission);
        FutureReader::new(
            access,
            async move { Ok::<_, wasmtime::Error>(requested.await) },
        )
    }

    fn changes(
        mut access: Access<T, Self>,
        permission: Resource<Permission>,
    ) -> wasmtime::Result<StreamReader<State>> {
        let context = access.get();
        let permission = *context.table.get(&permission)?;
        let changes = context.permissions.changes(permission);
        stream_of(access, changes, |_: &mut T, state| Ok(state))
    }
}
