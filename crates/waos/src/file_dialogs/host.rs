//! The code serving `wasdk:platform/file-dialogs` to a program: the options as resources with
//! setters, each dialog shown through the shell or answered as its user set it, and each pick
//! mounted before it is answered. A pick is the user's allowing the program a file or folder, so
//! where the system guards it with a permission of its own and does not allow it yet, the system
//! is asked before it is mounted, and a pick it still denies is answered as a cancelled dialog:
//! nothing is mounted that WaOS cannot read.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use waos_capabilities::FileDialog;

use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable};

use super::{FileDialogs, FileType, FolderOptions, OpenOptions, SaveOptions, set_answers};
use crate::access::Cause;
use crate::bindings::wasdk::platform::file_dialogs::{
    self, DialogError, HostFileDialogs, HostFileDialogsWithStore, HostFolderOptions,
    HostOpenOptions, HostSaveOptions,
};
use crate::files::{Picks, ProgramFiles};
use crate::permissions::ProgramSystemPermissions;
use crate::running::RunningProgram;
use crate::set_or_shown::{self, Outcome};
use crate::shell::{FileDialogFuture, ShellFileDialogs, SystemPermission, SystemPermissionState};
use crate::views::ProgramView;

/// What the file dialogs interface reads and writes in a program's store.
pub(crate) struct FileDialogsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a RunningProgram,
    /// The shell's dialogs; none where the shell has none.
    pub(crate) dialogs: Option<&'a std::sync::Arc<dyn ShellFileDialogs>>,
    /// The program's mounts, where an initial folder is looked up.
    pub(crate) files: &'a mut ProgramFiles,
    pub(crate) picks: &'a Picks,
    /// What the system itself allows of what the user picks.
    pub(crate) system_permissions: &'a ProgramSystemPermissions,
}

pub(crate) trait FileDialogsCtxView: Send {
    fn file_dialogs_ctx(&mut self) -> FileDialogsCtx<'_>;
}

struct HasFileDialogs;

impl HasData for HasFileDialogs {
    type Data<'a> = FileDialogsCtx<'a>;
}

pub(crate) fn add_to_linker<T: FileDialogsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    file_dialogs::add_to_linker::<_, HasFileDialogs>(linker, T::file_dialogs_ctx)
}

impl file_dialogs::Host for FileDialogsCtx<'_> {
    fn get_file_dialogs(&mut self) -> wasmtime::Result<Option<Resource<FileDialogs>>> {
        let place = self.dialogs.cloned().map(FileDialogs);
        Ok(place.map(|dialogs| self.table.push(dialogs)).transpose()?)
    }
}

impl FileDialogsCtx<'_> {
    /// Where on the machine the program's `path` is; none outside its mounts.
    fn machine_folder(&mut self, path: Option<PathBuf>) -> Option<PathBuf> {
        self.files.machine_path(&path?, Cause::FileDialog)
    }

    /// The view `parent` names, by its id.
    fn parent_id(&self, parent: Option<&Resource<ProgramView>>) -> wasmtime::Result<Option<u64>> {
        Ok(parent
            .map(|view| self.table.get(view).map(ProgramView::id))
            .transpose()?)
    }
}

/// A type offered, as the program added it.
fn file_type(label: String, extensions: Vec<String>, mime_types: Vec<String>) -> FileType {
    FileType {
        label,
        extensions,
        mime_types,
    }
}

impl HostOpenOptions for FileDialogsCtx<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<OpenOptions>> {
        Ok(self.table.push(OpenOptions::default())?)
    }

    fn add_type(
        &mut self,
        options: Resource<OpenOptions>,
        label: String,
        extensions: Vec<String>,
        mime_types: Vec<String>,
    ) -> wasmtime::Result<()> {
        let types = &mut self.table.get_mut(&options)?.types;
        types.push(file_type(label, extensions, mime_types));
        Ok(())
    }

    /// Kept as the program's path, looked up among its mounts when the dialog opens.
    fn initial_folder(
        &mut self,
        options: Resource<OpenOptions>,
        path: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.initial_folder = Some(PathBuf::from(path));
        Ok(())
    }

    fn confirm_label(
        &mut self,
        options: Resource<OpenOptions>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.confirm_label = Some(label);
        Ok(())
    }

    fn writable(&mut self, options: Resource<OpenOptions>) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.writable = true;
        Ok(())
    }

    fn drop(&mut self, options: Resource<OpenOptions>) -> wasmtime::Result<()> {
        self.table.delete(options)?;
        Ok(())
    }
}

impl HostFolderOptions for FileDialogsCtx<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<FolderOptions>> {
        Ok(self.table.push(FolderOptions::default())?)
    }

    fn initial_folder(
        &mut self,
        options: Resource<FolderOptions>,
        path: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.initial_folder = Some(PathBuf::from(path));
        Ok(())
    }

    fn confirm_label(
        &mut self,
        options: Resource<FolderOptions>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.confirm_label = Some(label);
        Ok(())
    }

    fn writable(&mut self, options: Resource<FolderOptions>) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.writable = true;
        Ok(())
    }

    fn drop(&mut self, options: Resource<FolderOptions>) -> wasmtime::Result<()> {
        self.table.delete(options)?;
        Ok(())
    }
}

