//! Installing in a test as WaOS installs, through the real package manager, without compiling a
//! component twice in a test run: the tests' package managers are handed [`TestCompiler`], which
//! compiles each component's content once, into a folder named by its SHA-256, and links that
//! code into each version's code folder. A compile is a function of the component's bytes and the
//! compiler alone, so the code is the code an install would compile; and a code file is never
//! changed in place, so a link to it is safe.
//!
//! A test that must see what happens while a compile runs ("Optimizing…", an install's
//! `Installing`) holds the compiler ([`HeldCompiler`]), so the compile waits for the test rather
//! than the test guessing with a timer how long it lasts.
//!
//! The invariant: a test that installs an app installs it through a package manager made here, so
//! no test waits on a compile another already did, and every test runs the package manager's real
//! checks and its one-step install.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

use anyhow::Result;
use futures::StreamExt as _;
use sha2::{Digest, Sha256};
use waos::Compiler;
use waos_package::Manifest;
use waos_package_manager::{
    Caller, CodeCompiler, DevelopmentBuild, InstalledProgram, InstalledVersion, PackageContent,
    PackageManager,
};
use waos_permissions::{NoPrompts, Prompts};
use waos_store::SourceProxy;

use super::app_services::{self, Domains};
use super::running_apps::RunningApps;

/// The smallest component, as text, which the engine compiles as it does a binary: what a test
/// installs where it shows or removes a program but never runs it, since an install compiles, and
/// this compiles at once.
pub(crate) const EMPTY_COMPONENT: &[u8] = b"(component)";

/// The engine's compiler, compiling each component's content once in a test run.
pub(crate) struct TestCompiler(pub(crate) Compiler);

impl CodeCompiler for TestCompiler {
    fn compile(&self, component: &Path, code: &Path) -> Result<()> {
        let compiled = self.compiled(component)?;
        fs::create_dir_all(code)?;
        for file in fs::read_dir(&compiled)? {
            let file = file?.path();
            let linked = code.join(file.file_name().expect("a file in a folder has a name"));
            let _ = fs::remove_file(&linked);
            if fs::hard_link(&file, &linked).is_err() {
                fs::copy(&file, &linked)?;
            }
        }
        Ok(())
    }

    fn is_usable(&self, code: &Path) -> bool {
        self.0.is_compiled(code)
    }
}

impl TestCompiler {
    /// The folder of the code of the component at `component`, compiled there once per content.
    fn compiled(&self, component: &Path) -> Result<PathBuf> {
        let sha256 = hex::encode(Sha256::digest(fs::read(component)?));
        let code = std::env::temp_dir().join("waos-test-code").join(&sha256);
        let turn = turn_of(&sha256);
        let _turn = turn.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.0.is_compiled(&code) {
            self.0.compile(component, &code)?;
        }
        Ok(code)
    }
}

/// The turn of the content `sha256`: one compile of it at a time in this process.
fn turn_of(sha256: &str) -> Arc<Mutex<()>> {
    static TURNS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut turns = TURNS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Arc::clone(turns.entry(sha256.to_owned()).or_default())
}

/// The tests' compiler, whose compiles wait while a test holds it.
#[derive(Default)]
pub(crate) struct HeldCompiler {
    /// How many holds are taken.
    holds: Mutex<usize>,
    released: Condvar,
}

/// A hold on a [`HeldCompiler`]: its compiles wait until this is dropped.
pub(crate) struct CompilerHold(Arc<HeldCompiler>);

impl HeldCompiler {
    /// Holds the compiler: a compile begun from now waits until the hold is dropped.
    pub(crate) fn hold(self: &Arc<Self>) -> CompilerHold {
        *self.holds.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        CompilerHold(Arc::clone(self))
    }

