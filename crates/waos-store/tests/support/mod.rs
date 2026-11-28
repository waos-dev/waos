//! What the store's tests install and read from: signed packages written to a folder, their
//! listings, and a local HTTP server; and the package manager over the store's directory, with
//! instant stand-ins for its compiler and the running apps, installing as the app does.

#![allow(dead_code, reason = "each test crate uses its own part of the support")]

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::{FutureExt as _, StreamExt as _, stream};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use waos_package::{ListedProgram, Manifest, ProgramId, SigningKey};
use waos_package_manager::{
    Caller, CodeCompiler, InstallError, InstalledProgram, PackageChange, PackageContent,
    PackageManager, StopApp,
};
use waos_permissions::{NothingInstalled, Permissions, SettingsFiles};
use waos_store::{Source, SourceProxy, Store};

/// A compiler that compiles nothing, and finds nothing to compile.
struct NoCompiler;

impl CodeCompiler for NoCompiler {
    fn compile(&self, _component: &Path, _code: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_usable(&self, _code: &Path) -> bool {
        true
    }
}

/// No app runs in the store's tests.
struct NothingRuns;

impl StopApp for NothingRuns {
    fn stop<'a>(&'a self, _id: &'a ProgramId, _change: PackageChange) -> BoxFuture<'a, ()> {
        async {}.boxed()
    }
}

/// The package manager over `store`'s directory, `directory`.
pub fn packages(directory: &Path) -> PackageManager {
    PackageManager::open(directory, Arc::new(NoCompiler), Arc::new(NothingRuns)).unwrap()
}

/// Installs the package at `path` with `packages`, for the user.
pub async fn install(
    packages: &PackageManager,
    path: &Path,
) -> Result<InstalledProgram, InstallError> {
    let bytes = fs::read(path).unwrap();
    let app = waos_package::Package::open(path)
        .unwrap()
        .manifest()
        .id()
        .clone();
    let size = Some(bytes.len() as u64);
    let content = PackageContent::Package {
        app,
        bytes: stream::iter([Ok(bytes)]).boxed(),
        size,
    };
    packages.install(content, Caller::User).finished().await
}

/// Installs `program` with `packages` as `source` lists it, downloaded by `store`.
pub async fn install_from(
    store: &Store,
    packages: &PackageManager,
    source: &Source,
    program: &ListedProgram,
) -> Result<InstalledProgram, InstallError> {
    let download = store.download(source, program).await.unwrap();
    let content = PackageContent::Package {
        app: program.id.clone(),
        bytes: download.bytes,
        size: download.size,
    };
    packages.install(content, Caller::User).finished().await
}

/// Removes the app `id` with `packages`, and `store` forgets it, as the app's startup has the
/// store follow the package manager's removals.
pub async fn remove(store: &Store, packages: &PackageManager, id: &ProgramId) {
    packages.remove(id).await.unwrap();
    store.forget(id);
}

/// The store in `directory`, its apps' activity recorded as the settings files there say.
pub fn open_store(directory: &Path) -> Store {
    let permissions = Permissions::open(Arc::new(SettingsFiles::new(directory)), &NothingInstalled);
    Store::open(directory, SourceProxy::None, permissions).unwrap()
}

/// What `future` comes to, on a runtime of its own: for a test that is not async.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

/// The component of `id` at `version`: bytes that say which they are.
pub fn component_of(id: &str, version: &str) -> Vec<u8> {
    format!("\0asm {id} {version}").into_bytes()
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

/// The listing of the package at `path`, of `id` at `version`, by its file name.
pub fn listing(path: &Path, id: &str, version: &str) -> ListedProgram {
    ListedProgram::new(
        id.parse().unwrap(),
        "Triangle",
        version.parse().unwrap(),
        path.file_name().unwrap().to_string_lossy(),
        Sha256::digest(fs::read(path).unwrap()).into(),
    )
}

/// The start of a PNG of `side` by `side`, all an icon's check reads.
fn png_header(side: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&side.to_be_bytes());
    png.extend_from_slice(&side.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    png
}

/// An HTTP server on this machine, answering `GET`s of the files it was given, and 404 to
/// anything else, from a task of the runtime it started on.
pub struct Server {
    address: SocketAddr,
}

impl Server {
    pub async fn serve(files: Vec<(String, Vec<u8>)>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let files = Arc::new(files.into_iter().collect::<HashMap<_, _>>());
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(answer(stream, files.clone()));
            }
        });
        Server { address }
    }

    /// The URL of the file `path` on this server.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.address)
    }
}

/// Answers the one request `stream` carries, then closes it.
async fn answer(mut stream: TcpStream, files: Arc<HashMap<String, Vec<u8>>>) {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|end| end == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await.unwrap();
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let (status, body) = match files.get(path.trim_start_matches('/')) {
        Some(body) => ("200 OK", body.clone()),
        None => ("404 Not Found", b"not found".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    let _ = stream.shutdown().await;
}