impl HostSaveOptions for FileDialogsCtx<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<SaveOptions>> {
        Ok(self.table.push(SaveOptions::default())?)
    }

    fn add_type(
        &mut self,
        options: Resource<SaveOptions>,
        label: String,
        extensions: Vec<String>,
        mime_types: Vec<String>,
    ) -> wasmtime::Result<()> {
        let types = &mut self.table.get_mut(&options)?.types;
        types.push(file_type(label, extensions, mime_types));
        Ok(())
    }

    fn initial_folder(
        &mut self,
        options: Resource<SaveOptions>,
        path: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.initial_folder = Some(PathBuf::from(path));
        Ok(())
    }

    fn suggested_name(
        &mut self,
        options: Resource<SaveOptions>,
        name: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.suggested_name = Some(name);
        Ok(())
    }

    fn confirm_label(
        &mut self,
        options: Resource<SaveOptions>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.confirm_label = Some(label);
        Ok(())
    }

    fn drop(&mut self, options: Resource<SaveOptions>) -> wasmtime::Result<()> {
        self.table.delete(options)?;
        Ok(())
    }
}

impl HostFileDialogs for FileDialogsCtx<'_> {
    fn drop(&mut self, dialogs: Resource<FileDialogs>) -> wasmtime::Result<()> {
        self.table.delete(dialogs)?;
        Ok(())
    }
}

impl<T: FileDialogsCtxView + 'static> HostFileDialogsWithStore<T> for HasFileDialogs {
    async fn open_file(
        accessor: &Accessor<T, Self>,
        dialogs: Resource<FileDialogs>,
        parent: Option<Resource<ProgramView>>,
        options: Resource<OpenOptions>,
    ) -> wasmtime::Result<Result<Option<String>, DialogError>> {
        let (shown, writable, mounting) = accessor.with(|mut access| {
            let mut context = access.get();
            let (parent, options) = open_options(&mut context, parent.as_ref(), options)?;
            let writable = options.writable;
            let shell = Arc::clone(&context.table.get(&dialogs)?.0);
            let program = context.program.clone();
            let shown = one(context.picks, FileDialog::OpenFile, move || {
                shell.open_file(&program, parent, options)
            });
            wasmtime::Result::<_>::Ok((shown, writable, Mounting::of(&context)))
        })?;
        Ok(mounting.mount_picked(shown.await, writable).await)
    }

    async fn open_files(
        accessor: &Accessor<T, Self>,
        dialogs: Resource<FileDialogs>,
        parent: Option<Resource<ProgramView>>,
        options: Resource<OpenOptions>,
    ) -> wasmtime::Result<Result<Vec<String>, DialogError>> {
        let (shown, writable, mounting) = accessor.with(|mut access| {
            let mut context = access.get();
            let (parent, options) = open_options(&mut context, parent.as_ref(), options)?;
            let writable = options.writable;
            let shell = Arc::clone(&context.table.get(&dialogs)?.0);
            let program = context.program.clone();
            let shown = dialog_answer(
                context.picks,
                FileDialog::OpenFiles,
                |paths| paths,
                move || shell.open_files(&program, parent, options),
            );
            wasmtime::Result::<_>::Ok((shown, writable, Mounting::of(&context)))
        })?;
        Ok(mounting.mount_each(shown.await, writable).await)
    }

    async fn open_folder(
        accessor: &Accessor<T, Self>,
        dialogs: Resource<FileDialogs>,
        parent: Option<Resource<ProgramView>>,
        options: Resource<FolderOptions>,
    ) -> wasmtime::Result<Result<Option<String>, DialogError>> {
        let (shown, writable, mounting) = accessor.with(|mut access| {
            let mut context = access.get();
            let parent = context.parent_id(parent.as_ref())?;
            let mut options = context.table.delete(options)?;
            options.initial_folder = context.machine_folder(options.initial_folder.take());
            let writable = options.writable;
            let shell = Arc::clone(&context.table.get(&dialogs)?.0);
            let program = context.program.clone();
            let shown = one(context.picks, FileDialog::OpenFolder, move || {
                shell.open_folder(&program, parent, options)
            });
            wasmtime::Result::<_>::Ok((shown, writable, Mounting::of(&context)))
        })?;
        Ok(mounting.mount_picked(shown.await, writable).await)
    }

    /// The file chosen is mounted writable, whether or not it exists yet.
    async fn save_file(
        accessor: &Accessor<T, Self>,
        dialogs: Resource<FileDialogs>,
        parent: Option<Resource<ProgramView>>,
        options: Resource<SaveOptions>,
    ) -> wasmtime::Result<Result<Option<String>, DialogError>> {
        let (shown, mounting) = accessor.with(|mut access| {
            let mut context = access.get();
            let parent = context.parent_id(parent.as_ref())?;
            let mut options = context.table.delete(options)?;
            options.initial_folder = context.machine_folder(options.initial_folder.take());
            let shell = Arc::clone(&context.table.get(&dialogs)?.0);
            let program = context.program.clone();
            let shown = one(context.picks, FileDialog::SaveFile, move || {
                shell.save_file(&program, parent, options)
            });
            wasmtime::Result::<_>::Ok((shown, Mounting::of(&context)))
        })?;
        Ok(mounting.mount_picked(shown.await, true).await)
    }
}

