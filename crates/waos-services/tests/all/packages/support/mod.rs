//! What the package manager's tests install with: the real manager over a throwaway folder, with
//! instant stand-ins for what it is handed (a compiler that writes a marker file, and a way to
//! stop apps that records each stop), and signed packages written to a folder.

#![allow(dead_code, reason = "each test crate uses its own part of the support")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::FutureExt as _;
use futures::channel::mpsc;
use futures::future::BoxFuture;
use futures::stream::{self, StreamExt as _};
use tempfile::TempDir;
use waos_package::{Manifest, ProgramId, SigningKey};
use waos_services::packages::{
    ByteStream, Caller, CodeCompiler, DevelopmentBuild, InstallError, InstalledAppsFiles,
    InstalledProgram, PackageChange, PackageContent, PackageManager, StopApp,
};

pub const TRIANGLE: &str = "dev.example.triangle";

pub fn triangle() -> ProgramId {
    TRIANGLE.parse().unwrap()
}

/// The file the stand-in compiler writes in a code folder, holding the component it compiled.
pub const MARKER: &str = "compiled";

/// What a component says to make the stand-in compiler refuse it.
pub const UNCOMPILABLE: &[u8] = b"\0asm does not compile";

/// A compiler that copies the component into a marker file, refusing [`UNCOMPILABLE`], and counts
/// its compiles.
#[derive(Default)]
pub struct MarkerCompiler {
    pub compiles: Mutex<usize>,
}

impl CodeCompiler for MarkerCompiler {
    fn compile(&self, component: &Path, code: &Path) -> anyhow::Result<()> {
        *self.compiles.lock().unwrap() += 1;
        let bytes = fs::read(component)?;
        anyhow::ensure!(bytes != UNCOMPILABLE, "the stand-in refuses it");
        fs::create_dir_all(code)?;
        fs::write(code.join(MARKER), bytes)?;
        Ok(())
    }

    fn is_usable(&self, code: &Path) -> bool {
        code.join(MARKER).is_file()
    }
}

/// Records each stop it is asked for, and stops nothing.
#[derive(Default)]
pub struct RecordedStops {
    pub stops: Mutex<Vec<(ProgramId, PackageChange)>>,
}

impl StopApp for RecordedStops {
    fn stop<'a>(&'a self, id: &'a ProgramId, change: PackageChange) -> BoxFuture<'a, ()> {
        self.stops.lock().unwrap().push((id.clone(), change));
        async {}.boxed()
    }
}

/// A package manager over a throwaway folder, with what it was handed.
pub struct Manager {
    pub packages: PackageManager,
    pub compiler: Arc<MarkerCompiler>,
    pub stops: Arc<RecordedStops>,
    pub folder: PathBuf,
    pub scratch: TempDir,
}

impl Manager {
    pub fn new() -> Manager {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("store");
        let compiler = Arc::new(MarkerCompiler::default());
        let stops = Arc::new(RecordedStops::default());
        let packages = open(&folder, &compiler, &stops);
        Manager {
            packages,
            compiler,
            stops,
            folder,
            scratch,
        }
    }

    /// The package manager opened again over the same folder, as after a restart.
    pub fn reopened(&self) -> PackageManager {
        open(&self.folder, &self.compiler, &self.stops)
    }

    /// The triangle at `version`, signed with `key`, as a package's bytes.
    pub fn package(&self, version: &str, key: &SigningKey) -> Vec<u8> {
        package_bytes(self.scratch.path(), TRIANGLE, version, key)
    }

    /// Installs `bytes`, a package of the triangle, for the user, and waits for the answer.
    pub async fn install(&self, bytes: Vec<u8>) -> Result<InstalledProgram, InstallError> {
        self.packages
            .install(content(TRIANGLE, bytes), Caller::User)
            .finished()
            .await
    }

    /// The versions installed, by id.
    pub fn installed(&self) -> Vec<(String, String)> {
        self.packages
            .installed()
            .into_iter()
            .map(|program| {
                let manifest = program.manifest;
                (manifest.id().to_string(), manifest.version().to_string())
            })
            .collect()
    }

    /// The folder of the app `id`.
    pub fn program_folder(&self, id: &ProgramId) -> PathBuf {
        self.folder.join("programs").join(id.as_str())
    }
}

fn open(
    folder: &Path,
    compiler: &Arc<MarkerCompiler>,
    stops: &Arc<RecordedStops>,
) -> PackageManager {
    PackageManager::new(
        Arc::new(InstalledAppsFiles::open(folder).unwrap()),
        Arc::clone(compiler) as Arc<dyn CodeCompiler>,
        Arc::clone(stops) as Arc<dyn StopApp>,
    )
}

pub fn triangle_at(version: &str) -> Vec<(String, String)> {
    vec![(TRIANGLE.to_owned(), version.to_owned())]
}

/// A package of `app`, its bytes sent whole.
pub fn content(app: &str, bytes: Vec<u8>) -> PackageContent {
    let size = Some(bytes.len() as u64);
    PackageContent::Package {
        app: app.parse().unwrap(),
        bytes: stream::iter([Ok(bytes)]).boxed(),
        size,
    }
}

/// A package of the triangle whose bytes are sent through the answer's sender, as a download
/// sends them, ending when the sender is dropped.
pub fn sent(
    size: Option<u64>,
) -> (
    PackageContent,
    mpsc::UnboundedSender<std::io::Result<Vec<u8>>>,
) {
    let (sender, receiver) = mpsc::unbounded();
    let bytes: ByteStream = receiver.boxed();
    let content = PackageContent::Package {
        app: triangle(),
        bytes,
        size,
    };
    (content, sender)
}

/// The triangle's development build at `version`, its component `component`, with no icon.
pub fn build(version: &str, component: &[u8]) -> DevelopmentBuild {
    DevelopmentBuild {
        manifest: Manifest::new(triangle(), "Triangle", version.parse().unwrap()).unwrap(),
        component: component.to_vec(),
        icon: None,
    }
}

/// The component of `id` at `version`: bytes that say which they are.
pub fn component_of(id: &str, version: &str) -> Vec<u8> {
    format!("\0asm {id} {version}").into_bytes()
}

/// The bytes of a package of `id` at `version`, signed with `key`, written in `folder` first.
pub fn package_bytes(folder: &Path, id: &str, version: &str, key: &SigningKey) -> Vec<u8> {
    fs::read(write_package(folder, id, version, key)).unwrap()
}

/// A package of `id` at `version`, signed with `key`, written in `folder` as
/// `<id>-<version>.wapk`.
pub fn write_package(folder: &Path, id: &str, version: &str, key: &SigningKey) -> PathBuf {
    let manifest =
        Manifest::new(id.parse().unwrap(), "Triangle", version.parse().unwrap()).unwrap();
    let component = folder.join(format!("{id}-{version}.wasm"));
    fs::write(&component, component_of(id, version)).unwrap();
    let icon = folder.join("icon.png");
    fs::write(&icon, png_header(256)).unwrap();
    let path = folder.join(format!("{id}-{version}.wapk"));
    waos_package::write(&path, &manifest, &component, &icon, key).unwrap();
    path
}

/// The start of a PNG of `side` by `side`, all an icon's check reads.
fn png_header(side: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&side.to_be_bytes());
    png.extend_from_slice(&side.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    png
}
