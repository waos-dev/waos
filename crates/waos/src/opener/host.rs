//! The code serving `wasdk:platform/opener` to a program: the shell's opener, where it has one, a
//! link opened once its scheme is allowed, and a file opened or shown where it is on the machine.

use std::path::Path;
use std::sync::Arc;

use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable};

use super::{LinkSchemes, Opener, scheme_of};
use crate::access::Cause;
use crate::bindings::wasdk::platform::opener::{self, HostOpener, HostOpenerWithStore, OpenError};
use crate::files::ProgramFiles;
use crate::running::RunningProgram;
use crate::shell::{OpenFuture, ShellOpener};

/// What the opener interface reads and writes in a program's store.
pub(crate) struct OpenerCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a RunningProgram,
    /// The shell's opener; none where the shell has none.
    pub(crate) opener: Option<&'a Arc<dyn ShellOpener>>,
    /// The program's mounts, where a file it names is looked up.
    pub(crate) files: &'a mut ProgramFiles,
    pub(crate) links: &'a LinkSchemes,
}

pub(crate) trait OpenerCtxView: Send {
    fn opener_ctx(&mut self) -> OpenerCtx<'_>;
}

struct HasOpener;

impl HasData for HasOpener {
    type Data<'a> = OpenerCtx<'a>;
}

pub(crate) fn add_to_linker<T: OpenerCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    opener::add_to_linker::<_, HasOpener>(linker, T::opener_ctx)
}

impl opener::Host for OpenerCtx<'_> {
    fn get_opener(&mut self) -> wasmtime::Result<Option<Resource<Opener>>> {
        let place = self.opener.cloned().map(Opener);
        Ok(place.map(|opener| self.table.push(opener)).transpose()?)
    }
}

impl HostOpener for OpenerCtx<'_> {
    fn drop(&mut self, opener: Resource<Opener>) -> wasmtime::Result<()> {
        self.table.delete(opener)?;
        Ok(())
    }
}

impl<T: OpenerCtxView + 'static> HostOpenerWithStore<T> for HasOpener {
    /// Asks the user first for a scheme that needs it, and opens nothing it refused.
    async fn open_link(
        accessor: &Accessor<T, Self>,
        opener: Resource<Opener>,
        link: String,
    ) -> wasmtime::Result<Result<(), OpenError>> {
        let Some(scheme) = scheme_of(&link) else {
            return Ok(Err(OpenError::Failed(format!("{link:?} is not a link"))));
        };
        let allowed = accessor.with(|mut access| access.get().links.allow(&link, &scheme));
        if !allowed.await {
            return Ok(Err(OpenError::Denied));
        }
        let opening = accessor.with(|mut access| {
            let context = access.get();
            let shell = &context.table.get(&opener)?.0;
            wasmtime::Result::<_>::Ok(shell.open_link(context.program, &link))
        })?;
        Ok(opening.await)
    }

    async fn open_file(
        accessor: &Accessor<T, Self>,
        opener: Resource<Opener>,
        path: String,
    ) -> wasmtime::Result<Result<(), OpenError>> {
        let opening = accessor.with(|mut access| {
            with_machine_path(access.get(), &opener, &path, |shell, program, path| {
                shell.open_file(program, path)
            })
        })?;
        Ok(opening.await)
    }

    async fn reveal_file(
        accessor: &Accessor<T, Self>,
        opener: Resource<Opener>,
        path: String,
    ) -> wasmtime::Result<Result<(), OpenError>> {
        let revealing = accessor.with(|mut access| {
            with_machine_path(access.get(), &opener, &path, |shell, program, path| {
                shell.reveal_file(program, path)
            })
        })?;
        Ok(revealing.await)
    }
}

/// What `call` asks of the shell's opener for the file at the program's `path`, where it is on the
/// machine; a failure, asking nothing, for a path outside the program's files.
fn with_machine_path(
    context: OpenerCtx<'_>,
    opener: &Resource<Opener>,
    path: &str,
    call: impl FnOnce(&dyn ShellOpener, &RunningProgram, &Path) -> OpenFuture,
) -> wasmtime::Result<OpenFuture> {
    let Some(machine_path) = context.files.machine_path(Path::new(path), Cause::Opener) else {
        let failed = OpenError::Failed(format!("no file at {path} among the program's"));
        return Ok(Box::pin(std::future::ready(Err(failed))));
    };
    let shell = &context.table.get(opener)?.0;
    Ok(call(shell.as_ref(), context.program, &machine_path))
}