    /// Returns once no hold is taken.
    fn wait_for_release(&self) {
        let holds = self.holds.lock().unwrap_or_else(PoisonError::into_inner);
        let _released = self
            .released
            .wait_while(holds, |holds| *holds > 0)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

impl Drop for CompilerHold {
    fn drop(&mut self) {
        *self.0.holds.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
        self.0.released.notify_all();
    }
}

impl CodeCompiler for HeldCompiler {
    fn compile(&self, component: &Path, code: &Path) -> Result<()> {
        self.wait_for_release();
        test_compiler().compile(component, code)
    }

    fn is_usable(&self, code: &Path) -> bool {
        test_compiler().is_usable(code)
    }
}

/// The tests' compiler, set up as the tests' engines are, so they map its code.
fn test_compiler() -> TestCompiler {
    static COMPILER: OnceLock<Compiler> = OnceLock::new();
    let compiler = COMPILER.get_or_init(|| Compiler::new(false).expect("a compiler"));
    TestCompiler(compiler.clone())
}

/// A compiler for a test that never runs what it installs: it writes a marker in the code folder,
/// which is all it takes to be usable.
pub(crate) struct MarkerCompiler;

/// The file [`MarkerCompiler`] writes.
const MARKER: &str = "compiled";

impl CodeCompiler for MarkerCompiler {
    fn compile(&self, _component: &Path, code: &Path) -> Result<()> {
        fs::create_dir_all(code)?;
        fs::write(code.join(MARKER), b"")?;
        Ok(())
    }

    fn is_usable(&self, code: &Path) -> bool {
        code.join(MARKER).is_file()
    }
}

/// A test's domains over `folder`, the store's directory, as WaOS opens them, for apps it never
/// runs, with nobody to ask.
pub(crate) fn domains_never_run(folder: &Path) -> Domains {
    let running = Arc::new(RunningApps::default());
    let (compiler, prompts) = (Arc::new(MarkerCompiler), Arc::new(NoPrompts));
    app_services::open_domains(folder, compiler, &running, SourceProxy::None, prompts)
        .expect("the domains open")
}

/// A test's domains over `folder`, the store's directory, as WaOS opens them, its package manager
/// compiling once per content with a compiler set up as the tests' engines are, so they map its
/// code, stopping apps among `running`, and Permissions asking the user through `prompts`.
pub(crate) fn domains(
    folder: &Path,
    running: &Arc<RunningApps>,
    prompts: Arc<dyn Prompts>,
) -> Domains {
    domains_compiling(folder, running, prompts, Arc::new(test_compiler()))
}

/// [`domains`], compiling with `compiler`: a [`HeldCompiler`] for a test that holds it.
pub(crate) fn domains_compiling(
    folder: &Path,
    running: &Arc<RunningApps>,
    prompts: Arc<dyn Prompts>,
    compiler: Arc<dyn CodeCompiler>,
) -> Domains {
    app_services::open_domains(folder, compiler, running, SourceProxy::None, prompts)
        .expect("the domains open")
}

/// Installs the package at `file` with `packages`, for the user, and waits until it is installed,
/// on a thread and a runtime of its own, so a test on a runtime or off one calls it alike.
pub(crate) fn install_package(packages: &PackageManager, file: &Path) -> InstalledProgram {
    let bytes = fs::read(file).unwrap();
    let app = waos_package::Package::open(file)
        .expect("a package")
        .manifest()
        .id()
        .clone();
    let size = Some(bytes.len() as u64);
    let content = PackageContent::Package {
        app,
        bytes: futures::stream::iter([Ok(bytes)]).boxed(),
        size,
    };
    wait_installed(packages, content, Caller::User)
}

/// Installs the build of `manifest`, its component the test program at `component`, as its
/// development install, as `install_package` does.
pub(crate) fn install_development(
    packages: &PackageManager,
    manifest: Manifest,
    component: &str,
) -> InstalledProgram {
    let build = DevelopmentBuild {
        manifest,
        component: fs::read(component).unwrap(),
        icon: None,
    };
    wait_installed(
        packages,
        PackageContent::Development(build),
        Caller::DeveloperTools,
    )
}

/// What `content` comes to, installed for `caller`, waited for on a thread and a runtime of its
/// own.
fn wait_installed(
    packages: &PackageManager,
    content: PackageContent,
    caller: Caller,
) -> InstalledProgram {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async { packages.install(content, caller).finished().await })
            })
            .join()
            .unwrap()
    })
    .expect("the app installs")
}

/// The version of `id` installed now in `packages`, read under its lock, on a runtime of its own.
pub(crate) fn installed_version(packages: &PackageManager, id: &str) -> InstalledVersion {
    let id = id.parse().unwrap();
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap()
                    .block_on(async { packages.lock(&id).await.version() })
            })
            .join()
            .unwrap()
    })
    .expect("the app is installed")
}

/// Spoils `version`, installed, as a disk that lost its files would: its component and every code
/// file in its code folder, so the engine neither maps the code nor compiles it again.
pub(crate) fn spoil(version: &InstalledVersion) {
    fs::write(version.component(), b"spoiled").unwrap();
    for file in fs::read_dir(version.code()).unwrap() {
        let file = file.unwrap().path();
        // A link's file is shared with every test that compiled the same content: unlink first.
        fs::remove_file(&file).unwrap();
        fs::write(file, b"spoiled").unwrap();
    }
}