/// What `dialog`, which answers one path, answers: the one its user set, checked, or else what
/// `show` shows, re-checking the user's setting if grants change while waiting.
fn one(
    picks: &Picks,
    dialog: FileDialog,
    show: impl FnOnce() -> FileDialogFuture<Option<PathBuf>> + Send + 'static,
) -> FileDialogFuture<Option<PathBuf>> {
    dialog_answer(picks, dialog, |paths| paths.into_iter().next(), show)
}

/// What `dialog` answers: the paths its user set, checked and shaped by `map_set`, or else what
/// `show` shows; if grants change while waiting so `dialog` now has a stored answer, `show`'s
/// future is dropped and the stored answer is returned.
fn dialog_answer<T: Send + 'static>(
    picks: &Picks,
    dialog: FileDialog,
    map_set: impl FnOnce(Vec<PathBuf>) -> T + Send + 'static,
    show: impl FnOnce() -> FileDialogFuture<T> + Send + 'static,
) -> FileDialogFuture<T> {
    let grants = picks.grants();
    Box::pin(async move {
        match set_or_shown::stored_or_shown(
            grants,
            move |grants| grants.file_dialog_answer(dialog),
            show,
        )
        .await
        {
            Outcome::Prompt(picked) => picked,
            Outcome::Setting(set) => set_answers::checked(dialog, set).map(map_set),
        }
    })
}

/// The open dialog's parent's id and its options, out of the program's store, the initial folder
/// looked up among its mounts.
fn open_options(
    context: &mut FileDialogsCtx<'_>,
    parent: Option<&Resource<ProgramView>>,
    options: Resource<OpenOptions>,
) -> wasmtime::Result<(Option<u64>, OpenOptions)> {
    let parent = context.parent_id(parent)?;
    let mut options = context.table.delete(options)?;
    options.initial_folder = context.machine_folder(options.initial_folder.take());
    Ok((parent, options))
}

/// What mounts a program's picks once a dialog closes: where they are mounted, and what the system
/// itself allows of them.
struct Mounting {
    picks: Picks,
    system_permissions: ProgramSystemPermissions,
}

impl Mounting {
    fn of(context: &FileDialogsCtx<'_>) -> Mounting {
        Mounting {
            picks: context.picks.clone(),
            system_permissions: context.system_permissions.clone(),
        }
    }

    /// What the user picked, if anything, mounted, as the program sees it; nothing, as a dialog
    /// cancelled, where the system denies WaOS the pick.
    async fn mount_picked(
        &self,
        picked: Result<Option<PathBuf>, DialogError>,
        writable: bool,
    ) -> Result<Option<String>, DialogError> {
        let paths = Vec::from_iter(picked?);
        let mounted = self.mount_each(Ok(paths), writable).await?;
        Ok(mounted.into_iter().next())
    }

    /// Each of the paths the user picked mounted, as the program sees them; none, as a dialog
    /// cancelled, where the system denies WaOS one of them.
    async fn mount_each(
        &self,
        picked: Result<Vec<PathBuf>, DialogError>,
        writable: bool,
    ) -> Result<Vec<String>, DialogError> {
        let paths = picked?;
        if !self.allowed_by_system(&paths).await {
            return Ok(Vec::new());
        }
        (paths.iter())
            .map(|path| mount_one(&self.picks, path, writable))
            .collect()
    }

    /// Whether the system allows WaOS each of `paths`, asked in its own way for one it does not
    /// allow yet.
    async fn allowed_by_system(&self, paths: &[PathBuf]) -> bool {
        for path in paths {
            let permission = SystemPermission::Path(path);
            if self.system_permissions.request(permission).await != SystemPermissionState::Allowed {
                tracing::info!(path = %path.display(), "the system denies WaOS a pick; not mounted");
                return false;
            }
        }
        true
    }
}

/// `path` mounted, as the program sees it; failed for a path a program has no name for, which the
/// user picked but the program cannot be handed.
fn mount_one(picks: &Picks, path: &Path, writable: bool) -> Result<String, DialogError> {
    picks.mount(path, writable).ok_or_else(|| {
        DialogError::Failed(format!(
            "{} is a place programs cannot reach: pick a file on one of this computer's drives",
            path.display()
        ))
    })
}
